//! gRPC proxy mode — intercept and route gRPC AI service calls.
//!
//! In addition to the OpenAI-compatible REST API on port 9401,
//! TokenWise Core can proxy gRPC requests to AI providers that
//! expose gRPC endpoints (e.g., Google Cloud AI, custom model
//! servers with gRPC).
//!
//! ## Architecture
//!
//! ```text
//! gRPC Client --> TokenWise (:9402) --> gRPC Router --> Upstream gRPC
//!                     |
//!                     +-- Reuses same classifier, cost calculator,
//!                         and recording store as the REST proxy.
//! ```
//!
//! ## Implementation Plan
//!
//! 1. Add `tonic` and `prost` dependencies to Cargo.toml
//! 2. Define a generic `AIService` proto that mirrors common AI
//!    gRPC interfaces (Chat, Embed, Classify)
//! 3. Implement a `GrpcProxyService` that:
//!    - Accepts gRPC requests on port 9402
//!    - Classifies complexity from message content
//!    - Routes to cheapest capable upstream model
//!    - Records calls with the same SQLite store
//!    - Streams responses with tee for token counting
//! 4. Add gRPC health checking via `grpc.health.v1.Health`
//!
//! ## Current State
//!
//! This module provides the request/response types and routing
//! logic. The gRPC server binary is compiled as a separate entry
//! point when the `grpc` feature flag is enabled.
//!
//! ## Enable gRPC mode
//!
//! ```bash
//! cargo build --release --features grpc
//! # Proxy listens on port 9402 for gRPC in addition to 9401 for REST
//! ```

/// Configuration for gRPC proxy mode.
#[derive(Debug, Clone)]
pub struct GrpcConfig {
    /// Address to listen for gRPC connections.
    pub listen: String,
    /// Whether gRPC proxy is enabled.
    pub enabled: bool,
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self {
            listen: "0.0.0.0:9402".to_string(),
            enabled: false,
        }
    }
}

/// A generic gRPC AI chat request (proto-compatible).
#[derive(Debug, Clone)]
pub struct GrpcChatRequest {
    /// One or more messages in the conversation.
    pub messages: Vec<GrpcMessage>,
    /// Requested model ID (may be overridden by router).
    pub model: String,
    /// Maximum tokens to generate.
    pub max_tokens: Option<u32>,
    /// Temperature for sampling.
    pub temperature: Option<f32>,
    /// Whether to stream the response.
    pub stream: bool,
}

/// A single message in a gRPC chat request.
#[derive(Debug, Clone)]
pub struct GrpcMessage {
    /// "system", "user", or "assistant"
    pub role: String,
    /// Message content text.
    pub content: String,
}

/// A generic gRPC AI chat response (proto-compatible).
#[derive(Debug, Clone)]
pub struct GrpcChatResponse {
    /// The generated message content.
    pub content: String,
    /// Token usage statistics.
    pub usage: Option<GrpcUsage>,
    /// Reason the generation stopped.
    pub finish_reason: Option<String>,
}

/// Token usage for a gRPC request.
#[derive(Debug, Clone)]
pub struct GrpcUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Convert a gRPC chat request to the internal message format used
/// by the complexity classifier.
impl GrpcChatRequest {
    /// Extract all message content as a single string for classification.
    pub fn content_for_classification(&self) -> String {
        self.messages
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Convert messages to serde_json::Value for recording.
    pub fn to_json_messages(&self) -> serde_json::Value {
        let msgs: Vec<serde_json::Value> = self
            .messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content,
                })
            })
            .collect();
        serde_json::json!({ "messages": msgs })
    }
}
