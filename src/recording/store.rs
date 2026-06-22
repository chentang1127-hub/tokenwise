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

    /// Quick health check — runs a simple query to verify DB connectivity.
    pub fn health_check(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("lock: {e}"))?;
        conn.query_row("SELECT 1", [], |_| Ok(()))
            .map_err(|e| format!("query: {e}"))?;
        Ok(())
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
                estimated_optimal_cost REAL,
                tenant_id       TEXT NOT NULL DEFAULT 'anon'
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

            CREATE TABLE IF NOT EXISTS cache (
                hash               TEXT PRIMARY KEY,
                response_json      TEXT NOT NULL,
                model              TEXT NOT NULL,
                prompt_tokens      INTEGER NOT NULL DEFAULT 0,
                completion_tokens  INTEGER NOT NULL DEFAULT 0,
                created_at         INTEGER NOT NULL,
                hit_count          INTEGER NOT NULL DEFAULT 1
            );

            CREATE INDEX IF NOT EXISTS idx_cache_ts ON cache(created_at);
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
        // v0.2.0: multi-tenant support
        let _ = conn.execute(
            "ALTER TABLE calls ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'anon'",
            [],
        );
        // Add index for tenant-scoped queries
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_calls_tenant ON calls(tenant_id)",
            [],
        );
        Ok(())
    }

    /// Force WAL checkpoint — flushes all data to the main DB file.
    /// Call on graceful shutdown to prevent data loss.
    pub fn checkpoint(&self) {
        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        }
    }

    // ── Response cache (Pro feature) ─────────────────

    /// Check the cache for a matching prompt hash.
    /// Returns the cached response JSON string if found and not expired.
    pub fn cache_get(&self, hash: &str, ttl_hours: u32) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        let cutoff = chrono::Utc::now().timestamp() - (ttl_hours as i64 * 3600);
        let result: Option<String> = conn
            .query_row(
                "SELECT response_json FROM cache WHERE hash = ?1 AND created_at > ?2
                 ORDER BY created_at DESC LIMIT 1",
                rusqlite::params![hash, cutoff],
                |row| row.get(0),
            )
            .ok()?;

        // Increment hit count
        if result.is_some() {
            let _ = conn.execute(
                "UPDATE cache SET hit_count = hit_count + 1 WHERE hash = ?1",
                rusqlite::params![hash],
            );
        }
        result
    }

    /// Store a response in the cache.
    pub fn cache_put(
        &self,
        hash: &str,
        response_json: &str,
        model: &str,
        prompt_tokens: u32,
        completion_tokens: u32,
        max_entries: u32,
    ) {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();

        // Evict old entries if over capacity
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM cache", [], |row| row.get(0))
            .unwrap_or(0);
        if count >= max_entries as i64 {
            // Remove oldest 10% of entries
            let to_remove = (max_entries as f64 * 0.1) as i64 + 1;
            let _ = conn.execute(
                "DELETE FROM cache WHERE hash IN (SELECT hash FROM cache ORDER BY created_at ASC LIMIT ?1)",
                rusqlite::params![to_remove],
            );
        }

        // Upsert
        let _ = conn.execute(
            "INSERT OR REPLACE INTO cache (hash, response_json, model, prompt_tokens, completion_tokens, created_at, hit_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
            rusqlite::params![hash, response_json, model, prompt_tokens, completion_tokens, now],
        );
    }

    /// Get cache statistics.
    pub fn cache_stats(&self) -> CacheStats {
        let conn = self.conn.lock().unwrap();
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM cache", [], |row| row.get(0))
            .unwrap_or(0);
        let total_hits: i64 = conn
            .query_row("SELECT COALESCE(SUM(hit_count), 0) FROM cache", [], |row| {
                row.get(0)
            })
            .unwrap_or(0);
        // Estimate savings: sum of (prompt_tokens * cheapest_rate) for cached entries
        CacheStats {
            total_entries: total,
            total_hits,
        }
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
            "INSERT INTO calls (id, timestamp, model, provider, complexity, prompt_tokens, completion_tokens, cost_usd, latency_ms, fallback_used, prompt_hash, finish_reason, was_routed, recommended_model, estimated_optimal_cost, tenant_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
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
                rec.tenant_id,
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
        tenant_id: Option<&str>,
    ) -> Result<Vec<CallRecord>, Box<dyn std::error::Error>> {
        let conn = self.conn.lock().unwrap();
        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(tid) =
            tenant_id
        {
            (
                "SELECT id, timestamp, model, provider, complexity, prompt_tokens, completion_tokens, cost_usd, latency_ms, fallback_used, prompt_hash, finish_reason, was_routed, recommended_model, estimated_optimal_cost, tenant_id
                 FROM calls WHERE tenant_id = ?1 ORDER BY timestamp DESC LIMIT ?2".to_string(),
                vec![Box::new(tid.to_string()), Box::new(limit as i64)],
            )
        } else {
            (
                "SELECT id, timestamp, model, provider, complexity, prompt_tokens, completion_tokens, cost_usd, latency_ms, fallback_used, prompt_hash, finish_reason, was_routed, recommended_model, estimated_optimal_cost, tenant_id
                 FROM calls ORDER BY timestamp DESC LIMIT ?1".to_string(),
                vec![Box::new(limit as i64)],
            )
        };
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;

        let rows = stmt.query_map(param_refs.as_slice(), |row| {
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
                tenant_id: row.get(15)?,
            })
        })?;

        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Estimate dollars saved by cache hits this month.
    /// For each cached entry with hit_count > 1, each extra hit
    /// avoided an API call. Uses stored token counts × a conservative
    /// nominal rate ($0.00015/1K prompt + $0.0006/1K completion).
    pub fn cache_savings_estimate(&self) -> f64 {
        let conn = self.conn.lock().unwrap();
        // Conservative rate: roughly gemini-flash level, cheaper than any real model
        const NOMINAL_PROMPT_RATE: f64 = 0.00015;
        const NOMINAL_COMPLETION_RATE: f64 = 0.0006;
        let savings: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(
                    (hit_count - 1) *
                    (prompt_tokens * ?1 / 1000.0 + completion_tokens * ?2 / 1000.0)
                 ), 0.0) FROM cache WHERE hit_count > 1",
                rusqlite::params![NOMINAL_PROMPT_RATE, NOMINAL_COMPLETION_RATE],
                |row| row.get(0),
            )
            .unwrap_or(0.0);
        savings
    }

    /// Count routed calls this month.
    pub fn routing_count(&self, tenant_id: Option<&str>) -> i64 {
        let conn = self.conn.lock().unwrap();
        let month_start = chrono::Utc::now().format("%Y-%m-01").to_string();
        if let Some(tid) = tenant_id {
            conn.query_row(
                "SELECT COUNT(*) FROM calls WHERE was_routed = 1 AND date(timestamp, 'unixepoch') >= ?1 AND tenant_id = ?2",
                rusqlite::params![month_start, tid],
                |row| row.get(0),
            ).unwrap_or(0)
        } else {
            conn.query_row(
                "SELECT COUNT(*) FROM calls WHERE was_routed = 1 AND date(timestamp, 'unixepoch') >= ?1",
                rusqlite::params![month_start],
                |row| row.get(0),
            ).unwrap_or(0)
        }
    }

    /// Count distinct models used this month.
    pub fn distinct_models(&self, tenant_id: Option<&str>) -> i64 {
        let conn = self.conn.lock().unwrap();
        let month_start = chrono::Utc::now().format("%Y-%m-01").to_string();
        if let Some(tid) = tenant_id {
            conn.query_row(
                "SELECT COUNT(DISTINCT model) FROM calls WHERE date(timestamp, 'unixepoch') >= ?1 AND tenant_id = ?2",
                rusqlite::params![month_start, tid],
                |row| row.get(0),
            ).unwrap_or(0)
        } else {
            conn.query_row(
                "SELECT COUNT(DISTINCT model) FROM calls WHERE date(timestamp, 'unixepoch') >= ?1",
                rusqlite::params![month_start],
                |row| row.get(0),
            )
            .unwrap_or(0)
        }
    }

    /// Get calls with optional filters: time range, complexity, decision type, pagination.
    pub fn recent_calls_filtered(
        &self,
        limit: usize,
        offset: usize,
        range_hours: Option<u32>,
        complexity: Option<&str>,
        decision: Option<&str>,
        tenant_id: Option<&str>,
    ) -> Result<Vec<CallRecord>, Box<dyn std::error::Error>> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();

        // Build query dynamically but with parameterized values
        let base = "SELECT id, timestamp, model, provider, complexity, prompt_tokens, completion_tokens, cost_usd, latency_ms, fallback_used, prompt_hash, finish_reason, was_routed, recommended_model, estimated_optimal_cost, tenant_id FROM calls WHERE 1=1";
        let mut conditions = String::new();
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(h) = range_hours {
            conditions.push_str(" AND timestamp >= ?");
            params.push(Box::new(now - (h as i64 * 3600)));
        }
        if let Some(c) = complexity {
            conditions.push_str(" AND complexity = ?");
            params.push(Box::new(c.to_string()));
        }
        match decision {
            Some("eliminated") => {
                // Cache hits: zero cost + near-instant latency
                conditions.push_str(" AND cost_usd = 0.0 AND latency_ms < 10");
            }
            Some("routed") => {
                conditions.push_str(" AND was_routed = 1");
            }
            Some("direct") => {
                conditions.push_str(" AND was_routed = 0 AND (cost_usd > 0.0 OR latency_ms >= 10)");
            }
            _ => {}
        }
        if let Some(tid) = tenant_id {
            conditions.push_str(" AND tenant_id = ?");
            params.push(Box::new(tid.to_string()));
        }

        let sql = format!("{base}{conditions} ORDER BY timestamp DESC LIMIT ? OFFSET ?");
        params.push(Box::new(limit as i64));
        params.push(Box::new(offset as i64));

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
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
                tenant_id: row.get(15)?,
            })
        })?;

        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Count calls matching filters (for pagination).
    pub fn calls_count_filtered(
        &self,
        range_hours: Option<u32>,
        complexity: Option<&str>,
        decision: Option<&str>,
        tenant_id: Option<&str>,
    ) -> Result<i64, Box<dyn std::error::Error>> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();

        let base = "SELECT COUNT(*) FROM calls WHERE 1=1";
        let mut conditions = String::new();
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(h) = range_hours {
            conditions.push_str(" AND timestamp >= ?");
            params.push(Box::new(now - (h as i64 * 3600)));
        }
        if let Some(c) = complexity {
            conditions.push_str(" AND complexity = ?");
            params.push(Box::new(c.to_string()));
        }
        match decision {
            Some("eliminated") => {
                conditions.push_str(" AND cost_usd = 0.0 AND latency_ms < 10");
            }
            Some("routed") => {
                conditions.push_str(" AND was_routed = 1");
            }
            Some("direct") => {
                conditions.push_str(" AND was_routed = 0 AND (cost_usd > 0.0 OR latency_ms >= 10)");
            }
            _ => {}
        }
        if let Some(tid) = tenant_id {
            conditions.push_str(" AND tenant_id = ?");
            params.push(Box::new(tid.to_string()));
        }

        let sql = format!("{base}{conditions}");
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let count: i64 = conn.query_row(&sql, param_refs.as_slice(), |row| row.get(0))?;
        Ok(count)
    }

    /// Aggregate stats for a time range (for the calls page summary bar).
    pub fn calls_summary(
        &self,
        range_hours: Option<u32>,
        tenant_id: Option<&str>,
    ) -> Result<CallsSummary, Box<dyn std::error::Error>> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();

        let mut conditions = if let Some(h) = range_hours {
            format!("timestamp >= {}", now - (h as i64 * 3600))
        } else {
            "1=1".to_string()
        };
        if let Some(tid) = tenant_id {
            conditions.push_str(&format!(" AND tenant_id = '{}'", tid.replace('\'', "''")));
        }

        let sql = format!(
            "SELECT COUNT(*), COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0),
                    COALESCE(SUM(cost_usd),0.0), COALESCE(AVG(latency_ms),0),
                    COALESCE(SUM(CASE WHEN cost_usd = 0.0 AND latency_ms < 10 THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN was_routed = 1 THEN 1 ELSE 0 END),0)
             FROM calls WHERE {conditions}"
        );

        let stats = conn.query_row(&sql, [], |row| {
            Ok(CallsSummary {
                total: row.get(0)?,
                total_prompt_tokens: row.get(1)?,
                total_completion_tokens: row.get(2)?,
                total_cost: row.get(3)?,
                avg_latency_ms: row.get(4)?,
                eliminated_count: row.get(5)?,
                routed_count: row.get(6)?,
            })
        })?;

        Ok(stats)
    }

    /// Token distribution by model for the current month (for charts).
    pub fn token_distribution(
        &self,
        tenant_id: Option<&str>,
    ) -> Result<Vec<ModelTokenStats>, Box<dyn std::error::Error>> {
        let conn = self.conn.lock().unwrap();
        let month_start = chrono::Utc::now().format("%Y-%m-01").to_string();
        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(tid) =
            tenant_id
        {
            (
                "SELECT model, COUNT(*), COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0), COALESCE(SUM(cost_usd),0.0)
                 FROM calls WHERE date(timestamp, 'unixepoch') >= ?1 AND tenant_id = ?2
                 GROUP BY model ORDER BY SUM(cost_usd) DESC LIMIT 10".to_string(),
                vec![Box::new(month_start), Box::new(tid.to_string())],
            )
        } else {
            (
                "SELECT model, COUNT(*), COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0), COALESCE(SUM(cost_usd),0.0)
                 FROM calls WHERE date(timestamp, 'unixepoch') >= ?1
                 GROUP BY model ORDER BY SUM(cost_usd) DESC LIMIT 10".to_string(),
                vec![Box::new(month_start)],
            )
        };
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;

        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            Ok(ModelTokenStats {
                model: row.get(0)?,
                call_count: row.get(1)?,
                prompt_tokens: row.get(2)?,
                completion_tokens: row.get(3)?,
                total_cost: row.get(4)?,
            })
        })?;

        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Check if total cost exceeds budget limit for a time window.
    pub fn total_cost_since(&self, since_ts: i64) -> f64 {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COALESCE(SUM(cost_usd), 0.0) FROM calls WHERE timestamp >= ?1",
            rusqlite::params![since_ts],
            |row| row.get(0),
        )
        .unwrap_or(0.0)
    }

    /// Get aggregate stats for the current month.
    pub fn monthly_stats(
        &self,
        tenant_id: Option<&str>,
    ) -> Result<MonthlyStats, Box<dyn std::error::Error>> {
        let conn = self.conn.lock().unwrap();
        let month_start = chrono::Utc::now().format("%Y-%m-01").to_string();

        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(tid) =
            tenant_id
        {
            (
                "SELECT COUNT(*), COALESCE(SUM(prompt_tokens), 0), COALESCE(SUM(completion_tokens), 0), COALESCE(SUM(cost_usd), 0.0), COALESCE(AVG(latency_ms), 0),
                        COALESCE(SUM(CASE WHEN was_routed = 0 AND estimated_optimal_cost IS NOT NULL THEN cost_usd - estimated_optimal_cost ELSE 0 END), 0.0)
                 FROM calls WHERE date(timestamp, 'unixepoch') >= ?1 AND tenant_id = ?2".to_string(),
                vec![Box::new(month_start), Box::new(tid.to_string())],
            )
        } else {
            (
                "SELECT COUNT(*), COALESCE(SUM(prompt_tokens), 0), COALESCE(SUM(completion_tokens), 0), COALESCE(SUM(cost_usd), 0.0), COALESCE(AVG(latency_ms), 0),
                        COALESCE(SUM(CASE WHEN was_routed = 0 AND estimated_optimal_cost IS NOT NULL THEN cost_usd - estimated_optimal_cost ELSE 0 END), 0.0)
                 FROM calls WHERE date(timestamp, 'unixepoch') >= ?1".to_string(),
                vec![Box::new(month_start)],
            )
        };
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();

        let stats = conn.query_row(&sql, param_refs.as_slice(), |row| {
            Ok(MonthlyStats {
                total_calls: row.get(0)?,
                total_prompt_tokens: row.get(1)?,
                total_completion_tokens: row.get(2)?,
                total_cost: row.get(3)?,
                avg_latency_ms: row.get(4)?,
                potential_savings: row.get(5)?,
            })
        })?;

        Ok(stats)
    }
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: i64,
    pub total_hits: i64,
}

#[derive(Debug, Clone)]
pub struct CallsSummary {
    pub total: i64,
    pub total_prompt_tokens: i64,
    pub total_completion_tokens: i64,
    pub total_cost: f64,
    #[allow(dead_code)]
    pub avg_latency_ms: f64,
    pub eliminated_count: i64,
    pub routed_count: i64,
}

#[derive(Debug, Clone)]
pub struct ModelTokenStats {
    pub model: String,
    pub call_count: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_cost: f64,
}

#[derive(Debug, Clone)]
pub struct MonthlyStats {
    pub total_calls: i64,
    #[allow(dead_code)]
    pub total_prompt_tokens: i64,
    #[allow(dead_code)]
    pub total_completion_tokens: i64,
    pub total_cost: f64,
    /// Average latency in ms (reserved for future dashboard use).
    #[allow(dead_code)]
    pub avg_latency_ms: f64,
    /// What Free-tier users could save with Pro routing (reserved).
    #[allow(dead_code)]
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
        let recent = store.recent_calls(10, None).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].model, "test-model");
    }

    #[test]
    fn test_monthly_stats_empty() {
        let store = Store::new(":memory:").expect("Failed to open in-memory store");
        let stats = store.monthly_stats(None).unwrap();
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
