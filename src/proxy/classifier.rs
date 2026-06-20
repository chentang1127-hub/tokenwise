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
/// 1. Token count thresholds (if available, or estimated from char count)
/// 2. Code patterns in prompt → Complex
/// 3. Complex keywords/phrases → Complex
/// 4. Question detection (short + "?" + what/why/how) → Simple
/// 5. Simple keywords → Simple
/// 6. Length heuristics
/// 7. Default → Medium
pub fn classify(
    messages: &[serde_json::Value],
    token_count: Option<usize>,
    config: &RoutingConfig,
) -> Complexity {
    // Extract all text from messages
    let text = extract_text(messages);
    let text_lower = text.to_lowercase();
    let char_count = text.chars().count();

    // ── Priority 1: Strong signals (code, multi-step) ───
    if has_code_patterns(&text_lower) {
        return Complexity::Complex;
    }
    if has_multi_step(&text_lower) {
        return Complexity::Complex;
    }

    // ── Priority 2: Keyword detection ──────────────────
    for kw in &config.complex_keywords {
        if text_lower.contains(&kw.to_lowercase()) {
            return Complexity::Complex;
        }
    }
    for kw in &config.simple_keywords {
        if text_lower.contains(&kw.to_lowercase()) {
            return Complexity::Simple;
        }
    }

    // ── Priority 3: Question heuristics ────────────────
    if is_simple_question(&text, char_count) {
        return Complexity::Simple;
    }

    // ── Priority 4: Token count thresholds ─────────────
    // Only when actual token count is available (from upstream API estimate)
    if let Some(tokens) = token_count {
        if tokens < config.simple_max_tokens {
            return Complexity::Simple;
        }
        if tokens > config.complex_min_tokens {
            return Complexity::Complex;
        }
    }

    // ── Priority 5: Length heuristics ──────────────────
    if char_count < 80 {
        return Complexity::Simple;
    }
    if char_count > 2000 {
        return Complexity::Complex;
    }

    Complexity::Medium
}

/// Detect code-related patterns in the prompt.
fn has_code_patterns(text: &str) -> bool {
    let code_markers = [
        "```",
        "fn ",
        "def ",
        "function ",
        "class ",
        "import ",
        "pub fn",
        "let mut",
        "const ",
        "var ",
        "func ",
        "package ",
        "#include",
        "console.log",
        "print(",
        "struct ",
        "impl ",
        "trait ",
        "enum ",
    ];
    let mut matches = 0;
    for marker in &code_markers {
        if text.contains(marker) {
            matches += 1;
            if matches >= 2 {
                return true;
            }
        }
    }
    false
}

/// Detect multi-step instructions.
fn has_multi_step(text: &str) -> bool {
    let step_markers = [
        "step 1",
        "step 2",
        "first",
        "second",
        "third",
        "firstly",
        "secondly",
        "finally",
        "then",
        "1.",
        "2.",
        "3.",
        "1)",
        "2)",
        "3)",
        "第一步",
        "第二步",
        "首先",
        "然后",
        "最后",
        "你需要",
        "请按照",
        "请根据以下步骤",
    ];
    let mut matches = 0;
    for marker in &step_markers {
        if text.contains(marker) {
            matches += 1;
            if matches >= 2 {
                return true;
            }
        }
    }
    false
}

/// Check if prompt is a simple factual question.
fn is_simple_question(text: &str, char_count: usize) -> bool {
    if char_count > 200 {
        return false;
    }
    let text = text.trim();
    // Must end with question mark or be very short
    if !text.ends_with('?') && !text.ends_with('？') && char_count > 50 {
        return false;
    }
    // Check for simple question patterns
    let lowered = text.to_lowercase();
    let question_starters = [
        "what is",
        "what are",
        "who is",
        "where is",
        "when did",
        "how many",
        "how much",
        "which",
        "define",
        "什么是",
        "谁",
        "哪里",
        "什么时候",
        "怎么读",
        "翻译",
        "capital of",
        "translate",
        "say hello",
    ];
    for starter in &question_starters {
        if lowered.contains(starter) {
            return true;
        }
    }
    false
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
                "summarize".into(),
                "translate".into(),
                "extract".into(),
                "classify".into(),
                "what is".into(),
                "define".into(),
            ],
            complex_keywords: vec![
                "step by step".into(),
                "debug".into(),
                "implement".into(),
                "write code".into(),
                "refactor".into(),
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
        let msgs = vec![msg(
            "Please debug this Rust code step by step and explain the memory issue",
        )];
        assert_eq!(classify(&msgs, None, &cfg), Complexity::Complex);
    }

    #[test]
    fn test_medium_default() {
        let cfg = test_config();
        let msgs = vec![msg(
            "Tell me about the history of machine learning and its applications in modern software engineering",
        )];
        assert_eq!(classify(&msgs, None, &cfg), Complexity::Medium);
    }

    #[test]
    fn test_token_count_thresholds() {
        let cfg = test_config();
        let msgs = vec![msg("Hello")];
        assert_eq!(classify(&msgs, Some(1), &cfg), Complexity::Simple);
        assert_eq!(classify(&msgs, Some(2000), &cfg), Complexity::Complex);
    }

    #[test]
    fn test_code_detection_is_complex() {
        let cfg = test_config();
        let msgs = vec![msg("Write a function in Rust: fn foo() { let x = 1; }")];
        assert_eq!(classify(&msgs, None, &cfg), Complexity::Complex);
    }

    #[test]
    fn test_multi_step_is_complex() {
        let cfg = test_config();
        let msgs = vec![msg(
            "First, install Python. Second, create a virtual environment. Then install dependencies.",
        )];
        assert_eq!(classify(&msgs, None, &cfg), Complexity::Complex);
    }

    #[test]
    fn test_simple_question_detection() {
        let cfg = test_config();
        assert_eq!(
            classify(&vec![msg("Who is the president of France?")], None, &cfg),
            Complexity::Simple
        );
        assert_eq!(
            classify(&vec![msg("Where is Tokyo?")], None, &cfg),
            Complexity::Simple
        );
    }

    #[test]
    fn test_chinese_question_detection() {
        let cfg = test_config();
        let msgs = vec![msg("翻译：Hello World")];
        assert_eq!(classify(&msgs, None, &cfg), Complexity::Simple);
    }
}
