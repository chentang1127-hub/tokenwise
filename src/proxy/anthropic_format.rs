//! Anthropic Messages API format ↔ OpenAI Chat Completions format translation.
//!
//! Claude Code speaks Anthropic Messages API (`/v1/messages`).
//! TokenWise translates to OpenAI format internally for classification,
//! routing, and upstream forwarding (since most providers default to
//! OpenAI-compatible endpoints).
//!
//! ## Supported paths
//! - `/v1/messages` — smart routing across all providers
//! - `/v1/{provider}/messages` — force a specific provider
//!
//! ## Format differences
//!
//! | Feature        | Anthropic                    | OpenAI                     |
//! |---------------|------------------------------|----------------------------|
//! | System prompt  | Top-level `"system"` field   | `messages[0]` role=system  |
//! | Max tokens     | `max_tokens`                 | `max_completion_tokens`    |
//! | Content        | `[{type:"text", text:"…"}]`  | `"content": "…"` (string)  |
//! | Usage keys     | `input_tokens`/`output_tokens` | `prompt_tokens`/`completion_tokens` |
//! | SSE events     | `message_start`/`content_block_delta`/… | `data: {"choices":[{"delta":…}]}` |
//! | Stop reason    | `stop_reason` top-level      | `choices[0].finish_reason` |

use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_util::stream::Stream;
use tracing::{debug, warn};

/// Convert an Anthropic Messages API request to OpenAI Chat Completions format.
pub fn anthropic_to_openai(request: &serde_json::Value) -> serde_json::Value {
    let max_tokens = request
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(4096);

    // Build messages array: system prompt (from top-level "system") + user/assistant messages
    let mut messages = Vec::new();

    // Anthropic puts system prompt as a top-level field
    if let Some(system) = request.get("system").and_then(|v| v.as_str())
        && !system.is_empty()
    {
        messages.push(serde_json::json!({
            "role": "system",
            "content": system
        }));
    }

    // Copy conversation messages
    if let Some(msgs) = request.get("messages").and_then(|v| v.as_array()) {
        for msg in msgs {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            // Anthropic content can be a string or array of content blocks
            let content = if let Some(text) = msg.get("content").and_then(|v| v.as_str()) {
                text.to_string()
            } else if let Some(blocks) = msg.get("content").and_then(|v| v.as_array()) {
                blocks
                    .iter()
                    .filter_map(|b| {
                        b.get("text")
                            .and_then(|t| t.as_str())
                            .or_else(|| b.get("type").map(|_| ""))
                    })
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                String::new()
            };
            messages.push(serde_json::json!({
                "role": role,
                "content": content
            }));
        }
    }

    let stream = request
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Preserve original model so router can find it
    let model = request
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    serde_json::json!({
        "model": model,
        "messages": messages,
        "max_completion_tokens": max_tokens,
        "stream": stream
    })
}

/// Convert messages from an Anthropic request to the internal prompt text
/// used for classification and hashing.
pub fn extract_prompt_text(request: &serde_json::Value) -> String {
    let mut parts = Vec::new();

    if let Some(system) = request.get("system").and_then(|v| v.as_str()) {
        parts.push(system.to_string());
    }

    if let Some(msgs) = request.get("messages").and_then(|v| v.as_array()) {
        for msg in msgs {
            let content = if let Some(text) = msg.get("content").and_then(|v| v.as_str()) {
                text.to_string()
            } else if let Some(blocks) = msg.get("content").and_then(|v| v.as_array()) {
                blocks
                    .iter()
                    .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                String::new()
            };
            if !content.is_empty() {
                parts.push(content);
            }
        }
    }

    parts.join(" ")
}

/// Convert an OpenAI Chat Completions response to Anthropic Messages API format.
/// Used for non-streaming responses.
pub fn openai_to_anthropic(response: &serde_json::Value, model: &str) -> serde_json::Value {
    let id = response
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("msg_tw_unknown");

    let content_text = response
        .get("choices")
        .and_then(|v| v.get(0))
        .and_then(|v| v.get("message"))
        .and_then(|v| v.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let finish_reason = response
        .get("choices")
        .and_then(|v| v.get(0))
        .and_then(|v| v.get("finish_reason"))
        .and_then(|v| v.as_str());

    let usage = response.get("usage");
    let input_tokens = usage
        .and_then(|u| u.get("prompt_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .and_then(|u| u.get("completion_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let mut result = serde_json::json!({
        "id": id,
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": [{
            "type": "text",
            "text": content_text
        }],
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens
        }
    });

    if let Some(reason) = finish_reason {
        result["stop_reason"] = serde_json::json!(reason);
    }

    result
}

/// Extract usage from an OpenAI response (for recording purposes).
pub fn extract_openai_usage(response: &serde_json::Value) -> (u32, u32) {
    let usage = response.get("usage");
    let prompt = usage
        .and_then(|u| u.get("prompt_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let completion = usage
        .and_then(|u| u.get("completion_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    (prompt, completion)
}

/// Accumulated state for translating an OpenAI SSE stream → Anthropic SSE events.
#[derive(Debug, Default)]
pub struct AnthropicSseState {
    pub message_id: String,
    pub model: String,
    pub content: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub finish_reason: Option<String>,
    pub started: bool,
    pub content_block_started: bool,
    pub stopped: bool,
    /// Set to true when the upstream stream is fully consumed.
    pub done: bool,
    /// Raw buffer for partial SSE lines
    buffer: Vec<u8>,
}

impl AnthropicSseState {
    pub fn new(message_id: String, model: String, input_tokens: u32) -> Self {
        Self {
            message_id,
            model,
            input_tokens,
            ..Default::default()
        }
    }

    /// Feed raw bytes (one or more chunks) from the upstream OpenAI SSE stream.
    /// Returns any Anthropic SSE events that should be emitted to the client.
    pub fn feed(&mut self, data: &[u8]) -> Vec<String> {
        let mut events = Vec::new();
        self.buffer.extend_from_slice(data);

        // Process complete lines
        loop {
            let newline_pos = self.buffer.iter().position(|&b| b == b'\n');
            if newline_pos.is_none() {
                break;
            }
            let idx = newline_pos.unwrap();
            let line_bytes = &self.buffer[..idx];
            let rest = if idx + 1 < self.buffer.len() {
                self.buffer[idx + 1..].to_vec()
            } else {
                Vec::new()
            };

            let line = String::from_utf8_lossy(line_bytes);
            let trimmed = line.trim();

            if !trimmed.is_empty() {
                // Parse SSE "data: {...}" line
                let payload = trimmed
                    .strip_prefix("data: ")
                    .or_else(|| trimmed.strip_prefix("data:"))
                    .unwrap_or(trimmed);

                if payload != "[DONE]" {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(payload) {
                        let new_events = self.process_openai_chunk(&json);
                        events.extend(new_events);
                    }
                } else {
                    // [DONE] — ensure we've sent message_stop
                    if !self.stopped {
                        let stop_events = self.finalize();
                        events.extend(stop_events);
                    }
                }
            }

            self.buffer = rest;
        }

        events
    }

    /// Process any remaining buffer content without requiring a trailing newline.
    /// Called when the upstream stream ends. Returns any final events.
    pub fn flush_buffer(&mut self) -> Vec<String> {
        if self.buffer.is_empty() {
            return Vec::new();
        }
        let remaining = std::mem::take(&mut self.buffer);
        let line = String::from_utf8_lossy(&remaining);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        let payload = trimmed
            .strip_prefix("data: ")
            .or_else(|| trimmed.strip_prefix("data:"))
            .unwrap_or(trimmed);
        if payload == "[DONE]" {
            return self.finalize();
        }
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(payload) {
            self.process_openai_chunk(&json)
        } else {
            Vec::new()
        }
    }

    /// Process a single OpenAI SSE JSON chunk, returning Anthropic SSE events.
    fn process_openai_chunk(&mut self, json: &serde_json::Value) -> Vec<String> {
        let mut events = Vec::new();

        // Send message_start on first chunk
        if !self.started {
            self.started = true;
            events.push(self.emit_message_start());
            events.push(self.emit_content_block_start());
            self.content_block_started = true;
        }

        // Extract content delta
        if let Some(delta_text) = json
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("delta"))
            .and_then(|d| d.get("content"))
            .and_then(|c| c.as_str())
        {
            self.content.push_str(delta_text);
            events.push(self.emit_content_block_delta(delta_text));
        }

        // Extract finish reason
        if self.finish_reason.is_none()
            && let Some(reason) = json
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("finish_reason"))
                .and_then(|r| r.as_str())
            && !reason.is_empty()
            && reason != "null"
        {
            self.finish_reason = Some(reason.to_string());
        }

        // Extract usage (usually in the final chunk)
        if let Some(usage) = json.get("usage") {
            let prompt = usage
                .get("prompt_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            let completion = usage
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            if prompt > 0 {
                self.input_tokens = prompt;
            }
            if completion > 0 {
                self.output_tokens = completion;
            }
        }

        events
    }

    fn emit_message_start(&self) -> String {
        let json = serde_json::json!({
            "type": "message_start",
            "message": {
                "id": self.message_id,
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": self.model,
                "usage": {
                    "input_tokens": self.input_tokens
                }
            }
        });
        format!(
            "event: message_start\ndata: {}\n\n",
            serde_json::to_string(&json).unwrap_or_default()
        )
    }

    fn emit_content_block_start(&self) -> String {
        let json = serde_json::json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {
                "type": "text",
                "text": ""
            }
        });
        format!(
            "event: content_block_start\ndata: {}\n\n",
            serde_json::to_string(&json).unwrap_or_default()
        )
    }

    fn emit_content_block_delta(&self, text: &str) -> String {
        let json = serde_json::json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {
                "type": "text_delta",
                "text": text
            }
        });
        format!(
            "event: content_block_delta\ndata: {}\n\n",
            serde_json::to_string(&json).unwrap_or_default()
        )
    }

    /// Called when the stream completes. Emits message_delta + message_stop.
    pub fn finalize(&mut self) -> Vec<String> {
        if self.stopped {
            return Vec::new();
        }
        self.stopped = true;

        let mut events = Vec::new();

        // message_delta with stop_reason + usage
        let mut delta_json = serde_json::json!({
            "type": "message_delta",
            "delta": {}
        });
        if let Some(ref reason) = self.finish_reason {
            delta_json["delta"]["stop_reason"] = serde_json::json!(reason);
        } else {
            delta_json["delta"]["stop_reason"] = serde_json::json!("end_turn");
        }
        if self.output_tokens > 0 {
            delta_json["usage"] = serde_json::json!({
                "output_tokens": self.output_tokens
            });
        }
        events.push(format!(
            "event: message_delta\ndata: {}\n\n",
            serde_json::to_string(&delta_json).unwrap_or_default()
        ));

        // message_stop
        let stop_json = serde_json::json!({"type": "message_stop"});
        events.push(format!(
            "event: message_stop\ndata: {}\n\n",
            serde_json::to_string(&stop_json).unwrap_or_default()
        ));

        events
    }
}

/// Stream adapter that translates OpenAI SSE bytes → Anthropic SSE bytes.
///
/// Wraps the upstream `reqwest` byte stream, parses each SSE chunk,
/// translates to Anthropic format, and emits Anthropic SSE events.
pub struct AnthropicSseStream<S> {
    inner: S,
    state: Arc<Mutex<AnthropicSseState>>,
    /// Buffered outgoing events (Anthropic SSE text, already formatted).
    output_buf: Vec<u8>,
    /// Set to true when upstream is exhausted and all events flushed.
    upstream_done: bool,
    /// Track total bytes to help debug truncated streams.
    total_bytes: usize,
}

impl<S> AnthropicSseStream<S> {
    /// Create a new stream adapter. Returns the stream and a shared handle
    /// to the state so the caller (recording task) can read final token counts
    /// after the stream is fully consumed.
    pub fn new(inner: S, state: AnthropicSseState) -> (Self, Arc<Mutex<AnthropicSseState>>) {
        let shared = Arc::new(Mutex::new(state));
        let stream = Self {
            inner,
            state: shared.clone(),
            output_buf: Vec::new(),
            upstream_done: false,
            total_bytes: 0,
        };
        (stream, shared)
    }
}

impl<S> Stream for AnthropicSseStream<S>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
{
    type Item = Result<Bytes, String>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        // 1. Flush any buffered outgoing events first
        if !this.output_buf.is_empty() {
            let data = std::mem::take(&mut this.output_buf);
            return Poll::Ready(Some(Ok(Bytes::from(data))));
        }

        // 2. If upstream is done, check if we've sent final events
        if this.upstream_done {
            // Already finalized — stream is complete
            this.state.lock().unwrap().done = true;
            return Poll::Ready(None);
        }

        // 3. Poll the inner stream for more data
        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                this.total_bytes += chunk.len();

                // Feed the chunk to our SSE translator
                let events = this.state.lock().unwrap().feed(&chunk);

                if events.is_empty() {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }

                // Serialize events to output buffer
                let mut out = String::new();
                for event in &events {
                    out.push_str(event);
                }
                let out_bytes = out.into_bytes();
                Poll::Ready(Some(Ok(Bytes::from(out_bytes))))
            }
            Poll::Ready(Some(Err(e))) => {
                warn!("Upstream stream error in Anthropic adapter: {e}");
                // Try to finalize gracefully
                let final_events = this.state.lock().unwrap().finalize();
                let mut out = String::new();
                for event in &final_events {
                    out.push_str(event);
                }
                if !out.is_empty() {
                    this.output_buf = out.into_bytes();
                }
                this.upstream_done = true;
                this.state.lock().unwrap().done = true;
                if !this.output_buf.is_empty() {
                    let data = std::mem::take(&mut this.output_buf);
                    return Poll::Ready(Some(Ok(Bytes::from(data))));
                }
                Poll::Ready(Some(Err(e.to_string())))
            }
            Poll::Ready(None) => {
                // Upstream exhausted — flush buffer then send final events
                debug!(
                    "Upstream stream done, total {} bytes, finalizing Anthropic SSE",
                    this.total_bytes
                );
                let mut state = this.state.lock().unwrap();
                let flush_events = state.flush_buffer();
                let final_events = state.finalize();
                drop(state);
                let all_events: Vec<String> =
                    flush_events.into_iter().chain(final_events).collect();
                if all_events.is_empty() {
                    this.state.lock().unwrap().done = true;
                    return Poll::Ready(None);
                }
                let mut out = String::new();
                for event in &all_events {
                    out.push_str(event);
                }
                let out_bytes = out.into_bytes();
                if out_bytes.is_empty() {
                    this.state.lock().unwrap().done = true;
                    return Poll::Ready(None);
                }
                this.upstream_done = true;
                this.state.lock().unwrap().done = true;
                Poll::Ready(Some(Ok(Bytes::from(out_bytes))))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anthropic_to_openai_basic() {
        let req = serde_json::json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "Hello, how are you?"}
            ],
            "stream": false
        });

        let openai = anthropic_to_openai(&req);
        assert_eq!(openai["model"], "claude-sonnet-4-6");
        assert_eq!(openai["max_completion_tokens"], 1024);
        assert_eq!(openai["stream"], false);
        let msgs = openai["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "Hello, how are you?");
    }

    #[test]
    fn test_anthropic_to_openai_with_system() {
        let req = serde_json::json!({
            "model": "deepseek-chat",
            "max_tokens": 500,
            "system": "You are a helpful assistant.",
            "messages": [
                {"role": "user", "content": "Hi"}
            ],
            "stream": true
        });

        let openai = anthropic_to_openai(&req);
        let msgs = openai["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "You are a helpful assistant.");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[1]["content"], "Hi");
        assert_eq!(openai["stream"], true);
    }

    #[test]
    fn test_anthropic_to_openai_content_array() {
        let req = serde_json::json!({
            "model": "deepseek-chat",
            "max_tokens": 100,
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "Hello world"},
                    {"type": "text", "text": "How are you?"}
                ]}
            ]
        });

        let openai = anthropic_to_openai(&req);
        let msgs = openai["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["content"], "Hello world\nHow are you?");
    }

    #[test]
    fn test_openai_to_anthropic_response() {
        let openai_resp = serde_json::json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "model": "deepseek-chat",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "I'm doing well, thanks!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 15,
                "completion_tokens": 8,
                "total_tokens": 23
            }
        });

        let anthropic = openai_to_anthropic(&openai_resp, "deepseek-chat");
        assert_eq!(anthropic["type"], "message");
        assert_eq!(anthropic["role"], "assistant");
        assert_eq!(anthropic["model"], "deepseek-chat");
        assert_eq!(anthropic["content"][0]["text"], "I'm doing well, thanks!");
        assert_eq!(anthropic["usage"]["input_tokens"], 15);
        assert_eq!(anthropic["usage"]["output_tokens"], 8);
        assert_eq!(anthropic["stop_reason"], "stop");
    }

    #[test]
    fn test_anthropic_sse_state_basic() {
        let mut state =
            AnthropicSseState::new("msg_001".to_string(), "deepseek-chat".to_string(), 10);

        // First chunk — should emit message_start + content_block_start
        // SSE lines must end with \n to be processed
        let chunk1 = "data: {\"id\":\"chatcmpl-1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"}}]}\n";
        let events = state.feed(chunk1.as_bytes());
        assert!(!events.is_empty(), "First chunk should produce events");
        // Should have message_start and content_block_start and content_block_delta
        let combined = events.join("\n");
        assert!(combined.contains("message_start"));
        assert!(combined.contains("content_block_start"));
        assert!(combined.contains("content_block_delta"));
        assert!(combined.contains("Hello"));

        // Second chunk
        let chunk2 = "data: {\"id\":\"chatcmpl-1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\"}}]}\n";
        let events2 = state.feed(chunk2.as_bytes());
        assert!(events2.iter().any(|e| e.contains("world")));

        // Final chunk with usage
        let chunk3 = "data: {\"id\":\"chatcmpl-1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n";
        let _events3 = state.feed(chunk3.as_bytes());

        // Finalize
        let final_events = state.finalize();
        let final_str = final_events.join("\n");
        assert!(final_str.contains("message_delta"));
        assert!(final_str.contains("message_stop"));
        assert_eq!(state.output_tokens, 5);
        assert_eq!(state.content, "Hello world");
    }

    #[test]
    fn test_anthropic_sse_state_no_double_stop() {
        let mut state = AnthropicSseState::new("msg_002".to_string(), "test".to_string(), 0);
        let events1 = state.finalize();
        assert_eq!(events1.len(), 2); // message_delta + message_stop
        let events2 = state.finalize();
        assert!(events2.is_empty()); // second call is no-op
    }
}
