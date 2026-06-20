//! Transparent proxy server — the heart of TokenWise.
//!
//! Handles OpenAI-compatible `/v1/chat/completions` requests.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use futures_util::StreamExt;
use http_body_util::{BodyExt, Full, StreamBody, combinators::BoxBody};
use hyper::body::Incoming;
use hyper::service::Service;
use hyper::{Request, Response, StatusCode};
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
    /// Whether smart routing is enabled (Pro feature).
    routing_enabled: bool,
}

impl ProxyService {
    pub fn new(cfg: Arc<Config>, store: Arc<Store>, routing_enabled: bool) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(cfg.proxy.timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            cfg,
            store,
            client,
            routing_enabled,
        }
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
    async fn handle(
        &self,
        req: Request<Incoming>,
    ) -> Result<Response<BoxBody<Bytes, String>>, Infallible> {
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
                return Ok(Self::error_response(
                    StatusCode::BAD_REQUEST,
                    &format!("Failed to read body: {e}"),
                ));
            }
        };

        // Parse request JSON
        let request_json: serde_json::Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => {
                warn!("Invalid JSON in request: {e}");
                return Ok(Self::error_response(
                    StatusCode::BAD_REQUEST,
                    &format!("Invalid JSON: {e}"),
                ));
            }
        };

        // Extract messages for classification
        let messages = request_json
            .get("messages")
            .and_then(|m| m.as_array())
            .cloned()
            .unwrap_or_default();

        // Classify complexity
        let complexity = classify(&messages, None, &self.cfg.routing);
        debug!("Classified as {:?}", complexity);

        // Compute the recommended route (what Pro would use)
        let recommended_route = route(complexity, &self.cfg);
        let recommended_provider = recommended_route.provider.clone();
        let recommended_model_id = recommended_route.model.clone();

        // Determine the actual route to use
        let (actual_route, routed_model_id, was_routed) = if self.routing_enabled {
            // Pro: rewrite model to the cheapest capable one
            let model = recommended_route.model.clone();
            (recommended_route, model, true)
        } else {
            // Free: keep the original requested model — passthrough
            let original_model = request_json
                .get("model")
                .and_then(|m| m.as_str())
                .unwrap_or("deepseek-chat");
            // Build a route that resolves the original model to a provider
            let passthrough = self.resolve_original_route(original_model);
            (passthrough, original_model.to_string(), false)
        };

        // Build upstream request body
        let mut upstream_json = request_json.clone();
        upstream_json["model"] = serde_json::Value::String(routed_model_id.clone());
        let upstream_body =
            serde_json::to_vec(&upstream_json).unwrap_or_else(|_| body_bytes.to_vec());

        // Make upstream request (with optional fallback)
        let (response, actual_route, fallback_used) =
            self.try_upstream(&upstream_body, &actual_route).await;

        let latency_ms = start.elapsed().as_millis() as u64;

        let upstream_response = match response {
            Ok(r) => r,
            Err(e) => {
                error!("All upstream attempts failed: {e}");
                return Ok(Self::error_response(
                    StatusCode::BAD_GATEWAY,
                    &format!("Upstream failed: {e}"),
                ));
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
                result.map(http_body::Frame::data).map_err(|s| {
                    Box::new(std::io::Error::other(s)) as Box<dyn std::error::Error + Send + Sync>
                })
            });
            let stream_body = StreamBody::new(body_stream);
            let boxed = BoxBody::new(
                BodyExt::boxed(stream_body)
                    .map_err(|e: Box<dyn std::error::Error + Send + Sync>| e.to_string()),
            );

            // Spawn recording task — awaits analyzer completion so token counts are real
            let store = self.store.clone();
            let cfg = self.cfg.clone();
            let request_json_clone = request_json.clone();
            let model = actual_route.model.clone();
            let provider = actual_route.provider.clone();
            let complexity_name = complexity.tier_name().to_string();
            let recommended_model = if was_routed {
                None
            } else {
                Some(format!("{}/{}", recommended_provider, recommended_model_id))
            };

            tokio::spawn(async move {
                // This resolves only after TeeStream is dropped (client consumed all chunks)
                // and the analyzer has finished processing every chunk.
                let metrics = analyzer_handle.await.unwrap_or_default();
                let prompt = metrics.prompt_tokens.unwrap_or(0);
                let completion = metrics.completion_tokens.unwrap_or(0);

                let mut rec = CallRecord::from_request(
                    &model,
                    &provider,
                    &complexity_name,
                    fallback_used,
                    latency_ms,
                )
                .with_usage(prompt, completion);

                rec.was_routed = was_routed;
                rec.recommended_model = recommended_model;

                // Compute actual cost from model pricing
                if let Some(model_cfg) = cfg.model_config(&provider, &model) {
                    rec.cost_usd =
                        crate::cost::calculator::compute_cost(prompt, completion, model_cfg);
                }

                // For Free tier: compute what Pro would have cost
                if !was_routed
                    && let Some(opt_cfg) =
                        cfg.model_config(&recommended_provider, &recommended_model_id)
                {
                    rec.estimated_optimal_cost = Some(crate::cost::calculator::compute_cost(
                        prompt, completion, opt_cfg,
                    ));
                }

                if let Some(ref reason) = metrics.finish_reason {
                    let _ = reason;
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

            // Extract usage and compute cost
            let mut rec = CallRecord::from_request(
                &actual_route.model,
                &actual_route.provider,
                complexity.tier_name(),
                fallback_used,
                latency_ms,
            );

            rec.was_routed = was_routed;
            if !was_routed {
                rec.recommended_model =
                    Some(format!("{}/{}", recommended_provider, recommended_model_id));
            }

            if let Ok(response_json) = serde_json::from_slice::<serde_json::Value>(&resp_bytes)
                && let Some(usage) = response_json.get("usage")
            {
                let prompt = usage
                    .get("prompt_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                let completion = usage
                    .get("completion_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                rec = rec.with_usage(prompt, completion);

                // Compute actual cost from model pricing
                if let Some(model_cfg) = self
                    .cfg
                    .model_config(&actual_route.provider, &actual_route.model)
                {
                    rec.cost_usd =
                        crate::cost::calculator::compute_cost(prompt, completion, model_cfg);
                }

                // For Free tier: compute what Pro would have cost
                if !was_routed
                    && let Some(opt_cfg) = self
                        .cfg
                        .model_config(&recommended_provider, &recommended_model_id)
                {
                    rec.estimated_optimal_cost = Some(crate::cost::calculator::compute_cost(
                        prompt, completion, opt_cfg,
                    ));
                }

                debug!(
                    "Usage: prompt={prompt}, completion={completion}, cost=${:.6}",
                    rec.cost_usd
                );
            }

            let _ = self.store.record_call(&rec, &request_json);

            Ok(Response::builder()
                .status(resp_status)
                .body(BoxBody::new(
                    Full::new(resp_bytes).map_err(|e: Infallible| match e {}),
                ))
                .unwrap())
        }
    }

    /// Resolve the original client-requested model to a provider route.
    /// Used by Free tier (passthrough mode) to forward without model rewriting.
    /// Falls back to first configured provider if model not found.
    fn resolve_original_route(&self, original_model: &str) -> crate::proxy::router::Route {
        // Try to find the model in any configured provider
        for provider in &self.cfg.providers {
            for model in &provider.models {
                if model.id == original_model {
                    let api_key = std::env::var(&provider.api_key_env).unwrap_or_default();
                    return crate::proxy::router::Route {
                        provider: provider.name.clone(),
                        model: model.id.clone(),
                        base_url: provider.base_url.clone(),
                        api_key,
                        tier: model.tier.clone(),
                    };
                }
            }
        }
        // Model not found in config — use first available provider
        let p = self.cfg.providers.first().expect("No providers configured");
        let m = p.models.first().expect("No models configured for provider");
        let api_key = std::env::var(&p.api_key_env).unwrap_or_default();
        debug!(
            "Model '{original_model}' not in config, routing via {}/{}",
            p.name, m.id
        );
        crate::proxy::router::Route {
            provider: p.name.clone(),
            model: m.id.clone(),
            base_url: p.base_url.clone(),
            api_key,
            tier: m.tier.clone(),
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
                    warn!(
                        "Upstream {} returned {status_code}, attempting fallback...",
                        route.model
                    );

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

                (
                    Err(format!("Upstream returned {status_code}")),
                    route.clone(),
                    false,
                )
            }
            Err(e) => {
                warn!(
                    "Upstream {} error: {e}. Attempting fallback...",
                    route.model
                );

                if self.cfg.safety_net.enabled
                    && let Some(fb_route) = fallback_route(&route.tier, &self.cfg)
                {
                    let fb_url = format!("{}/chat/completions", fb_route.base_url);
                    let fb_req = self
                        .client
                        .post(&fb_url)
                        .header("Authorization", format!("Bearer {}", fb_route.api_key))
                        .header("Content-Type", "application/json")
                        .body(body.to_vec());

                    match fb_req.send().await {
                        Ok(fb_resp) => {
                            info!(
                                "Fallback to {} succeeded after connection error",
                                fb_route.model
                            );
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

                (Err(format!("Upstream error: {e}")), route.clone(), false)
            }
        }
    }
}

impl Service<Request<Incoming>> for ProxyService {
    type Response = Response<BoxBody<Bytes, String>>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        let this = self.clone();
        Box::pin(async move { Ok(this.handle(req).await.unwrap()) })
    }
}
