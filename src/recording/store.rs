//! SQLite storage — local-only, zero external dependencies.

use std::sync::Mutex;

use rusqlite::Connection;
use tracing::{debug, info};

use super::model::CallRecord;

/// Thread-safe SQLite store.
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// Open (or create) the SQLite database and run migrations.
    pub fn new(db_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let conn = Connection::open(db_path)?;

        // Enable WAL mode for better concurrent reads
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")?;

        // Run migrations
        Self::migrate(&conn)?;

        info!("SQLite store ready at {db_path}");

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn migrate(conn: &Connection) -> Result<(), rusqlite::Error> {
        // Main schema — create tables if they don't exist
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS calls (
                id              TEXT PRIMARY KEY,
                timestamp       INTEGER NOT NULL,
                model           TEXT NOT NULL,
                provider        TEXT NOT NULL,
                complexity      TEXT NOT NULL DEFAULT 'medium',
                prompt_tokens   INTEGER NOT NULL DEFAULT 0,
                completion_tokens INTEGER NOT NULL DEFAULT 0,
                cost_usd        REAL NOT NULL DEFAULT 0.0,
                latency_ms      INTEGER NOT NULL DEFAULT 0,
                fallback_used   INTEGER NOT NULL DEFAULT 0,
                prompt_hash     TEXT NOT NULL DEFAULT '',
                finish_reason   TEXT,
                was_routed      INTEGER NOT NULL DEFAULT 0,
                recommended_model TEXT,
                estimated_optimal_cost REAL
            );

            CREATE INDEX IF NOT EXISTS idx_calls_ts ON calls(timestamp);
            CREATE INDEX IF NOT EXISTS idx_calls_model ON calls(model);
            CREATE INDEX IF NOT EXISTS idx_calls_complexity ON calls(complexity);

            CREATE TABLE IF NOT EXISTS daily_stats (
                date              TEXT PRIMARY KEY,
                total_calls       INTEGER NOT NULL DEFAULT 0,
                total_prompt_tokens  INTEGER NOT NULL DEFAULT 0,
                total_completion_tokens INTEGER NOT NULL DEFAULT 0,
                total_cost_usd     REAL NOT NULL DEFAULT 0.0,
                estimated_savings_usd REAL NOT NULL DEFAULT 0.0
            );
            ",
        )?;

        // v0.1.0 → v0.1.1 column additions (ignore errors if already present)
        for col_sql in [
            "ALTER TABLE calls ADD COLUMN was_routed INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE calls ADD COLUMN recommended_model TEXT",
            "ALTER TABLE calls ADD COLUMN estimated_optimal_cost REAL",
        ] {
            let _ = conn.execute(col_sql, []);
        }
        Ok(())
    }

    /// Record a single API call.
    pub fn record_call(
        &self,
        rec: &CallRecord,
        request_json: &serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let conn = self.conn.lock().unwrap();

        // Extract prompt text for hashing
        let prompt_text = request_json
            .get("messages")
            .and_then(|m| m.as_array())
            .map(|msgs| {
                msgs.iter()
                    .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();

        let prompt_hash = CallRecord::hash_prompt(&prompt_text);

        conn.execute(
            "INSERT INTO calls (id, timestamp, model, provider, complexity, prompt_tokens, completion_tokens, cost_usd, latency_ms, fallback_used, prompt_hash, finish_reason, was_routed, recommended_model, estimated_optimal_cost)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            rusqlite::params![
                rec.id,
                rec.timestamp,
                rec.model,
                rec.provider,
                rec.complexity,
                rec.prompt_tokens,
                rec.completion_tokens,
                rec.cost_usd,
                rec.latency_ms,
                rec.fallback_used as i32,
                prompt_hash,
                rec.finish_reason,
                rec.was_routed as i32,
                rec.recommended_model,
                rec.estimated_optimal_cost,
            ],
        )?;

        debug!("Recorded call {} ({}/{})", rec.id, rec.provider, rec.model);

        Ok(())
    }

    /// Get total calls for today.
    #[allow(dead_code)]
    pub fn today_call_count(&self) -> Result<i64, Box<dyn std::error::Error>> {
        let conn = self.conn.lock().unwrap();
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM calls WHERE date(timestamp, 'unixepoch') = ?1",
            rusqlite::params![today],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Get recent calls.
    pub fn recent_calls(
        &self,
        limit: usize,
    ) -> Result<Vec<CallRecord>, Box<dyn std::error::Error>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, model, provider, complexity, prompt_tokens, completion_tokens, cost_usd, latency_ms, fallback_used, prompt_hash, finish_reason, was_routed, recommended_model, estimated_optimal_cost
             FROM calls ORDER BY timestamp DESC LIMIT ?1",
        )?;

        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(CallRecord {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                model: row.get(2)?,
                provider: row.get(3)?,
                complexity: row.get(4)?,
                prompt_tokens: row.get(5)?,
                completion_tokens: row.get(6)?,
                cost_usd: row.get(7)?,
                latency_ms: row.get(8)?,
                fallback_used: row.get::<_, i32>(9)? != 0,
                prompt_hash: row.get(10)?,
                finish_reason: row.get(11)?,
                was_routed: row.get::<_, i32>(12)? != 0,
                recommended_model: row.get(13)?,
                estimated_optimal_cost: row.get(14)?,
            })
        })?;

        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Get aggregate stats for the current month.
    pub fn monthly_stats(&self) -> Result<MonthlyStats, Box<dyn std::error::Error>> {
        let conn = self.conn.lock().unwrap();
        let month_start = chrono::Utc::now().format("%Y-%m-01").to_string();

        let stats = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(prompt_tokens), 0), COALESCE(SUM(completion_tokens), 0), COALESCE(SUM(cost_usd), 0.0), COALESCE(AVG(latency_ms), 0),
                    COALESCE(SUM(CASE WHEN was_routed = 0 AND estimated_optimal_cost IS NOT NULL THEN cost_usd - estimated_optimal_cost ELSE 0 END), 0.0)
             FROM calls WHERE date(timestamp, 'unixepoch') >= ?1",
            rusqlite::params![month_start],
            |row| {
                Ok(MonthlyStats {
                    total_calls: row.get(0)?,
                    total_prompt_tokens: row.get(1)?,
                    total_completion_tokens: row.get(2)?,
                    total_cost: row.get(3)?,
                    avg_latency_ms: row.get(4)?,
                    potential_savings: row.get(5)?,
                })
            },
        )?;

        Ok(stats)
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MonthlyStats {
    pub total_calls: i64,
    pub total_prompt_tokens: i64,
    pub total_completion_tokens: i64,
    pub total_cost: f64,
    pub avg_latency_ms: f64,
    /// What Free-tier users could save with Pro routing.
    pub potential_savings: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::model::CallRecord;

    fn test_rec() -> CallRecord {
        CallRecord::from_request("test-model", "test-provider", "simple", false, 150)
    }

    #[test]
    fn test_store_open_and_record() {
        let store = Store::new(":memory:").expect("Failed to open in-memory store");
        let rec = test_rec();
        let request = serde_json::json!({"messages": [{"role": "user", "content": "Hello"}]});
        store
            .record_call(&rec, &request)
            .expect("record_call failed");
        let recent = store.recent_calls(10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].model, "test-model");
    }

    #[test]
    fn test_monthly_stats_empty() {
        let store = Store::new(":memory:").expect("Failed to open in-memory store");
        let stats = store.monthly_stats().unwrap();
        assert_eq!(stats.total_calls, 0);
        assert_eq!(stats.total_cost, 0.0);
    }

    #[test]
    fn test_today_call_count() {
        let store = Store::new(":memory:").expect("Failed to open in-memory store");
        let rec = test_rec();
        let request = serde_json::json!({"messages": []});
        store.record_call(&rec, &request).unwrap();
        let count = store.today_call_count().unwrap();
        assert!(count >= 1);
    }
}
