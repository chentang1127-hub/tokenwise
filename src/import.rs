//! History import — parse Claude Code JSONL transcript files and
//! import API call records into the TokenWise SQLite store.
//!
//! ## Usage
//! ```bash
//! tokenwise import --source ~/.claude/projects/
//! ```
//!
//! ## What it imports
//! - Each unique assistant message with usage data → one CallRecord
//! - Token counts from Anthropic-style `usage.input_tokens`/`output_tokens`
//! - Model name, timestamp, and session ID

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use tracing::{info, warn};

use crate::config::Config;
use crate::recording::Store;

/// Result of an import operation.
#[derive(Debug, Default)]
pub struct ImportResult {
    /// Total JSONL files scanned.
    pub files_scanned: usize,
    /// Total JSON lines parsed.
    pub lines_parsed: usize,
    /// Unique assistant messages found with usage data.
    pub messages_found: usize,
    /// Records actually inserted (skipping duplicates by hash).
    pub records_inserted: usize,
    /// Total cost of imported calls.
    pub total_cost: f64,
}

/// Find all JSONL files under a directory (recursive).
fn find_jsonl_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(find_jsonl_files(&path));
            } else if path.extension().map(|e| e == "jsonl").unwrap_or(false) {
                files.push(path);
            }
        }
    }
    files
}

/// Parse an ISO 8601 timestamp string to a Unix timestamp (seconds).
fn parse_timestamp(ts: &str) -> Option<i64> {
    // Format: "2026-06-13T01:03:55.196Z"
    // chrono can parse this directly
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        return Some(dt.timestamp());
    }
    // Try without timezone (UTC assumed)
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%.fZ") {
        return Some(dt.and_utc().timestamp());
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%SZ") {
        return Some(dt.and_utc().timestamp());
    }
    None
}

/// Import API calls from Claude Code JSONL transcript files.
pub fn import_from_directory(
    source_dir: &Path,
    store: &Store,
    cfg: &Config,
) -> Result<ImportResult, Box<dyn std::error::Error>> {
    let mut result = ImportResult::default();
    let files = find_jsonl_files(source_dir);

    if files.is_empty() {
        info!("No JSONL files found in {}", source_dir.display());
        return Ok(result);
    }

    result.files_scanned = files.len();
    info!("Scanning {} JSONL files...", files.len());

    // Track seen message IDs to deduplicate
    let mut seen_message_ids: HashSet<String> = HashSet::new();

    for file_path in &files {
        let content = match fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(e) => {
                warn!("Skipping {}: {e}", file_path.display());
                continue;
            }
        };

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            result.lines_parsed += 1;

            // Parse JSON line
            let json: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            // Only process assistant messages with usage data
            if json.get("type").and_then(|v| v.as_str()) != Some("assistant") {
                continue;
            }

            // Usage is nested inside the "message" object
            let message = match json.get("message") {
                Some(m) => m,
                None => continue,
            };

            let usage = match message.get("usage") {
                Some(u) => u,
                None => continue,
            };

            // Deduplicate by message ID
            let msg_id = message.get("id").and_then(|v| v.as_str()).unwrap_or("");

            if msg_id.is_empty() || !seen_message_ids.insert(msg_id.to_string()) {
                continue; // Already seen this message
            }

            // Extract data
            let model = message
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("deepseek-v4-pro")
                .to_string();

            let timestamp_str = json.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
            let timestamp =
                parse_timestamp(timestamp_str).unwrap_or_else(|| chrono::Utc::now().timestamp());

            let input_tokens = usage
                .get("input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;

            let output_tokens = usage
                .get("output_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;

            // Determine provider from model name
            let (provider_name, lookup_model) = infer_provider_from_model(&model);

            // Compute cost from model pricing in config
            let cost_usd = if let Some(model_cfg) = cfg.model_config(provider_name, lookup_model) {
                crate::cost::calculator::compute_cost(input_tokens, output_tokens, model_cfg)
            } else {
                // Fallback: use default DeepSeek pricing
                const DEFAULT_PROMPT_RATE: f64 = 0.00027;
                const DEFAULT_COMPLETION_RATE: f64 = 0.0011;
                (input_tokens as f64 / 1000.0) * DEFAULT_PROMPT_RATE
                    + (output_tokens as f64 / 1000.0) * DEFAULT_COMPLETION_RATE
            };

            result.total_cost += cost_usd;

            // Build a CallRecord manually with the historical timestamp
            let rec = crate::recording::CallRecord {
                id: uuid::Uuid::new_v4().to_string(),
                timestamp,
                model: model.clone(),
                provider: provider_name.to_string(),
                complexity: "historical".to_string(),
                prompt_tokens: input_tokens,
                completion_tokens: output_tokens,
                cost_usd,
                latency_ms: 0,
                fallback_used: false,
                prompt_hash: String::new(),
                finish_reason: None,
                was_routed: false,
                recommended_model: None,
                estimated_optimal_cost: None,
                tenant_id: "anon".to_string(),
            };

            // Build a minimal request JSON for recording
            let request_json = serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": "[imported from Claude Code history]"}]
            });

            if let Err(e) = store.record_call(&rec, &request_json) {
                warn!("Failed to record imported call {}: {e}", msg_id);
                continue;
            }

            result.messages_found += 1;
            result.records_inserted += 1;
        }
    }

    info!(
        "Import complete: {} calls ({:.4} USD total) from {} files",
        result.records_inserted, result.total_cost, result.files_scanned
    );

    Ok(result)
}

/// Infer provider from model name.
/// Returns (provider_name, model_id_to_lookup_in_config).
fn infer_provider_from_model(model: &str) -> (&str, &str) {
    let model_lower = model.to_lowercase();

    if model_lower.contains("deepseek") {
        ("deepseek", "deepseek-chat")
    } else if model_lower.contains("claude") || model_lower.contains("anthropic") {
        ("anthropic", "claude-sonnet-4-6")
    } else if model_lower.contains("gpt") || model_lower.contains("openai") {
        ("openai", "gpt-4.1-mini")
    } else if model_lower.contains("gemini") || model_lower.contains("google") {
        ("google", "gemini-2.5-flash")
    } else if model_lower.contains("mistral") || model_lower.contains("codestral") {
        ("mistral", "mistral-small")
    } else if model_lower.contains("grok") || model_lower.contains("xai") {
        ("xai", "grok-4-mini")
    } else if model_lower.contains("llama") || model_lower.contains("groq") {
        ("groq", "llama-4-scout")
    } else {
        // Default to deepseek (most common for Chinese users)
        ("deepseek", "deepseek-chat")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_timestamp() {
        let ts = parse_timestamp("2026-06-13T01:03:55.196Z");
        assert!(ts.is_some());
        // Verify it's a reasonable value
        let val = ts.unwrap();
        assert!(val > 1_700_000_000); // After 2023
        assert!(val < 1_800_000_000); // Before 2027
    }

    #[test]
    fn test_infer_provider() {
        assert_eq!(infer_provider_from_model("deepseek-v4-pro").0, "deepseek");
        assert_eq!(
            infer_provider_from_model("claude-sonnet-4-6").0,
            "anthropic"
        );
        assert_eq!(infer_provider_from_model("gpt-4.1").0, "openai");
        assert_eq!(infer_provider_from_model("gemini-2.5-flash").0, "google");
        assert_eq!(infer_provider_from_model("unknown-model").0, "deepseek");
    }
}
