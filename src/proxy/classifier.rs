//! Complexity classifier — determines whether a prompt needs a cheap, mid, or premium model.
//!
//! MVP: Rule-based heuristic (~1ms, zero model dependencies).
//! V2: ONNX sentence-embedding model embedded in binary.

use crate::config::RoutingConfig;

/// Classification result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Complexity {
    Simple,
    Medium,
    Complex,
}

impl Complexity {
    pub fn tier_name(&self) -> &'static str {
        match self {
            Complexity::Simple => "simple",
            Complexity::Medium => "medium",
            Complexity::Complex => "complex",
        }
    }
}

/// Classify a prompt based on heuristic rules.
///
/// Rules (in priority order):
/// 1. Token count thresholds (if available)
/// 2. Complex keywords/phrases → Complex
/// 3. Simple keywords → Simple
/// 4. Default → Medium
pub fn classify(
    messages: &[serde_json::Value],
    token_count: Option<usize>,
    config: &RoutingConfig,
) -> Complexity {
    // Extract all text from messages
    let text = extract_text(messages);
    let text_lower = text.to_lowercase();

    // Token-count thresholds (if token count available)
    if let Some(tokens) = token_count {
        if tokens < config.simple_max_tokens {
            return Complexity::Simple;
        }
        if tokens > config.complex_min_tokens {
            return Complexity::Complex;
        }
    }

    // Keyword-based detection: check complex patterns first
    for kw in &config.complex_keywords {
        if text_lower.contains(&kw.to_lowercase()) {
            return Complexity::Complex;
        }
    }

    // Check simple keywords
    for kw in &config.simple_keywords {
        if text_lower.contains(&kw.to_lowercase()) {
            return Complexity::Simple;
        }
    }

    // Heuristic: if text is very short (< 80 chars), probably simple
    if text.chars().count() < 80 {
        return Complexity::Simple;
    }

    // Heuristic: if text is long (> 2000 chars), probably complex
    if text.chars().count() > 2000 {
        return Complexity::Complex;
    }

    Complexity::Medium
}

/// Extract concatenated text from chat completion messages.
fn extract_text(messages: &[serde_json::Value]) -> String {
    let mut text = String::with_capacity(1024);
    for msg in messages {
        if let Some(content) = msg.get("content") {
            match content {
                serde_json::Value::String(s) => {
                    text.push_str(s);
                    text.push(' ');
                }
                serde_json::Value::Array(parts) => {
                    // Handle multimodal content arrays
                    for part in parts {
                        if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                            text.push_str(t);
                            text.push(' ');
                        }
                    }
                }
                _ => {}
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> RoutingConfig {
        RoutingConfig {
            simple_max_tokens: 300,
            complex_min_tokens: 1500,
            simple_keywords: vec![
                "summarize".into(), "translate".into(), "extract".into(),
                "classify".into(), "what is".into(), "define".into(),
            ],
            complex_keywords: vec![
                "step by step".into(), "debug".into(), "implement".into(),
                "write code".into(), "refactor".into(),
            ],
            tier_simple: "cheap".into(),
            tier_complex: "premium".into(),
            tier_default: "mid".into(),
        }
    }

    fn msg(content: &str) -> serde_json::Value {
        serde_json::json!({"role": "user", "content": content})
    }

    #[test]
    fn test_simple_classification() {
        let cfg = test_config();
        let msgs = vec![msg("What is the capital of France?")];
        assert_eq!(classify(&msgs, None, &cfg), Complexity::Simple);
    }

    #[test]
    fn test_complex_classification() {
        let cfg = test_config();
        let msgs = vec![msg("Please debug this Rust code step by step and explain the memory issue")];
        assert_eq!(classify(&msgs, None, &cfg), Complexity::Complex);
    }

    #[test]
    fn test_medium_default() {
        let cfg = test_config();
        let msgs = vec![msg("Tell me about the history of machine learning and its applications in modern software engineering")];
        assert_eq!(classify(&msgs, None, &cfg), Complexity::Medium);
    }

    #[test]
    fn test_token_count_thresholds() {
        let cfg = test_config();
        // Short prompt but > simple_max_tokens
        let msgs = vec![msg("Hello")];
        // Token count estimation says 1 token, which is < 300 → simple
        assert_eq!(classify(&msgs, Some(1), &cfg), Complexity::Simple);
        // High token count
        assert_eq!(classify(&msgs, Some(2000), &cfg), Complexity::Complex);
    }
}
