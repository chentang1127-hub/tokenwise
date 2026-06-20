//! Data models for recorded API calls.

use sha2::{Digest, Sha256};

/// A single recorded API call.
#[derive(Debug, Clone)]
pub struct CallRecord {
    pub id: String,
    pub timestamp: i64,
    pub model: String,
    pub provider: String,
    pub complexity: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub cost_usd: f64,
    pub latency_ms: u64,
    pub fallback_used: bool,
    #[allow(dead_code)]
    pub prompt_hash: String,
    pub finish_reason: Option<String>,
    /// Whether smart routing was actually applied (Pro only).
    pub was_routed: bool,
    /// The model TokenWise would have routed to (shown in Free tier as savings tip).
    pub recommended_model: Option<String>,
    /// What the cost would have been with smart routing.
    pub estimated_optimal_cost: Option<f64>,
}

impl CallRecord {
    /// Create a new call record with defaults (filled in later).
    pub fn from_request(
        model: &str,
        provider: &str,
        complexity: &str,
        fallback_used: bool,
        latency_ms: u64,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            model: model.to_string(),
            provider: provider.to_string(),
            complexity: complexity.to_string(),
            prompt_tokens: 0,
            completion_tokens: 0,
            cost_usd: 0.0,
            latency_ms,
            fallback_used,
            prompt_hash: String::new(),
            finish_reason: None,
            was_routed: false,
            recommended_model: None,
            estimated_optimal_cost: None,
        }
    }

    /// Compute a truncated SHA256 hash of the prompt for dedup analysis.
    pub fn hash_prompt(prompt: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(prompt.as_bytes());
        hex::encode(&hasher.finalize()[..8])
    }

    /// Set token counts from parsed usage.
    pub fn with_usage(mut self, prompt: u32, completion: u32) -> Self {
        self.prompt_tokens = prompt;
        self.completion_tokens = completion;
        self
    }

    /// Set the prompt hash.
    #[allow(dead_code)]
    pub fn with_prompt(self, prompt: &str) -> Self {
        let mut this = self;
        this.prompt_hash = Self::hash_prompt(prompt);
        this
    }
}
