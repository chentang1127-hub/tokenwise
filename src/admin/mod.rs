//! Admin API + Dashboard — served on port 9400.

mod api;

use std::sync::Arc;

use axum::Router;

use crate::config::Config;
use crate::recording::Store;

/// Shared application state.
pub struct AppState {
    pub config: Arc<Config>,
    pub store: Arc<Store>,
    pub routing_enabled: bool,
}

/// Build the admin router.
pub fn build_router(state: Arc<AppState>) -> Router {
    api::make_router(state)
}
