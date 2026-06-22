//! Admin API + Dashboard — served on port 9400.

mod api;
pub mod chat_widget;
pub mod metrics;

use std::sync::Arc;
use std::time::Instant;

use axum::Router;

use crate::config::Config;
use crate::recording::Store;

pub use metrics::Metrics;

/// Shared application state.
pub struct AppState {
    pub config: Arc<Config>,
    pub store: Arc<Store>,
    pub routing_enabled: bool,
    /// Path to the config file, used by Pro setup to persist API keys.
    #[allow(dead_code)]
    pub config_path: String,
    /// Prometheus metrics counters.
    pub metrics: Arc<Metrics>,
    /// Server start time (used for uptime in health check).
    pub start_time: Instant,
}

/// Build the admin router.
pub fn build_router(state: Arc<AppState>) -> Router {
    api::make_router(state)
}
