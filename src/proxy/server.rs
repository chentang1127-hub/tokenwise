//! Transparent proxy server — the heart of TokenWise Core.
//!
//! Handles both OpenAI-compatible `/v1/chat/completions` and
//! Anthropic Messages API `/v1/messages` requests, translating
//! between formats so Claude Code and other Anthropic-native
//! clients can route through TokenWise.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use futures_util::StreamExt;
use http_body_util::{BodyExt, Full, StreamBody, combinators::BoxBody};
use hyper::body::Incoming;
use hyper::header::{HeaderName, HeaderValue};
use hyper::service::Service;
use hyper::{Request, Response, StatusCode};
use tracing::{debug, error, info, warn};

use crate::admin::Metrics;
use crate::config::Config;
use crate::proxy::anthropic_format::{
    AnthropicSseState, AnthropicSseStream, anthropic_to_openai,
    openai_to_anthropic,
};
use crate::proxy::classifier::classify;
use crate::proxy::router::{fallback_route, fallback_route_within_provider, route, route_within_provider};
use crate::proxy::tee_stream::spawn_analyzer;
use crate::recording::{CallRecord, Store};
use crate::webhooks::WebhookDispatcher;

/// Clone-able proxy service.
#[derive(Clone)]
pub struct ProxyService {
    cfg: Arc<Config>,
    store: Arc<Store>,
    client: reqwest::Client,
    /// Whether smart routing is enabled (Pro feature).
    routing_enabled: bool,
    /// Prometheus metrics counters.
    metrics: Arc<Metrics>,
    /// Webhook dispatcher for budget alerts (None if no URL configured).
    webhook: Option<Arc<tokio::sync::Mutex<WebhookDispatcher>>>,
}

impl ProxyService {
    pub fn new(
        cfg: Arc<Config>,
        store: Arc<Store>,
        routing_enabled: bool,
        metrics: Arc<Metrics>,
        webhook: Option<Arc<tokio::sync::Mutex<WebhookDispatcher>>>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(cfg.proxy.timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            cfg,
            store,
            client,
            routing_enabled,
            metrics,
            webhook,
        }
    }

    /// Inject CORS headers into a response so browser JS on the admin
    /// origin (port 9400) can call the proxy (port 9401) directly.
    /// The user's API key stays in the browser — zero-trust preserved.
    fn add_cors_headers(resp: &mut Response<BoxBody<Bytes, String>>) {
        resp.headers_mut().insert(
            HeaderName::from_static("access-control-allow-origin"),
            HeaderValue::from_static("*"),
        );
        resp.headers_mut().insert(
            HeaderName::from_static("access-control-allow-methods"),
            HeaderValue::from_static("POST, OPTIONS"),
        );
        resp.headers_mut().insert(
            HeaderName::from_static("access-control-allow-headers"),
            HeaderValue::from_static("Content-Type, Authorization"),
        );
    }

    /// Helper: create a JSON error response.
    fn error_response(status: StatusCode, msg: &str) -> Response<BoxBody<Bytes, String>> {
        let json = format!(r#"{{"error":"{}"}}"#, msg.replace('"', r#"\""#));
        let mut resp = Response::builder()
            .status(status)
            .body(BoxBody::new(
                Full::new(Bytes::from(json)).map_err(|e: Infallible| match e {}),
            ))
            .unwrap();
        Self::add_cors_headers(&mut resp);
        resp
    }

    /// Core request handling.
    async fn handle(
        &self,
        req: Request<Incoming>,
    ) -> Result<Response<BoxBody<Bytes, String>>, Infallible> {
        // Handle CORS preflight — browser sends OPTIONS before cross-origin POST.
        if req.method() == hyper::Method::OPTIONS {
            let mut resp = Response::builder()
                .status(StatusCode::NO_CONTENT)
                .body(BoxBody::new(
                    Full::new(Bytes::new()).map_err(|e: Infallible| match e {}),
                ))
                .unwrap();
            Self::add_cors_headers(&mut resp);
            return Ok(resp);
        }

        let start = Instant::now();

        // Path-based routing.
        //
        // OpenAI format:
        //   /v1/chat/completions          → smart routing
        //   /v1/{provider}/chat/completions → force provider
        //
        // Anthropic format:
        //   /v1/messages                  → smart routing
        //   /v1/{provider}/messages       → force provider
        let path = req.uri().path().to_string();
        let (force_provider, is_anthropic): (Option<String>, bool) = if path == "/v1/chat/completions" {
            (None, false)
        } else if path == "/v1/messages" {
            (None, true)
        } else if path.starts_with("/v1/") && path.ends_with("/chat/completions") {
            let middle = path
                .strip_prefix("/v1/")
                .and_then(|s| s.strip_suffix("/chat/completions"))
                .unwrap_or("");
            if middle.is_empty() || !self.cfg.providers.iter().any(|p| p.name == middle) {
                return Ok(Self::error_response(
                    StatusCode::NOT_FOUND,
                    &format!("Unknown provider '{middle}'. Available: {}",
                        self.cfg.providers.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")),
                ));
            }
            (Some(middle.to_string()), false)
        } else if path.starts_with("/v1/") && path.ends_with("/messages") {
            let middle = path
                .strip_prefix("/v1/")
                .and_then(|s| s.strip_suffix("/messages"))
                .unwrap_or("");
            if middle.is_empty() || !self.cfg.providers.iter().any(|p| p.name == middle) {
                return Ok(Self::error_response(
                    StatusCode::NOT_FOUND,
                    &format!("Unknown provider '{middle}'. Available: {}",
                        self.cfg.providers.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")),
                ));
            }
            (Some(middle.to_string()), true)
        } else {
            return Ok(Self::error_response(
                StatusCode::NOT_FOUND,
                "TokenWise Core proxy handles /v1/chat/completions, /v1/messages, and their provider-scoped variants",
            ));
        };

        // Budget check: block requests if daily/monthly limit exceeded
        if self.cfg.budget.daily_limit_usd > 0.0 || self.cfg.budget.monthly_limit_usd > 0.0 {
            let now = chrono::Utc::now();
            if self.cfg.budget.daily_limit_usd > 0.0 {
                let today_start = now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
                let spent_today = self.store.total_cost_since(today_start);
                if spent_today >= self.cfg.budget.daily_limit_usd {
                    return Ok(Self::error_response(
                        StatusCode::TOO_MANY_REQUESTS,
                        &format!("Daily budget exceeded: ${:.4} / ${:.2}", spent_today, self.cfg.budget.daily_limit_usd),
                    ));
                }
            }
            if self.cfg.budget.monthly_limit_usd > 0.0 {
                let month_start = now.format("%Y-%m-01").to_string();
                let month_start_ts = chrono::NaiveDate::parse_from_str(&month_start, "%Y-%m-%d")
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
                    .timestamp();
                let spent_month = self.store.total_cost_since(month_start_ts);
                if spent_month >= self.cfg.budget.monthly_limit_usd {
                    return Ok(Self::error_response(
                        StatusCode::TOO_MANY_REQUESTS,
                        &format!("Monthly budget exceeded: ${:.4} / ${:.2}", spent_month, self.cfg.budget.monthly_limit_usd),
                    ));
                }
            }
        }

        // Capture the client's auth header for passthrough.
        // Anthropic clients use `x-api-key`, OpenAI clients use `Authorization: Bearer`.
        // Free tier forwards the client's own key — TokenWise never sees it.
        let client_auth = req
            .headers()
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .or_else(|| {
                req.headers()
                    .get("x-api-key")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| format!("Bearer {s}"))
            });

        // Derive tenant ID from the client's API key (SHA-256 hash, never stored raw).
        let tenant_id = crate::multi_user::extract_tenant(client_auth.as_deref());

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
        let mut request_json: serde_json::Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => {
                warn!("Invalid JSON in request: {e}");
                return Ok(Self::error_response(
                    StatusCode::BAD_REQUEST,
                    &format!("Invalid JSON: {e}"),
                ));
            }
        };

        // If Anthropic format, translate to OpenAI format for internal processing.
        // Save original model name for response translation.
        let (anthropic_model, _anthropic_system) = if is_anthropic {
            let model = request_json
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let system = request_json
                .get("system")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            // Translate in-place — classification, routing, and upstream
            // all work in OpenAI format.
            let openai_req = anthropic_to_openai(&request_json);
            debug!("Anthropic → OpenAI: model={model}");
            request_json = openai_req;
            (Some(model), system)
        } else {
            (None, None)
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

        // Compute the recommended route (what Pro would use across all providers)
        let recommended_route = route(complexity, &self.cfg);
        let recommended_provider = recommended_route.provider.clone();
        let recommended_model_id = recommended_route.model.clone();

        // Determine the actual route to use.
        // When force_provider is set (path-based routing), constrain to that provider.
        let (actual_route, routed_model_id, was_routed) = {
            // Use the first model from the first provider as fallback,
            // rather than a hardcoded model name that may not be in config.
            let default_model = self.cfg
                .providers
                .first()
                .and_then(|p| p.models.first())
                .map(|m| m.id.as_str())
                .unwrap_or("deepseek-chat");
            let original_model = request_json
                .get("model")
                .and_then(|m| m.as_str())
                .unwrap_or(default_model);

            if let Some(ref fp) = force_provider {
                // Path-based routing: force a specific provider
                let route = self.resolve_original_route(original_model);
                let provider_route = crate::proxy::router::Route {
                    provider: fp.clone(),
                    model: original_model.to_string(),
                    base_url: self.cfg.providers.iter()
                        .find(|p| p.name == *fp)
                        .map(|p| p.base_url.clone())
                        .unwrap_or(route.base_url),
                    api_key: String::new(),
                    tier: route.tier,
                };
                if self.routing_enabled {
                    // Pro: try to pick the cheapest model within the forced provider
                    if let Some(within) = route_within_provider(complexity, &self.cfg, fp) {
                        let m = within.model.clone();
                        (within, m, true)
                    } else {
                        (provider_route, original_model.to_string(), false)
                    }
                } else {
                    (provider_route, original_model.to_string(), false)
                }
            } else if self.routing_enabled {
                // Pro: pick the cheapest model in the appropriate tier,
                // BUT constrained to the user's original provider.
                // In zero-trust mode the client's API key only works
                // for their own provider — cross-provider routing would 401.
                let original_provider = self.resolve_original_route(original_model);
                let provider_name = &original_provider.provider;

                if let Some(within) = route_within_provider(complexity, &self.cfg, provider_name) {
                    let model = within.model.clone();
                    (within, model, true)
                } else {
                    debug!(
                        "No {} model in tier for provider {provider_name}, staying on {original_model}",
                        complexity.tier_name(),
                    );
                    (original_provider, original_model.to_string(), false)
                }
            } else {
                // Free: keep the original requested model — passthrough
                let passthrough = self.resolve_original_route(original_model);
                (passthrough, original_model.to_string(), false)
            }
        };

        // Compute cache key from messages + model (Pro feature)
        let is_stream = request_json
            .get("stream")
            .and_then(|s| s.as_bool())
            .unwrap_or(false);

        let cache_hash = if self.routing_enabled && !is_stream {
            let messages_str = serde_json::to_string(&messages).unwrap_or_default();
            let cache_input = format!("{}:{}", routed_model_id, messages_str);
            Some(CallRecord::hash_prompt(&cache_input))
        } else {
            None
        };

        // Check cache (Pro only, non-streaming only)
        if let Some(ref hash) = cache_hash
            && let Some(cached_json) = self.store.cache_get(hash, self.cfg.cache.ttl_hours)
        {
            let latency_ms = start.elapsed().as_millis() as u64;
            debug!("Cache HIT ({latency_ms}ms) — saved an API call");

            self.metrics.inc_requests();
            self.metrics.inc_cache_hits();

            // Record as a synthetic call so it's counted
            let mut rec = CallRecord::from_request(
                &routed_model_id,
                &actual_route.provider,
                complexity.tier_name(),
                false,
                latency_ms,
            );
            rec.tenant_id = tenant_id.0.clone();
            let _ = self.store.record_call(&rec, &request_json);

            // Convert cached OpenAI-format response to Anthropic if needed
            let response_bytes = if is_anthropic {
                if let Ok(openai_resp) =
                    serde_json::from_slice::<serde_json::Value>(cached_json.as_bytes())
                {
                    let display_model = anthropic_model.as_deref().unwrap_or(&routed_model_id);
                    let anthropic_resp = openai_to_anthropic(&openai_resp, display_model);
                    Bytes::from(serde_json::to_vec(&anthropic_resp).unwrap_or_default())
                } else {
                    Bytes::from(cached_json)
                }
            } else {
                Bytes::from(cached_json)
            };

            let mut resp = Response::builder()
                .status(StatusCode::OK)
                .body(BoxBody::new(
                    Full::new(response_bytes).map_err(|e: Infallible| match e {}),
                ))
                .unwrap();
            Self::add_cors_headers(&mut resp);
            return Ok(resp);
        }

        // Build upstream request body
        let mut upstream_json = request_json.clone();
        upstream_json["model"] = serde_json::Value::String(routed_model_id.clone());
        let upstream_body =
            serde_json::to_vec(&upstream_json).unwrap_or_else(|_| body_bytes.to_vec());

        // Make upstream request (with optional fallback)
        let (response, actual_route, fallback_used) =
            self.try_upstream(&upstream_body, &actual_route, client_auth.as_deref()).await;

        let latency_ms = start.elapsed().as_millis() as u64;

        let upstream_response = match response {
            Ok(r) => r,
            Err(e) => {
                error!("All upstream attempts failed: {e}");
                self.metrics.inc_requests();
                return Ok(Self::error_response(
                    StatusCode::BAD_GATEWAY,
                    &format!("Upstream failed: {e}"),
                ));
            }
        };

        self.metrics.inc_requests();
        if was_routed {
            self.metrics.inc_routed();
        }
        if fallback_used {
            self.metrics.inc_fallbacks();
        }

        let status = upstream_response.status();

        if is_stream {
            if is_anthropic {
                // ── Anthropic format streaming ──────────────────────
                // Wrap upstream OpenAI SSE stream and translate to Anthropic SSE.
                let msg_id = format!("msg_{}", uuid::Uuid::new_v4());
                let display_model = anthropic_model.as_deref().unwrap_or(&routed_model_id);

                let sse_state = AnthropicSseState::new(
                    msg_id.clone(),
                    display_model.to_string(),
                    0u32,
                );
                let upstream_stream = upstream_response.bytes_stream();
                let (anthropic_stream, shared_state) = AnthropicSseStream::new(upstream_stream, sse_state);

                // Convert to http_body Body
                let body_stream = anthropic_stream.map(|result| {
                    result.map(http_body::Frame::data).map_err(|s| {
                        Box::new(std::io::Error::other(s))
                            as Box<dyn std::error::Error + Send + Sync>
                    })
                });
                let stream_body = StreamBody::new(body_stream);
                let boxed = BoxBody::new(
                    BodyExt::boxed(stream_body)
                        .map_err(|e: Box<dyn std::error::Error + Send + Sync>| e.to_string()),
                );

                // Spawn recording task — reads real token counts from shared_state
                // after the stream is fully consumed by hyper.
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
                let stream_metrics = self.metrics.clone();
                let tenant_for_spawn = tenant_id.0.clone();
                let webhook_anthro = self.webhook.clone();
                let budget_anthro = self.cfg.budget.clone();

                tokio::spawn(async move {
                    // Wait for the stream to be consumed (hyper reads until None).
                    // Poll the shared state with backoff up to 60 seconds.
                    for _ in 0..600 {
                        if shared_state.lock().unwrap().done {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }

                    // Read real token counts from the completed SSE state
                    let (prompt_tokens, completion_tokens, stream_empty, stream_truncated) = {
                        let s = shared_state.lock().unwrap();
                        let pt = if s.input_tokens > 0 {
                            s.input_tokens
                        } else {
                            serde_json::to_string(&request_json_clone.get("messages").unwrap_or(&serde_json::json!([])))
                                .unwrap_or_default().len() as u32 / 4
                        };
                        let ct = s.output_tokens;
                        let empty = s.content.is_empty();
                        let truncated = s.finish_reason.as_deref() == Some("length")
                            || s.finish_reason.as_deref() == Some("max_tokens");
                        (pt, ct, empty, truncated)
                    };

                    let mut rec = CallRecord::from_request(
                        &model,
                        &provider,
                        &complexity_name,
                        fallback_used,
                        latency_ms,
                    )
                    .with_usage(prompt_tokens, completion_tokens);

                    rec.was_routed = was_routed;
                    rec.recommended_model = recommended_model;
                    rec.tenant_id = tenant_for_spawn;

                    if let Some(model_cfg) = cfg.model_config(&provider, &model) {
                        rec.cost_usd =
                            crate::cost::calculator::compute_cost(prompt_tokens, completion_tokens, model_cfg);
                    }

                    stream_metrics.add_tokens(prompt_tokens as u64, completion_tokens as u64);
                    stream_metrics.add_cost(rec.cost_usd);

                    if let Err(e) = store.record_call(&rec, &request_json_clone) {
                        error!("Failed to record Anthropic streaming call: {e}");
                    }

                    // Stream safety net: log warnings for empty/truncated
                    if stream_empty {
                        warn!(
                            "Safety net: Anthropic streaming response was empty (model={}, provider={})",
                            model, provider
                        );
                        stream_metrics.inc_empty_streams();
                    }
                    if stream_truncated {
                        warn!(
                            "Safety net: Anthropic streaming response was truncated (model={}, provider={})",
                            model, provider
                        );
                        stream_metrics.inc_truncated_streams();
                    }

                    // Fire webhook budget alert
                    if let Some(ref wh) = webhook_anthro {
                        let now_ts = chrono::Utc::now().timestamp();
                        let spent_today = if budget_anthro.daily_limit_usd > 0.0 {
                            let today_start = chrono::Utc::now()
                                .date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
                            store.total_cost_since(today_start)
                        } else { 0.0 };
                        let spent_month = if budget_anthro.monthly_limit_usd > 0.0 {
                            let month_start = chrono::Utc::now().format("%Y-%m-01").to_string();
                            let ms_ts = chrono::NaiveDate::parse_from_str(&month_start, "%Y-%m-%d")
                                .unwrap().and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
                            store.total_cost_since(ms_ts)
                        } else { 0.0 };
                        let mut dispatcher = wh.lock().await;
                        dispatcher.check_budget(
                            spent_today, budget_anthro.daily_limit_usd,
                            spent_month, budget_anthro.monthly_limit_usd, now_ts,
                        ).await;
                    }
                });

                let mut resp = Response::builder().status(status).body(boxed).unwrap();
                // For Anthropic SSE, set the correct content type
                resp.headers_mut().insert(
                    hyper::header::CONTENT_TYPE,
                    HeaderValue::from_static("text/event-stream"),
                );
                Self::add_cors_headers(&mut resp);
                Ok(resp)
            } else {
                // ── OpenAI format streaming (existing) ─────────────
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

                let stream_metrics = self.metrics.clone();
                let tenant_for_spawn2 = tenant_id.0.clone();
                let webhook_openai = self.webhook.clone();
                let budget_openai = self.cfg.budget.clone();
                tokio::spawn(async move {
                    let metrics = analyzer_handle.await.unwrap_or_default();
                    let prompt = metrics.prompt_tokens.unwrap_or(0);
                    let completion = metrics.completion_tokens.unwrap_or(0);

                    // Stream safety net: detect empty or truncated streaming responses
                    if metrics.content_preview.is_empty() {
                        warn!(
                            "Safety net: OpenAI streaming response was empty (model={}, provider={})",
                            model, provider
                        );
                        stream_metrics.inc_empty_streams();
                    }
                    if metrics.finish_reason.as_deref() == Some("length") {
                        warn!(
                            "Safety net: OpenAI streaming response was truncated (model={}, provider={})",
                            model, provider
                        );
                        stream_metrics.inc_truncated_streams();
                    }

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
                    rec.tenant_id = tenant_for_spawn2;

                    if let Some(model_cfg) = cfg.model_config(&provider, &model) {
                        rec.cost_usd =
                            crate::cost::calculator::compute_cost(prompt, completion, model_cfg);
                    }

                    if !was_routed
                        && let Some(opt_cfg) =
                            cfg.model_config(&recommended_provider, &recommended_model_id)
                    {
                        rec.estimated_optimal_cost = Some(crate::cost::calculator::compute_cost(
                            prompt, completion, opt_cfg,
                        ));
                    }

                    stream_metrics.add_tokens(prompt as u64, completion as u64);
                    stream_metrics.add_cost(rec.cost_usd);

                    if let Err(e) = store.record_call(&rec, &request_json_clone) {
                        error!("Failed to record streaming call: {e}");
                    }

                    // Fire webhook budget alert
                    if let Some(ref wh) = webhook_openai {
                        let now_ts = chrono::Utc::now().timestamp();
                        let spent_today = if budget_openai.daily_limit_usd > 0.0 {
                            let today_start = chrono::Utc::now()
                                .date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
                            store.total_cost_since(today_start)
                        } else { 0.0 };
                        let spent_month = if budget_openai.monthly_limit_usd > 0.0 {
                            let month_start = chrono::Utc::now().format("%Y-%m-01").to_string();
                            let ms_ts = chrono::NaiveDate::parse_from_str(&month_start, "%Y-%m-%d")
                                .unwrap().and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
                            store.total_cost_since(ms_ts)
                        } else { 0.0 };
                        let mut dispatcher = wh.lock().await;
                        dispatcher.check_budget(
                            spent_today, budget_openai.daily_limit_usd,
                            spent_month, budget_openai.monthly_limit_usd, now_ts,
                        ).await;
                    }
                });

                let mut resp = Response::builder().status(status).body(boxed).unwrap();
                Self::add_cors_headers(&mut resp);
                Ok(resp)
            }
        } else {
            // Non-streaming: collect body, record, return
            let resp_status = status;
            let mut resp_bytes = match upstream_response.bytes().await {
                Ok(b) => b,
                Err(e) => {
                    return Ok(Self::error_response(
                        StatusCode::BAD_GATEWAY,
                        &format!("Failed to read upstream: {e}"),
                    ));
                }
            };

            // ── Safety net: check for empty/truncated responses ──────
            let (mut actual_route_ns, mut fallback_used_ns, mut latency_ms_ns) =
                (actual_route.clone(), fallback_used, latency_ms);
            if self.cfg.safety_net.enabled && !fallback_used {
                let needs_fallback = self.check_empty_or_truncated(&resp_bytes);
                if needs_fallback {
                    warn!(
                        "Safety net: empty/truncated response from {}, attempting fallback",
                        actual_route_ns.model
                    );
                    let fb_opt = fallback_route_within_provider(
                        &actual_route_ns.tier, &self.cfg, &actual_route_ns.provider,
                    )
                    .or_else(|| fallback_route(&actual_route_ns.tier, &self.cfg));
                    if let Some(fb_route) = fb_opt {
                        let fb_start = Instant::now();
                        match self.try_upstream(&upstream_body, &fb_route, client_auth.as_deref()).await {
                            (Ok(fb_resp), fb_route_ret, _) => {
                                match fb_resp.bytes().await {
                                    Ok(fb_bytes) => {
                                        info!("Safety net fallback to {} succeeded", fb_route_ret.model);
                                        resp_bytes = fb_bytes;
                                        actual_route_ns = fb_route_ret;
                                        fallback_used_ns = true;
                                        latency_ms_ns += fb_start.elapsed().as_millis() as u64;
                                    }
                                    Err(e) => {
                                        warn!("Safety net fallback body read failed: {e}");
                                    }
                                }
                            }
                            (Err(e), _, _) => {
                                warn!("Safety net fallback request failed: {e}");
                            }
                        }
                    }
                }
            }
            let actual_route = actual_route_ns;
            let fallback_used = fallback_used_ns;
            let latency_ms = latency_ms_ns;

            // Extract usage and compute cost
            let mut rec = CallRecord::from_request(
                &actual_route.model,
                &actual_route.provider,
                complexity.tier_name(),
                fallback_used,
                latency_ms,
            );
            rec.tenant_id = tenant_id.0.clone();

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

                self.metrics.add_tokens(prompt as u64, completion as u64);
                self.metrics.add_cost(rec.cost_usd);
            }

            let _ = self.store.record_call(&rec, &request_json);

            // Check webhook budget alerts
            self.check_webhook_budget().await;

            // Cache the response for Pro users (non-streaming only)
            // Always cache in OpenAI format — format conversion happens on retrieval.
            if let Some(ref hash) = cache_hash
                && let Ok(resp_str) = std::str::from_utf8(&resp_bytes)
            {
                self.store.cache_put(
                    hash,
                    resp_str,
                    &routed_model_id,
                    rec.prompt_tokens,
                    rec.completion_tokens,
                    self.cfg.cache.max_entries,
                );
            }

            // Convert to Anthropic format if the client expects it
            let final_bytes = if is_anthropic {
                if let Ok(openai_resp) =
                    serde_json::from_slice::<serde_json::Value>(&resp_bytes)
                {
                    let display_model = anthropic_model.as_deref().unwrap_or(&routed_model_id);
                    let anthropic_resp = openai_to_anthropic(&openai_resp, display_model);
                    Bytes::from(serde_json::to_vec(&anthropic_resp).unwrap_or_default())
                } else {
                    resp_bytes
                }
            } else {
                resp_bytes
            };

            let mut resp = Response::builder()
                .status(resp_status)
                .body(BoxBody::new(
                    Full::new(final_bytes).map_err(|e: Infallible| match e {}),
                ))
                .unwrap();
            Self::add_cors_headers(&mut resp);
            Ok(resp)
        }
    }

    /// Resolve the original client-requested model to a provider route.
    /// Used by Free tier (passthrough mode) — the provider/model are used for
    /// cost calculation and base_url resolution only. The actual Authorization
    /// header is forwarded from the client, so the route's api_key is unused.
    /// Falls back to first configured provider if model not found.
    fn resolve_original_route(&self, original_model: &str) -> crate::proxy::router::Route {
        // Try to find the model in any configured provider
        for provider in &self.cfg.providers {
            for model in &provider.models {
                if model.id == original_model {
                    return crate::proxy::router::Route {
                        provider: provider.name.clone(),
                        model: model.id.clone(),
                        base_url: provider.base_url.clone(),
                        api_key: String::new(), // not used — client auth forwarded
                        tier: model.tier.clone(),
                    };
                }
            }
        }
        // Model not found in config — use first available provider
        let p = self.cfg.providers.first().expect("No providers configured");
        let m = p.models.first().expect("No models configured for provider");
        debug!(
            "Model '{original_model}' not in config, routing via {}/{}",
            p.name, m.id
        );
        crate::proxy::router::Route {
            provider: p.name.clone(),
            model: m.id.clone(),
            base_url: p.base_url.clone(),
            api_key: String::new(),
            tier: m.tier.clone(),
        }
    }

    /// Try an upstream host. On failure, attempt fallback if configured.
    /// `client_auth` is the original Authorization header from the client.
    /// When set (Free tier passthrough), it is forwarded instead of using
    /// TokenWise's own keys — the client's key never reaches TokenWise config.
    async fn try_upstream(
        &self,
        body: &[u8],
        route: &crate::proxy::router::Route,
        client_auth: Option<&str>,
    ) -> (
        Result<reqwest::Response, String>,
        crate::proxy::router::Route,
        bool,
    ) {
        let url = format!("{}/chat/completions", route.base_url);

        // Auth priority:
        // 1. If route has no key (Free tier passthrough), forward client's auth header.
        // 2. If route has a key (Pro tier routing), use TokenWise's configured key.
        //    Pro rewrites models across providers, so the client's key wouldn't work.
        let effective_key = if route.api_key.is_empty() {
            client_auth.unwrap_or("")
        } else {
            &route.api_key
        };
        let auth_header = if effective_key.starts_with("Bearer ") {
            effective_key.to_string()
        } else if effective_key.is_empty() {
            String::new()
        } else {
            format!("Bearer {effective_key}")
        };

        let mut req_builder = self
            .client
            .post(&url)
            .header("Content-Type", "application/json");
        if !auth_header.is_empty() {
            req_builder = req_builder.header("Authorization", &auth_header);
        }
        let req = req_builder.body(body.to_vec());

        // Debug: log the actual request
        let body_preview = std::str::from_utf8(body)
            .unwrap_or("<binary>");
        debug!(
            "Upstream request: POST {url} | auth={auth_header} | body={body_preview}"
        );

        match req.send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    return (Ok(resp), route.clone(), false);
                }

                // Save status before draining body
                let status_code = resp.status();

                // Only fallback on 5xx (server errors), NOT 4xx (auth/bad request).
                // Fallback is a safety net for model failures, not for invalid API keys.
                if self.cfg.safety_net.enabled && status_code.is_server_error() {
                    warn!(
                        "Upstream {} returned {status_code}, attempting fallback...",
                        route.model
                    );

                    // Drain body to allow connection reuse
                    let _ = resp.bytes().await;

                    // Use provider-constrained fallback so the client's API key
                    // stays within its own provider (zero-trust compatible).
                    let fb_opt = fallback_route_within_provider(&route.tier, &self.cfg, &route.provider)
                        .or_else(|| {
                            // If no fallback within the same provider, try global
                            // fallback as last resort (works when TokenWise has its own keys).
                            debug!("No within-provider fallback for {}, trying global", route.provider);
                            fallback_route(&route.tier, &self.cfg)
                        });
                    if let Some(fb_route) = fb_opt {
                        let fb_url = format!("{}/chat/completions", fb_route.base_url);
                        let fb_effective = if fb_route.api_key.is_empty() {
                            client_auth.unwrap_or("")
                        } else {
                            &fb_route.api_key
                        };
                        let fb_auth_header = if fb_effective.starts_with("Bearer ") {
                            fb_effective.to_string()
                        } else if fb_effective.is_empty() {
                            String::new()
                        } else {
                            format!("Bearer {fb_effective}")
                        };
                        let mut fb_req_builder = self
                            .client
                            .post(&fb_url)
                            .header("Content-Type", "application/json");
                        if !fb_auth_header.is_empty() {
                            fb_req_builder =
                                fb_req_builder.header("Authorization", &fb_auth_header);
                        }
                        let fb_req = fb_req_builder.body(body.to_vec());

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

                // Use provider-constrained fallback first (zero-trust compatible),
                // then try global as last resort.
                let fb_opt2 = if self.cfg.safety_net.enabled {
                    fallback_route_within_provider(&route.tier, &self.cfg, &route.provider)
                        .or_else(|| fallback_route(&route.tier, &self.cfg))
                } else {
                    None
                };
                if let Some(fb_route) = fb_opt2 {
                    let fb_url = format!("{}/chat/completions", fb_route.base_url);
                    let fb_auth2 = client_auth.unwrap_or(&fb_route.api_key);
                    let fb_auth_header2 = if fb_auth2.starts_with("Bearer ") {
                        fb_auth2.to_string()
                    } else if fb_auth2.is_empty() {
                        String::new()
                    } else {
                        format!("Bearer {fb_auth2}")
                    };
                    let mut fb_req_builder2 = self
                        .client
                        .post(&fb_url)
                        .header("Content-Type", "application/json");
                    if !fb_auth_header2.is_empty() {
                        fb_req_builder2 =
                            fb_req_builder2.header("Authorization", &fb_auth_header2);
                    }
                    let fb_req = fb_req_builder2.body(body.to_vec());

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

    /// Check whether a non-streaming response body triggers safety-net fallback:
    /// - `fallback_on_empty_response`: content field is empty or missing
    /// - `fallback_on_truncated`: finish_reason is "length" (max_tokens reached)
    fn check_empty_or_truncated(&self, resp_bytes: &[u8]) -> bool {
        let Ok(json) = serde_json::from_slice::<serde_json::Value>(resp_bytes) else {
            return false;
        };
        if self.cfg.safety_net.fallback_on_empty_response {
            let content_empty = json
                .get("choices")
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.first())
                .and_then(|c| c.get("message"))
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .map(|s| s.trim().is_empty())
                .unwrap_or(true);
            if content_empty {
                return true;
            }
        }
        if self.cfg.safety_net.fallback_on_truncated {
            let truncated = json
                .get("choices")
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.first())
                .and_then(|c| c.get("finish_reason"))
                .and_then(|f| f.as_str())
                .map(|f| f == "length")
                .unwrap_or(false);
            if truncated {
                return true;
            }
        }
        false
    }

    /// Fire webhook alerts if budget thresholds are crossed.
    async fn check_webhook_budget(&self) {
        if let Some(ref webhook) = self.webhook {
            let now_ts = chrono::Utc::now().timestamp();

            let spent_today = if self.cfg.budget.daily_limit_usd > 0.0 {
                let today_start = chrono::Utc::now()
                    .date_naive()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
                    .timestamp();
                self.store.total_cost_since(today_start)
            } else {
                0.0
            };

            let spent_month = if self.cfg.budget.monthly_limit_usd > 0.0 {
                let month_start = chrono::Utc::now().format("%Y-%m-01").to_string();
                let month_start_ts = chrono::NaiveDate::parse_from_str(&month_start, "%Y-%m-%d")
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
                    .timestamp();
                self.store.total_cost_since(month_start_ts)
            } else {
                0.0
            };

            let mut dispatcher = webhook.lock().await;
            dispatcher
                .check_budget(
                    spent_today,
                    self.cfg.budget.daily_limit_usd,
                    spent_month,
                    self.cfg.budget.monthly_limit_usd,
                    now_ts,
                )
                .await;
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
