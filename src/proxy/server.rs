//! Transparent proxy server — the heart of TokenWise.
//!
//! Handles OpenAI-compatible `/v1/chat/completions` requests.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use hyper::body::Incoming;
use hyper::service::Service;
use hyper::{Request, Response, StatusCode};
use http_body_util::{combinators::BoxBody, BodyExt, Full, StreamBody};
use futures_util::StreamExt;
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::proxy::classifier::classify;
use crate::proxy::router::{fallback_route, route};
use crate::proxy::tee_stream::spawn_analyzer;
use crate::recording::{CallRecord, Store};

/// Clone-able proxy service.
#[derive(Clone)]
pub struct ProxyService {
    cfg: Arc<Config>,
    store: Arc<Store>,
    client: reqwest::Client,
}

impl ProxyService {
    pub fn new(cfg: Arc<Config>, store: Arc<Store>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(cfg.proxy.timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        Self { cfg, store, client }
    }

    /// Helper: create a JSON error response.
    fn error_response(status: StatusCode, msg: &str) -> Response<BoxBody<Bytes, String>> {
        let json = format!(r#"{{"error":"{}"}}"#, msg.replace('"', r#"\""#));
        Response::builder()
            .status(status)
            .body(BoxBody::new(
                Full::new(Bytes::from(json)).map_err(|e: Infallible| match e {}),
            ))
            .unwrap()
    }

    /// Core request handling.
    async fn handle(&self, req: Request<Incoming>) -> Result<Response<BoxBody<Bytes, String>>, Infallible> {
        let start = Instant::now();

        // Only handle /v1/chat/completions
        if req.uri().path() != "/v1/chat/completions" {
            return Ok(Self::error_response(
                StatusCode::NOT_FOUND,
                "TokenWise proxy only handles /v1/chat/completions",
            ));
        }

        // Collect the full request body
        let (_parts, body) = req.into_parts();
        let body_bytes = match body.collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(e) => {
                return Ok(Self::error_response(StatusCode::BAD_REQUEST, &format!("Failed to read body: {e}")));
            }
        };

        // Parse request JSON
        let request_json: serde_json::Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => {
                warn!("Invalid JSON in request: {e}");
                return Ok(Self::error_response(StatusCode::BAD_REQUEST, &format!("Invalid JSON: {e}")));
            }
        };

        // Extract messages for classification
        let messages = request_json
            .get("messages")
            .and_then(|m| m.as_array())
            .cloned()
            .unwrap_or_default();

        // Classify complexity → route
        let complexity = classify(&messages, None, &self.cfg.routing);
        debug!("Classified as {:?}", complexity);

        let primary_route = route(complexity, &self.cfg);

        // Make upstream request (with optional fallback)
        let (response, actual_route, fallback_used) = self
            .try_upstream(&body_bytes, &primary_route)
            .await;

        let latency_ms = start.elapsed().as_millis() as u64;

        let upstream_response = match response {
            Ok(r) => r,
            Err(e) => {
                error!("All upstream attempts failed: {e}");
                return Ok(Self::error_response(StatusCode::BAD_GATEWAY, &format!("Upstream failed: {e}")));
            }
        };

        let status = upstream_response.status();

        // Check if streaming
        let is_stream = request_json
            .get("stream")
            .and_then(|s| s.as_bool())
            .unwrap_or(false);

        if is_stream {
            // Set up tee stream + analyzer
            let (tx, analyzer_handle) = spawn_analyzer();
            let upstream_stream = upstream_response.bytes_stream();
            let tee = crate::proxy::tee_stream::TeeStream::new(upstream_stream, tx);

            // Convert TeeStream into an http_body Body by mapping items to frames
            let body_stream = tee.map(|result| {
                result
                    .map(|bytes| http_body::Frame::data(bytes))
                    .map_err(|s| Box::new(std::io::Error::new(std::io::ErrorKind::Other, s))
                        as Box<dyn std::error::Error + Send + Sync>)
            });
            let stream_body = StreamBody::new(body_stream);
            let boxed = BoxBody::new(BodyExt::boxed(stream_body).map_err(|e: Box<dyn std::error::Error + Send + Sync>| e.to_string()));

            // Spawn recording task — awaits analyzer completion so token counts are real
            let store = self.store.clone();
            let request_json_clone = request_json.clone();
            let model = actual_route.model.clone();
            let provider = actual_route.provider.clone();
            let complexity_name = complexity.tier_name().to_string();

            tokio::spawn(async move {
                // This resolves only after TeeStream is dropped (client consumed all chunks)
                // and the analyzer has finished processing every chunk.
                let metrics = analyzer_handle.await.unwrap_or_default();

                let rec = CallRecord::from_request(
                    &model,
                    &provider,
                    &complexity_name,
                    fallback_used,
                    latency_ms,
                )
                .with_usage(
                    metrics.prompt_tokens.unwrap_or(0),
                    metrics.completion_tokens.unwrap_or(0),
                );

                if let Some(ref reason) = metrics.finish_reason {
                    let _ = reason; // recorded in a future schema migration
                }

                if let Err(e) = store.record_call(&rec, &request_json_clone) {
                    error!("Failed to record streaming call: {e}");
                }
            });

            Ok(Response::builder().status(status).body(boxed).unwrap())
        } else {
            // Non-streaming: collect body, record, return
            let resp_status = status;
            let resp_bytes = match upstream_response.bytes().await {
                Ok(b) => b,
                Err(e) => {
                    return Ok(Self::error_response(
                        StatusCode::BAD_GATEWAY,
                        &format!("Failed to read upstream: {e}"),
                    ));
                }
            };

            // Extract usage from response for better tracking
            if let Ok(response_json) = serde_json::from_slice::<serde_json::Value>(&resp_bytes) {
                if let Some(usage) = response_json.get("usage") {
                    debug!("Usage: {:?}", usage);
                }
            }

            let rec = CallRecord::from_request(
                &actual_route.model,
                &actual_route.provider,
                complexity.tier_name(),
                fallback_used,
                latency_ms,
            );

            let _ = self.store.record_call(&rec, &request_json);

            Ok(Response::builder()
                .status(resp_status)
                .body(BoxBody::new(
                    Full::new(resp_bytes).map_err(|e: Infallible| match e {}),
                ))
                .unwrap())
        }
    }

    /// Try an upstream host. On failure, attempt fallback if configured.
    async fn try_upstream(
        &self,
        body: &[u8],
        route: &crate::proxy::router::Route,
    ) -> (
        Result<reqwest::Response, String>,
        crate::proxy::router::Route,
        bool,
    ) {
        let url = format!("{}/chat/completions", route.base_url);

        let req = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", route.api_key))
            .header("Content-Type", "application/json")
            .body(body.to_vec());

        match req.send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    return (Ok(resp), route.clone(), false);
                }

                // Save status before draining body
                let status_code = resp.status();

                if self.cfg.safety_net.enabled {
                    warn!("Upstream {} returned {status_code}, attempting fallback...", route.model);

                    // Drain body to allow connection reuse
                    let _ = resp.bytes().await;

                    if let Some(fb_route) = fallback_route(&route.tier, &self.cfg) {
                        let fb_url = format!("{}/chat/completions", fb_route.base_url);
                        let fb_req = self
                            .client
                            .post(&fb_url)
                            .header("Authorization", format!("Bearer {}", fb_route.api_key))
                            .header("Content-Type", "application/json")
                            .body(body.to_vec());

                        match fb_req.send().await {
                            Ok(fb_resp) => {
                                info!("Fallback to {} succeeded", fb_route.model);
                                return (Ok(fb_resp), fb_route, true);
                            }
                            Err(e) => {
                                return (Err(format!("Fallback also failed: {e}")), fb_route, true);
                            }
                        }
                    }
                } else {
                    // Drain body to avoid leaking connection
                    let _ = resp.bytes().await;
                }

                (Err(format!("Upstream returned {status_code}")), route.clone(), false)
            }
            Err(e) => {
                warn!("Upstream {} error: {e}. Attempting fallback...", route.model);

                if self.cfg.safety_net.enabled {
                    if let Some(fb_route) = fallback_route(&route.tier, &self.cfg) {
                        let fb_url = format!("{}/chat/completions", fb_route.base_url);
                        let fb_req = self
                            .client
                            .post(&fb_url)
                            .header("Authorization", format!("Bearer {}", fb_route.api_key))
                            .header("Content-Type", "application/json")
                            .body(body.to_vec());

                        match fb_req.send().await {
                            Ok(fb_resp) => {
                                info!("Fallback to {} succeeded after connection error", fb_route.model);
                                return (Ok(fb_resp), fb_route, true);
                            }
                            Err(fb_e) => {
                                return (
                                    Err(format!("Both primary and fallback failed: {e} / {fb_e}")),
                                    fb_route,
                                    true,
                                );
                            }
                        }
                    }
                }

                (Err(format!("Upstream error: {e}")), route.clone(), false)
            }
        }
    }
}

impl Service<Request<Incoming>> for ProxyService {
    type Response = Response<BoxBody<Bytes, String>>;
    type Error = Infallible;
    type Future = Pin<
        Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        let this = self.clone();
        Box::pin(async move { Ok(this.handle(req).await.unwrap()) })
    }
}
