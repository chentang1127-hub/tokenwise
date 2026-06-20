//! Transparent proxy — intercept, classify, route, tee.

pub mod classifier;
pub mod router;
pub mod server;
pub mod tee_stream;

use std::convert::Infallible;
use std::sync::Arc;

use bytes::Bytes;
use hyper::body::Incoming;
use hyper::service::Service;
use http_body_util::combinators::BoxBody;

use crate::config::Config;
use crate::recording::Store;

/// Build the hyper service that handles all incoming proxy requests.
pub fn build_service(
    cfg: Arc<Config>,
    store: Arc<Store>,
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
    server::ProxyService::new(cfg, store)
}
