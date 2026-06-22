//! Transparent proxy — intercept, classify, route, tee.

pub mod anthropic_format;
pub mod classifier;
pub mod router;
pub mod server;
pub mod tee_stream;

use std::convert::Infallible;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use hyper::body::Incoming;
use hyper::service::Service;

use crate::admin::Metrics;
use crate::config::Config;
use crate::recording::Store;
use crate::webhooks::WebhookDispatcher;

/// Build the hyper service that handles all incoming proxy requests.
#[allow(clippy::type_complexity)]
pub fn build_service(
    cfg: Arc<Config>,
    store: Arc<Store>,
    routing_enabled: bool,
    metrics: Arc<Metrics>,
    webhook: Option<Arc<tokio::sync::Mutex<WebhookDispatcher>>>,
) -> impl Service<
    hyper::Request<Incoming>,
    Response = hyper::Response<BoxBody<Bytes, String>>,
    Error = Infallible,
    Future = std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<hyper::Response<BoxBody<Bytes, String>>, Infallible>,
                > + Send,
        >,
    >,
> + Clone {
    server::ProxyService::new(cfg, store, routing_enabled, metrics, webhook)
}
