//! Streaming tee — forward SSE chunks immediately while extracting
//! usage and content delta on a non-blocking cold path.
//!
//! Hot path (every chunk): forward bytes to client immediately.
//! Cold path (spawned): parse JSON, extract usage + delta for recording.
//!
//! Lifecycle:
//!   TeeStream is polled by Hyper → copies chunks to mpsc → returns chunk
//!   When TeeStream ends, tx drops → analyzer loop exits → JoinHandle resolves
//!   Recording task awaits JoinHandle → records with real token counts

use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_util::stream::Stream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Data extracted from a streaming response.
#[derive(Debug, Default, Clone)]
pub struct StreamMetrics {
    pub content_preview: String,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub finish_reason: Option<String>,
    pub total_chunks: usize,
}

/// Wraps an upstream SSE stream, forwarding every chunk while
/// sending a copy through an mpsc channel for offline analysis.
pub struct TeeStream<S> {
    inner: S,
    tx: mpsc::UnboundedSender<Bytes>,
}

impl<S> TeeStream<S> {
    pub fn new(inner: S, tx: mpsc::UnboundedSender<Bytes>) -> Self {
        Self { inner, tx }
    }
}

impl<S> Stream for TeeStream<S>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
{
    type Item = Result<Bytes, String>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                // Hot path: send a copy to the analyzer
                let _ = this.tx.send(chunk.clone());
                // Return the chunk immediately — never block the hot path
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(e))) => {
                tracing::warn!("Upstream stream error: {e}");
                Poll::Ready(Some(Err(e.to_string())))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Spawns the stream analyzer on a separate tokio task.
///
/// Returns a sender (for TeeStream to push chunks into) and a JoinHandle
/// that resolves with the final StreamMetrics when the stream completes
/// and all chunks have been processed.
pub fn spawn_analyzer() -> (mpsc::UnboundedSender<Bytes>, JoinHandle<StreamMetrics>) {
    let (tx, mut rx) = mpsc::unbounded_channel::<Bytes>();

    let handle = tokio::spawn(async move {
        let mut local = StreamMetrics::default();

        while let Some(chunk) = rx.recv().await {
            let chunk_str = String::from_utf8_lossy(&chunk);

            for line in chunk_str.lines() {
                // SSE format: "data: {...}" or "data: [DONE]"
                let payload = line.strip_prefix("data: ").unwrap_or(line);
                if payload.is_empty() || payload == "[DONE]" {
                    continue;
                }

                local.total_chunks += 1;

                // Try to parse as JSON and extract fields
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(payload) {
                    // Extract content delta
                    if let Some(delta) = json
                        .get("choices")
                        .and_then(|c| c.get(0))
                        .and_then(|c| c.get("delta"))
                        .and_then(|d| d.get("content"))
                        .and_then(|c| c.as_str())
                    {
                        if local.content_preview.len() < 200 {
                            local.content_preview.push_str(delta);
                        }
                    }

                    // Extract usage (usually in last chunk)
                    if let Some(usage) = json.get("usage") {
                        local.prompt_tokens =
                            usage.get("prompt_tokens").and_then(|v| v.as_u64()).map(|v| v as u32);
                        local.completion_tokens =
                            usage.get("completion_tokens").and_then(|v| v.as_u64()).map(|v| v as u32);
                    }

                    // Extract finish reason
                    if let Some(reason) = json
                        .get("choices")
                        .and_then(|c| c.get(0))
                        .and_then(|c| c.get("finish_reason"))
                        .and_then(|r| r.as_str())
                    {
                        local.finish_reason = Some(reason.to_string());
                    }
                }
            }
        }

        local
    });

    (tx, handle)
}
