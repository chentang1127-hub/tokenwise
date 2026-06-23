//! Integration tests for TokenWise Core.
//!
//! Tests admin API handlers using axum's built-in test utilities
//! (no real TCP — instant, reliable, parallel-safe).
//!
//! Run with: `cargo test --test integration_tests`

use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::sync::Arc;
use tower::util::ServiceExt;

// ── Helpers ──────────────────────────────────────────────

fn test_config() -> tokenwise::config::Config {
    tokenwise::config::Config {
        locale: "en".to_string(),
        headless: true,
        proxy: tokenwise::config::ProxyConfig {
            listen: "127.0.0.1:9401".to_string(),
            admin: "127.0.0.1:9400".to_string(),
            timeout_secs: 10,
            bypass: false,
        },
        providers: vec![tokenwise::config::ProviderConfig {
            name: "deepseek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            api_key_env: "DEEPSEEK_API_KEY".into(),
            models: vec![
                tokenwise::config::ModelConfig {
                    id: "deepseek-chat".into(),
                    tier: "cheap".into(),
                    cost_per_1k_prompt: 0.00027,
                    cost_per_1k_completion: 0.0011,
                },
                tokenwise::config::ModelConfig {
                    id: "deepseek-reasoner".into(),
                    tier: "premium".into(),
                    cost_per_1k_prompt: 0.00055,
                    cost_per_1k_completion: 0.00219,
                },
            ],
        }],
        routing: tokenwise::config::RoutingConfig {
            simple_max_tokens: 300,
            complex_min_tokens: 1500,
            simple_keywords: vec!["summarize".into(), "translate".into()],
            complex_keywords: vec!["debug".into(), "implement".into()],
            tier_simple: "cheap".into(),
            tier_complex: "premium".into(),
            tier_default: "mid".into(),
        },
        safety_net: tokenwise::config::SafetyNetConfig {
            enabled: false,
            max_fallback_retries: 1,
            fallback_map: {
                let mut m = std::collections::HashMap::new();
                m.insert("cheap".to_string(), "mid".to_string());
                m
            },
            fallback_on_empty_response: true,
            fallback_on_truncated: true,
        },
        license: tokenwise::config::LicenseConfig { key: String::new() },
        storage: tokenwise::config::StorageConfig {
            db_path: ":memory:".to_string(),
            retention_days: 90,
        },
        cache: Default::default(),
        budget: tokenwise::config::BudgetConfig {
            daily_limit_usd: 0.0,
            monthly_limit_usd: 0.0,
        },
        webhook: Default::default(),
    }
}

fn build_test_app() -> axum::Router {
    let _ = std::fs::write(".tokenwise_setup_done", "");
    let cfg = test_config();
    let store = Arc::new(
        tokenwise::recording::Store::new(":memory:").expect("Failed to create in-memory store"),
    );
    let state = Arc::new(tokenwise::admin::AppState {
        config: Arc::new(cfg),
        store,
        routing_enabled: false,
        config_path: "config.yaml".to_string(),
        metrics: Arc::new(tokenwise::admin::Metrics::default()),
        start_time: std::time::Instant::now(),
    });
    tokenwise::admin::build_router(state)
}

async fn body_string(body: axum::body::Body) -> String {
    let bytes = axum::body::to_bytes(body, 1024 * 1024).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

// ── Admin Endpoint Tests ─────────────────────────────────

#[tokio::test]
async fn test_health_endpoint() {
    let app = build_test_app();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp.into_body()).await;
    assert!(body.contains("\"status\":\"ok\"") || body.contains("\"status\":\"degraded\""));
    assert!(body.contains("\"version\""));
    assert!(body.contains("\"uptime_seconds\""));
    assert!(body.contains("\"routing_enabled\""));
}

#[tokio::test]
async fn test_dashboard_returns_html() {
    let app = build_test_app();
    let resp = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let content_type = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(content_type.contains("text/html"));
    let body = body_string(resp.into_body()).await;
    assert!(body.contains("TokenWise Core"));
}

#[tokio::test]
async fn test_dashboard_cn_locale() {
    let app = build_test_app();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/?lang=zh")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp.into_body()).await;
    assert!(body.contains("仪表板") || body.contains("执行层"));
}

#[tokio::test]
async fn test_demo_chat_endpoint() {
    let app = build_test_app();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/demo")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"message":"Hello, TokenWise!"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp.into_body()).await;
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(json.get("choices").is_some());
    assert!(json.get("usage").is_some());
    assert_eq!(json["usage"]["cost_usd"].as_f64().unwrap(), 0.0);
}

#[tokio::test]
async fn test_demo_chat_empty_message() {
    let app = build_test_app();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/demo")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"message":""}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp.into_body()).await;
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(json.get("error").is_some());
}

#[tokio::test]
async fn test_token_distribution_api() {
    let app = build_test_app();
    // Record a demo call first
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/demo")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"message":"test"}"#))
                .unwrap(),
        )
        .await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/token-distribution")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp.into_body()).await;
    let json: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
    assert!(!json.is_empty());
    let first = &json[0];
    assert!(first.get("model").is_some());
    assert!(first.get("call_count").is_some());
    assert!(first.get("prompt_tokens").is_some());
    assert!(first.get("completion_tokens").is_some());
    assert!(first.get("total_cost").is_some());
}

#[tokio::test]
async fn test_budget_status_api() {
    let app = build_test_app();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/budget-status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp.into_body()).await;
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(json.get("daily").is_some());
    assert!(json.get("monthly").is_some());
}

#[tokio::test]
async fn test_calls_page() {
    let app = build_test_app();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/calls")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp.into_body()).await;
    assert!(body.contains("Calls") || body.contains("调用记录"));
}

#[tokio::test]
async fn test_calls_page_with_filters() {
    let app = build_test_app();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/calls?range=24h&complexity=simple&decision=direct&page=1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_savings_page() {
    let app = build_test_app();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/savings")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp.into_body()).await;
    assert!(body.contains("Savings") || body.contains("节省"));
}

#[tokio::test]
async fn test_metrics_endpoint() {
    let app = build_test_app();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp.into_body()).await;
    assert!(body.contains("tokenwise_requests_total"));
    assert!(body.contains("tokenwise_cache_hits_total"));
    assert!(body.contains("tokenwise_cost_usd_total"));
    assert!(body.contains("tokenwise_cache_hit_ratio"));
}

#[tokio::test]
async fn test_setup_page() {
    let app = build_test_app();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/setup")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp.into_body()).await;
    assert!(body.contains("TokenWise") || body.contains("Setup") || body.contains("设置"));
}

#[tokio::test]
async fn test_404_fallback_returns_html() {
    let app = build_test_app();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/no-such-page")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let content_type = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(content_type.contains("text/html"));
    let body = body_string(resp.into_body()).await;
    assert!(body.contains("404"));
    assert!(body.contains("TokenWise Core"));
}

#[tokio::test]
async fn test_404_fallback_cn_locale() {
    let app = build_test_app();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/no-such-page?lang=zh")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_string(resp.into_body()).await;
    assert!(body.contains("页面未找到"));
    assert!(body.contains("TokenWise Core"));
}

#[tokio::test]
async fn test_api_test_webhook_no_url() {
    let app = build_test_app();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/test-webhook")
                .method("POST")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp.into_body()).await;
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["ok"], false);
    assert!(json["error"].as_str().unwrap().contains("webhook"));
}
