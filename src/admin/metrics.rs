//! Prometheus-compatible metrics endpoint.
//!
//! Exposes counters and gauges about request volume, token usage,
//! cache performance, and cost. Format is Prometheus text exposition.

use std::sync::atomic::{AtomicU64, Ordering};

/// Thread-safe metrics counters.
/// Wrapped in Arc so both admin and proxy can increment them.
#[derive(Default)]
pub struct Metrics {
    /// Total chat completion requests received by the proxy.
    pub requests_total: AtomicU64,
    /// Requests served from cache.
    pub cache_hits_total: AtomicU64,
    /// Requests that were smart-routed.
    pub routed_total: AtomicU64,
    /// Requests that failed / fell back.
    pub fallbacks_total: AtomicU64,
    /// Total prompt tokens across all calls.
    pub prompt_tokens_total: AtomicU64,
    /// Total completion tokens across all calls.
    pub completion_tokens_total: AtomicU64,
    /// Cumulative cost in micro-dollars (USD × 1_000_000) for integer precision.
    pub cost_micros_total: AtomicU64,
    /// Streaming responses that returned zero content (empty body).
    pub empty_streams_total: AtomicU64,
    /// Streaming responses that were truncated (finish_reason = length/max_tokens).
    pub truncated_streams_total: AtomicU64,
}

impl Metrics {
    /// Increment request counter.
    pub fn inc_requests(&self) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment cache hit counter.
    pub fn inc_cache_hits(&self) {
        self.cache_hits_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment routed counter.
    pub fn inc_routed(&self) {
        self.routed_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment fallback counter.
    pub fn inc_fallbacks(&self) {
        self.fallbacks_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Add token counts.
    pub fn add_tokens(&self, prompt: u64, completion: u64) {
        self.prompt_tokens_total.fetch_add(prompt, Ordering::Relaxed);
        self.completion_tokens_total
            .fetch_add(completion, Ordering::Relaxed);
    }

    /// Add cost in micro-dollars.
    pub fn add_cost(&self, usd: f64) {
        let micros = (usd * 1_000_000.0) as u64;
        self.cost_micros_total.fetch_add(micros, Ordering::Relaxed);
    }

    /// Increment empty stream counter (streaming safety net).
    pub fn inc_empty_streams(&self) {
        self.empty_streams_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment truncated stream counter (streaming safety net).
    pub fn inc_truncated_streams(&self) {
        self.truncated_streams_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Render Prometheus text format.
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(1024);

        macro_rules! metric {
            ($name:expr, $type:expr, $help:expr, $val:expr) => {
                out.push_str(&format!(
                    "# HELP {} {}\n# TYPE {} {}\n{} {}\n",
                    $name, $help, $name, $type, $name, $val
                ));
            };
        }

        metric!(
            "tokenwise_requests_total",
            "counter",
            "Total number of chat completion requests processed.",
            self.requests_total.load(Ordering::Relaxed)
        );
        metric!(
            "tokenwise_cache_hits_total",
            "counter",
            "Total number of requests served from response cache.",
            self.cache_hits_total.load(Ordering::Relaxed)
        );
        metric!(
            "tokenwise_routed_total",
            "counter",
            "Total number of requests that were smart-routed to a cheaper model.",
            self.routed_total.load(Ordering::Relaxed)
        );
        metric!(
            "tokenwise_fallbacks_total",
            "counter",
            "Total number of upstream fallback escalations.",
            self.fallbacks_total.load(Ordering::Relaxed)
        );
        metric!(
            "tokenwise_prompt_tokens_total",
            "counter",
            "Total prompt tokens processed.",
            self.prompt_tokens_total.load(Ordering::Relaxed)
        );
        metric!(
            "tokenwise_completion_tokens_total",
            "counter",
            "Total completion tokens generated.",
            self.completion_tokens_total.load(Ordering::Relaxed)
        );

        let cost_micros = self.cost_micros_total.load(Ordering::Relaxed);
        let cost_usd = cost_micros as f64 / 1_000_000.0;
        out.push_str(&format!(
            "# HELP tokenwise_cost_usd_total Total estimated USD cost.\n\
             # TYPE tokenwise_cost_usd_total gauge\n\
             tokenwise_cost_usd_total {:.6}\n",
            cost_usd
        ));

        metric!(
            "tokenwise_empty_streams_total",
            "counter",
            "Streaming responses that returned zero content (safety net).",
            self.empty_streams_total.load(Ordering::Relaxed)
        );
        metric!(
            "tokenwise_truncated_streams_total",
            "counter",
            "Streaming responses that were truncated — finish_reason=length/max_tokens (safety net).",
            self.truncated_streams_total.load(Ordering::Relaxed)
        );

        // Cache hit ratio
        let total = self.requests_total.load(Ordering::Relaxed);
        let hits = self.cache_hits_total.load(Ordering::Relaxed);
        let ratio = if total > 0 {
            hits as f64 / total as f64
        } else {
            0.0
        };
        out.push_str(&format!(
            "# HELP tokenwise_cache_hit_ratio Ratio of cache hits to total requests.\n\
             # TYPE tokenwise_cache_hit_ratio gauge\n\
             tokenwise_cache_hit_ratio {:.4}\n",
            ratio
        ));

        out
    }
}

/// GET /metrics handler — returns Prometheus text.
pub async fn metrics_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<super::AppState>>,
) -> String {
    state.metrics.render()
}
