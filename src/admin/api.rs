//! REST API + HTMX dashboard endpoints.
//! Supports EN and ZH locales — switched via config.locale or ?lang= query param.

use std::{collections::HashMap, sync::Arc};

use super::{AppState, chat_widget};
use askama::Template;
use axum::{
    Router,
    extract::{Json, Query, State},
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
};
use serde::Deserialize;

pub fn make_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(dashboard))
        .route("/calls", get(calls_page))
        .route("/savings", get(savings_page))
        .route("/setup", get(setup_page).post(setup_save))
        .route("/api/demo", post(demo_chat))
        .route("/api/token-distribution", get(token_distribution))
        .route("/api/budget-status", get(budget_status))
        .route("/api/test-webhook", post(test_webhook))
        .route("/api/export/calls", get(export_calls))
        .route("/api/export/savings", get(export_savings))
        .route(
            "/api/webhook-config",
            get(get_webhook_config).post(save_webhook_config),
        )
        .route("/api/settings/routing", post(save_routing))
        .route("/api/budget-banner", get(budget_banner))
        .route("/settings", get(settings_page))
        .route("/metrics", get(super::metrics::metrics_handler))
        .route("/health", get(health))
        .fallback(fallback_404)
        .with_state(state)
}

// ── English Templates ──────────────────────────────────

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    total_calls: i64,
    month_cost: String,
    savings_pct: String,
    recent_calls: Vec<CallRow>,
    lang_toggle_label: &'static str,
    lang_toggle_url: &'static str,
    is_pro: bool,
    cache_count: i64,
    cache_saved: String,
    routing_count: i64,
    routing_saved: String,
    distinct_models: i64,
    chat_widget: String,
    version: &'static str,
    /// Token distribution data for inline SVG chart (model name → cost).
    chart_models: String,
    chart_costs: String,
    chart_max_cost: f64,
}

#[derive(Template)]
#[template(path = "calls.html")]
struct CallsTemplate {
    calls: Vec<CallRow>,
    stats: CallsPageStats,
    filters: CallsFilters,
    lang_toggle_label: &'static str,
    lang_toggle_url: &'static str,
    version: &'static str,
}

#[derive(Template)]
#[template(path = "savings.html")]
struct SavingsTemplate {
    month_cost: String,
    total_calls: i64,
    cache_count: i64,
    cache_saved: String,
    routing_count: i64,
    routing_saved: String,
    total_saved: String,
    lang_toggle_label: &'static str,
    lang_toggle_url: &'static str,
    is_pro: bool,
    version: &'static str,
}

// ── Chinese Templates ──────────────────────────────────

#[derive(Template)]
#[template(path = "cn/dashboard.html")]
struct DashboardTemplateCn {
    total_calls: i64,
    month_cost: String,
    savings_pct: String,
    recent_calls: Vec<CallRow>,
    lang_toggle_label: &'static str,
    lang_toggle_url: &'static str,
    is_pro: bool,
    cache_count: i64,
    cache_saved: String,
    routing_count: i64,
    routing_saved: String,
    distinct_models: i64,
    chat_widget: String,
    version: &'static str,
    /// Token distribution data for inline SVG chart.
    chart_models: String,
    chart_costs: String,
    chart_max_cost: f64,
}

#[derive(Template)]
#[template(path = "cn/calls.html")]
struct CallsTemplateCn {
    calls: Vec<CallRow>,
    stats: CallsPageStats,
    filters: CallsFilters,
    lang_toggle_label: &'static str,
    lang_toggle_url: &'static str,
    version: &'static str,
}

#[derive(Template)]
#[template(path = "cn/savings.html")]
struct SavingsTemplateCn {
    month_cost: String,
    total_calls: i64,
    cache_count: i64,
    cache_saved: String,
    routing_count: i64,
    routing_saved: String,
    total_saved: String,
    lang_toggle_label: &'static str,
    lang_toggle_url: &'static str,
    is_pro: bool,
    version: &'static str,
}

// ── Setup Wizard Templates ────────────────────────────

#[derive(Template)]
#[template(path = "setup.html")]
struct SetupTemplate {
    step: u8,
    lang_toggle_label: &'static str,
    lang_toggle_url: &'static str,
    proxy_url: String,
    chat_widget: String,
    version: &'static str,
}

#[derive(Template)]
#[template(path = "cn/setup.html")]
struct SetupTemplateCn {
    step: u8,
    lang_toggle_label: &'static str,
    lang_toggle_url: &'static str,
    proxy_url: String,
    chat_widget: String,
    version: &'static str,
}

// ── Settings Templates ────────────────────────────────

#[derive(Template)]
#[template(path = "settings.html")]
struct SettingsTemplate {
    simple_keywords: String,
    complex_keywords: String,
    webhook_url: String,
    webhook_warning_pct: String,
    webhook_cooldown_secs: String,
    webhook_anomaly: bool,
    tg_bot_token: String,
    tg_chat_id: String,
    lang_toggle_label: &'static str,
    lang_toggle_url: &'static str,
    version: &'static str,
}

#[derive(Template)]
#[template(path = "cn/settings.html")]
struct SettingsTemplateCn {
    simple_keywords: String,
    complex_keywords: String,
    webhook_url: String,
    webhook_warning_pct: String,
    webhook_cooldown_secs: String,
    webhook_anomaly: bool,
    tg_bot_token: String,
    tg_chat_id: String,
    lang_toggle_label: &'static str,
    lang_toggle_url: &'static str,
    version: &'static str,
}

// ── Error Templates ─────────────────────────────────

#[derive(Template)]
#[template(path = "404.html")]
struct Error404Template {
    version: &'static str,
}

#[derive(Template)]
#[template(path = "500.html")]
#[allow(dead_code)]
struct Error500Template {
    version: &'static str,
}

#[derive(Template)]
#[template(path = "429.html")]
#[allow(dead_code)]
struct Error429Template {
    version: &'static str,
}

#[derive(Template)]
#[template(path = "cn/404.html")]
struct Error404TemplateCn {
    version: &'static str,
}

#[derive(Template)]
#[template(path = "cn/500.html")]
#[allow(dead_code)]
struct Error500TemplateCn {
    version: &'static str,
}

#[derive(Template)]
#[template(path = "cn/429.html")]
#[allow(dead_code)]
struct Error429TemplateCn {
    version: &'static str,
}

// ── Shared row types ───────────────────────────────────

struct CallRow {
    model: String,
    complexity: String,
    decision: String,
    decision_label: String,
    latency: String,
    cost: String,
    ago: String,
    prompt_tokens: String,
    completion_tokens: String,
}

struct CallsPageStats {
    total_calls: i64,
    total_prompt_tokens: String,
    total_completion_tokens: String,
    total_cost: String,
    eliminated_count: i64,
    routed_count: i64,
}

struct CallsFilters {
    range: String,      // current range value
    complexity: String, // current complexity filter
    decision: String,   // current decision filter
    page: usize,        // current page (1-based)
    total_pages: usize, // total pages
    has_prev: bool,
    has_next: bool,
    range_label: String, // human-readable label for current range
}

/// Whether the configured locale is Chinese (from config, fallback).
fn is_cn(state: &Arc<AppState>) -> bool {
    state.config.locale == "zh" || state.config.locale == "cn"
}

/// Resolve effective locale from query param → config → default.
/// Returns (is_chinese, toggle_label, toggle_url).
fn resolve_lang(
    params: &HashMap<String, String>,
    state: &Arc<AppState>,
) -> (bool, &'static str, &'static str) {
    let use_cn = match params.get("lang") {
        Some(l) if l == "zh" || l == "cn" => true,
        Some(l) if l == "en" => false,
        _ => is_cn(state),
    };
    if use_cn {
        (true, "EN", "?lang=en")
    } else {
        (false, "中文", "?lang=zh")
    }
}

// ── Handlers ───────────────────────────────────────────

async fn dashboard(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> axum::response::Response {
    // First-run check: redirect to setup if no keys configured
    if state.config.is_first_run() {
        let lang_param = params
            .get("lang")
            .map(|l| format!("?lang={l}"))
            .unwrap_or_default();
        return Redirect::to(&format!("/setup{lang_param}")).into_response();
    }

    let tenant_id = params.get("tenant").map(|s| s.as_str());

    let stats = state.store.monthly_stats(tenant_id).unwrap_or_default();
    let is_pro = state.routing_enabled;
    let proxy_url = format!("http://{}/v1", state.config.proxy.listen);
    let demo_url = format!("http://{}/api/demo", state.config.proxy.admin);

    // Real savings: cache hits avoid API calls; routing picks cheaper models.
    let cache_stats = if is_pro {
        state.store.cache_stats()
    } else {
        crate::recording::store::CacheStats {
            total_entries: 0,
            total_hits: 0,
        }
    };
    let cache_count = cache_stats
        .total_hits
        .saturating_sub(cache_stats.total_entries)
        .max(0);
    let cache_saved = crate::cost::calculator::format_usd(state.store.cache_savings_estimate());
    let routing_count = if is_pro {
        state.store.routing_count(tenant_id)
    } else {
        0
    };
    // Conservative routing savings estimate: each routed call saves ~30% vs
    // the most-expensive model. Real number is the sum of per-call deltas.
    let routing_saved_est = if routing_count > 0 {
        // avg_cost × 0.3 × routing_count as a rough, honest estimate
        let avg = if stats.total_calls > 0 {
            stats.total_cost / stats.total_calls as f64
        } else {
            0.0
        };
        avg * 0.3 * routing_count as f64
    } else {
        0.0
    };
    let routing_saved = crate::cost::calculator::format_usd(routing_saved_est);
    let distinct_models = state.store.distinct_models(tenant_id);

    // Real savings percentage: (cache_saved + routing_saved) / (actual_cost + savings) × 100
    let cache_saved_val = state.store.cache_savings_estimate();
    let routing_saved_val = routing_saved_est;
    let total_saved_val = cache_saved_val + routing_saved_val;
    let savings_pct = if stats.total_cost > 0.0 && total_saved_val > 0.0 {
        let denom = stats.total_cost + total_saved_val;
        let pct = (total_saved_val / denom) * 100.0;
        format!("{:.0}%", pct)
    } else {
        "N/A".to_string()
    };

    let total_calls = stats.total_calls;
    let month_cost = crate::cost::calculator::format_usd(stats.total_cost);

    // Build chart data from token distribution
    let dist = state
        .store
        .token_distribution(tenant_id)
        .unwrap_or_default();
    let chart_models_json =
        serde_json::to_string(&dist.iter().map(|m| &m.model).collect::<Vec<_>>())
            .unwrap_or_default();
    let chart_costs_json =
        serde_json::to_string(&dist.iter().map(|m| m.total_cost).collect::<Vec<_>>())
            .unwrap_or_default();
    let chart_max_cost = dist.iter().map(|m| m.total_cost).fold(0.0f64, f64::max);

    let (use_cn, lang_toggle_label, lang_toggle_url) = resolve_lang(&params, &state);

    // Recent request log (last 8 calls, terminal-log style)
    let now = chrono::Utc::now().timestamp();
    let calls = state.store.recent_calls(8, tenant_id).unwrap_or_default();
    let recent_calls: Vec<CallRow> = calls
        .into_iter()
        .map(|c| {
            let (decision, decision_label) =
                if c.provider == "demo" || (c.latency_ms < 10 && c.cost_usd == 0.0) {
                    ("eliminated", if use_cn { "已消除" } else { "eliminated" })
                } else if c.was_routed {
                    (
                        "routed",
                        if use_cn {
                            "已路由 → 更优"
                        } else {
                            "routed → cheaper"
                        },
                    )
                } else {
                    ("direct", if use_cn { "直连" } else { "direct" })
                };
            let ago_secs = (now - c.timestamp).max(0);
            let ago = if ago_secs < 60 {
                if use_cn { "刚刚" } else { "just now" }.to_string()
            } else if ago_secs < 3600 {
                format!("{}min ago", ago_secs / 60)
            } else if ago_secs < 86400 {
                format!("{}h ago", ago_secs / 3600)
            } else {
                format!("{}d ago", ago_secs / 86400)
            };
            CallRow {
                model: format!("{}/{}", c.provider, c.model),
                complexity: c.complexity,
                decision: decision.to_string(),
                decision_label: decision_label.to_string(),
                latency: format!("{}ms", c.latency_ms),
                cost: crate::cost::calculator::format_usd(c.cost_usd),
                ago,
                prompt_tokens: format_num(c.prompt_tokens as i64),
                completion_tokens: format_num(c.completion_tokens as i64),
            }
        })
        .collect();

    if use_cn {
        let template = DashboardTemplateCn {
            total_calls,
            month_cost,
            savings_pct,
            recent_calls,
            lang_toggle_label,
            lang_toggle_url,
            is_pro,
            cache_count,
            cache_saved,
            routing_count,
            routing_saved,
            distinct_models,
            chat_widget: chat_widget::render(&proxy_url, &demo_url),
            version: env!("CARGO_PKG_VERSION"),
            chart_models: chart_models_json,
            chart_costs: chart_costs_json,
            chart_max_cost,
        };
        Html(
            template
                .render()
                .unwrap_or_else(|e| format!("Template error: {e}")),
        )
        .into_response()
    } else {
        let template = DashboardTemplate {
            total_calls,
            month_cost,
            savings_pct,
            recent_calls,
            lang_toggle_label,
            lang_toggle_url,
            is_pro,
            cache_count,
            cache_saved,
            routing_count,
            routing_saved,
            distinct_models,
            chat_widget: chat_widget::render(&proxy_url, &demo_url),
            version: env!("CARGO_PKG_VERSION"),
            chart_models: chart_models_json,
            chart_costs: chart_costs_json,
            chart_max_cost,
        };
        Html(
            template
                .render()
                .unwrap_or_else(|e| format!("Template error: {e}")),
        )
        .into_response()
    }
}

async fn calls_page(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Html<String> {
    let (use_cn, lang_toggle_label, lang_toggle_url) = resolve_lang(&params, &state);

    // Parse filter params
    let range = params.get("range").map(|s| s.as_str()).unwrap_or("all");
    let complexity = params
        .get("complexity")
        .map(|s| s.as_str())
        .unwrap_or("all");
    let decision = params.get("decision").map(|s| s.as_str()).unwrap_or("all");
    let page: usize = params
        .get("page")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1)
        .max(1);

    let range_hours = match range {
        "24h" => Some(24u32),
        "7d" => Some(168u32),
        "30d" => Some(720u32),
        _ => None,
    };
    let range_label = match range {
        "24h" => {
            if use_cn {
                "24 小时"
            } else {
                "24 Hours"
            }
        }
        "7d" => {
            if use_cn {
                "7 天"
            } else {
                "7 Days"
            }
        }
        "30d" => {
            if use_cn {
                "30 天"
            } else {
                "30 Days"
            }
        }
        _ => {
            if use_cn {
                "全部"
            } else {
                "All Time"
            }
        }
    };

    let complexity_filter = if complexity != "all" {
        Some(complexity)
    } else {
        None
    };
    let decision_filter = if decision != "all" {
        Some(decision)
    } else {
        None
    };
    let tenant_id = params.get("tenant").map(|s| s.as_str());

    let per_page: usize = 50;
    let offset = (page - 1) * per_page;

    // Get filtered calls
    let calls = state
        .store
        .recent_calls_filtered(
            per_page,
            offset,
            range_hours,
            complexity_filter,
            decision_filter,
            tenant_id,
        )
        .unwrap_or_default();

    let total_count = state
        .store
        .calls_count_filtered(range_hours, complexity_filter, decision_filter, tenant_id)
        .unwrap_or(0);
    let total_pages = ((total_count as f64) / (per_page as f64)).ceil() as usize;
    let total_pages = total_pages.max(1);

    // Get summary stats for the filter bar
    let summary = state.store.calls_summary(range_hours, tenant_id).unwrap_or(
        crate::recording::store::CallsSummary {
            total: 0,
            total_prompt_tokens: 0,
            total_completion_tokens: 0,
            total_cost: 0.0,
            avg_latency_ms: 0.0,
            eliminated_count: 0,
            routed_count: 0,
        },
    );

    let stats = CallsPageStats {
        total_calls: summary.total,
        total_prompt_tokens: format_tokens(summary.total_prompt_tokens),
        total_completion_tokens: format_tokens(summary.total_completion_tokens),
        total_cost: crate::cost::calculator::format_usd(summary.total_cost),
        eliminated_count: summary.eliminated_count,
        routed_count: summary.routed_count,
    };

    let filters = CallsFilters {
        range: range.to_string(),
        complexity: complexity.to_string(),
        decision: decision.to_string(),
        page,
        total_pages,
        has_prev: page > 1,
        has_next: page < total_pages,
        range_label: range_label.to_string(),
    };

    let now = chrono::Utc::now().timestamp();
    let call_rows: Vec<CallRow> = calls
        .into_iter()
        .map(|c| {
            let (d, dl) = if c.provider == "demo" || (c.cost_usd == 0.0 && c.latency_ms < 10) {
                ("eliminated", if use_cn { "已消除" } else { "eliminated" })
            } else if c.was_routed {
                (
                    "routed",
                    if use_cn {
                        "已路由 → 更优"
                    } else {
                        "routed → cheaper"
                    },
                )
            } else {
                ("direct", if use_cn { "直连" } else { "direct" })
            };
            let ago_secs = (now - c.timestamp).max(0);
            let ago = if ago_secs < 60 {
                if use_cn { "刚刚" } else { "just now" }.to_string()
            } else if ago_secs < 3600 {
                if use_cn {
                    format!("{}分钟前", ago_secs / 60)
                } else {
                    format!("{}min ago", ago_secs / 60)
                }
            } else if ago_secs < 86400 {
                if use_cn {
                    format!("{}小时前", ago_secs / 3600)
                } else {
                    format!("{}h ago", ago_secs / 3600)
                }
            } else {
                if use_cn {
                    format!("{}天前", ago_secs / 86400)
                } else {
                    format!("{}d ago", ago_secs / 86400)
                }
            };
            CallRow {
                model: format!("{}/{}", c.provider, c.model),
                complexity: c.complexity,
                decision: d.to_string(),
                decision_label: dl.to_string(),
                latency: format!("{}ms", c.latency_ms),
                cost: crate::cost::calculator::format_usd(c.cost_usd),
                ago,
                prompt_tokens: format_num(c.prompt_tokens as i64),
                completion_tokens: format_num(c.completion_tokens as i64),
            }
        })
        .collect();

    if use_cn {
        let template = CallsTemplateCn {
            calls: call_rows,
            stats,
            filters,
            lang_toggle_label,
            lang_toggle_url,
            version: env!("CARGO_PKG_VERSION"),
        };
        Html(
            template
                .render()
                .unwrap_or_else(|e| format!("Template error: {e}")),
        )
    } else {
        let template = CallsTemplate {
            calls: call_rows,
            stats,
            filters,
            lang_toggle_label,
            lang_toggle_url,
            version: env!("CARGO_PKG_VERSION"),
        };
        Html(
            template
                .render()
                .unwrap_or_else(|e| format!("Template error: {e}")),
        )
    }
}

/// Format a number with K/M suffixes for readability.
fn format_num(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Format token count with K/M/B suffixes.
fn format_tokens(n: i64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}B", n as f64 / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

async fn savings_page(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Html<String> {
    let tenant_id = params.get("tenant").map(|s| s.as_str());

    let stats = state.store.monthly_stats(tenant_id).unwrap_or_default();
    let is_pro = state.routing_enabled;

    // Real savings from cache + routing
    let cache_count = state
        .store
        .cache_stats()
        .total_hits
        .saturating_sub(state.store.cache_stats().total_entries)
        .max(0);
    let cache_saved = crate::cost::calculator::format_usd(state.store.cache_savings_estimate());
    let routing_count = if is_pro {
        state.store.routing_count(tenant_id)
    } else {
        0
    };
    let routing_saved = {
        let avg = if stats.total_calls > 0 {
            stats.total_cost / stats.total_calls as f64
        } else {
            0.0
        };
        let est = if routing_count > 0 {
            avg * 0.3 * routing_count as f64
        } else {
            0.0
        };
        crate::cost::calculator::format_usd(est)
    };
    let total_saved_est = state.store.cache_savings_estimate()
        + if routing_count > 0 {
            let avg = if stats.total_calls > 0 {
                stats.total_cost / stats.total_calls as f64
            } else {
                0.0
            };
            avg * 0.3 * routing_count as f64
        } else {
            0.0
        };
    let total_saved = crate::cost::calculator::format_usd(total_saved_est);

    let month_cost = crate::cost::calculator::format_usd(stats.total_cost);

    let (use_cn, lang_toggle_label, lang_toggle_url) = resolve_lang(&params, &state);

    if use_cn {
        let template = SavingsTemplateCn {
            month_cost,
            total_calls: stats.total_calls,
            cache_count,
            cache_saved,
            routing_count,
            routing_saved,
            total_saved,
            lang_toggle_label,
            lang_toggle_url,
            is_pro,
            version: env!("CARGO_PKG_VERSION"),
        };
        Html(
            template
                .render()
                .unwrap_or_else(|e| format!("Template error: {e}")),
        )
    } else {
        let template = SavingsTemplate {
            month_cost,
            total_calls: stats.total_calls,
            cache_count,
            cache_saved,
            routing_count,
            routing_saved,
            total_saved,
            lang_toggle_label,
            lang_toggle_url,
            is_pro,
            version: env!("CARGO_PKG_VERSION"),
        };
        Html(
            template
                .render()
                .unwrap_or_else(|e| format!("Template error: {e}")),
        )
    }
}

/// GET /api/token-distribution — returns token usage by model for charts.
async fn token_distribution(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Json<Vec<serde_json::Value>> {
    let tenant_id = params.get("tenant").map(|s| s.as_str());
    let dist = state
        .store
        .token_distribution(tenant_id)
        .unwrap_or_default();
    let data: Vec<serde_json::Value> = dist
        .into_iter()
        .map(|m| {
            serde_json::json!({
                "model": m.model,
                "call_count": m.call_count,
                "prompt_tokens": m.prompt_tokens,
                "completion_tokens": m.completion_tokens,
                "total_cost": m.total_cost,
            })
        })
        .collect();
    Json(data)
}

/// GET /api/budget-status — returns current spending vs budget caps.
async fn budget_status(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let now = chrono::Utc::now();
    let today_start = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp();
    let month_start = now.format("%Y-%m-01").to_string();
    let month_start_ts = chrono::NaiveDate::parse_from_str(&month_start, "%Y-%m-%d")
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp();

    let spent_today = state.store.total_cost_since(today_start);
    let spent_month = state.store.total_cost_since(month_start_ts);

    Json(serde_json::json!({
        "daily": {
            "spent": spent_today,
            "limit": state.config.budget.daily_limit_usd,
            "pct": if state.config.budget.daily_limit_usd > 0.0 {
                (spent_today / state.config.budget.daily_limit_usd * 100.0).min(100.0)
            } else { 0.0 }
        },
        "monthly": {
            "spent": spent_month,
            "limit": state.config.budget.monthly_limit_usd,
            "pct": if state.config.budget.monthly_limit_usd > 0.0 {
                (spent_month / state.config.budget.monthly_limit_usd * 100.0).min(100.0)
            } else { 0.0 }
        }
    }))
}

async fn health(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let db_ok = state.store.health_check().is_ok();
    let uptime = state.start_time.elapsed();
    Json(serde_json::json!({
        "status": if db_ok { "ok" } else { "degraded" },
        "version": env!("CARGO_PKG_VERSION"),
        "db": if db_ok { "connected" } else { "error" },
        "uptime_seconds": uptime.as_secs(),
        "routing_enabled": state.routing_enabled,
    }))
}

/// POST /api/test-webhook — send a test notification to the configured webhook URL.
async fn test_webhook(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    if state.config.webhook.url.is_empty() {
        return Json(serde_json::json!({
            "ok": false,
            "error": "No webhook URL configured. Set webhook.url in config.yaml."
        }));
    }

    // Verify dispatcher creation is possible
    if crate::webhooks::WebhookDispatcher::new(state.config.webhook.clone()).is_none() {
        return Json(serde_json::json!({
            "ok": false,
            "error": "Failed to create webhook dispatcher."
        }));
    }

    let event = crate::webhooks::WebhookEvent::BudgetWarning {
        scope: "test".into(),
        spent: 0.0,
        limit: 1.0,
        pct: 0.0,
    };
    let url = state.config.webhook.url.clone();
    let client = reqwest::Client::new();
    let payload = serde_json::to_vec(&event).unwrap_or_default();
    match client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("User-Agent", "TokenWise-Webhook/1.0")
        .body(payload)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            Json(serde_json::json!({
                "ok": status.is_success(),
                "status_code": status.as_u16(),
                "url": url,
            }))
        }
        Err(e) => Json(serde_json::json!({
            "ok": false,
            "error": format!("{}", e),
            "url": url,
        })),
    }
}

// ── Setup Wizard Handlers ──────────────────────────────

/// GET /setup — show the setup wizard.
/// Creates the `.tokenwise_setup_done` marker so the dashboard won't
/// redirect back here (breaking the first-run loop in zero-trust mode).
async fn setup_page(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Html<String> {
    // Mark setup as seen — prevents the dashboard → setup redirect loop
    // in zero-trust mode where no API key env vars are ever set.
    let _ = std::fs::write(".tokenwise_setup_done", "");

    let (use_cn, lang_toggle_label, lang_toggle_url) = resolve_lang(&params, &state);
    let proxy_url = format!("http://{}/v1", state.config.proxy.listen);
    let demo_url = format!("http://{}/api/demo", state.config.proxy.admin);

    let chat_html = chat_widget::render(&proxy_url, &demo_url);

    if use_cn {
        let template = SetupTemplateCn {
            step: 1,
            lang_toggle_label,
            lang_toggle_url,
            proxy_url,
            chat_widget: chat_html,
            version: env!("CARGO_PKG_VERSION"),
        };
        Html(
            template
                .render()
                .unwrap_or_else(|e| format!("Template error: {e}")),
        )
    } else {
        let template = SetupTemplate {
            step: 1,
            lang_toggle_label,
            lang_toggle_url,
            proxy_url,
            chat_widget: chat_html,
            version: env!("CARGO_PKG_VERSION"),
        };
        Html(
            template
                .render()
                .unwrap_or_else(|e| format!("Template error: {e}")),
        )
    }
}

/// POST /setup/save — transition from step 1 to step 2 (done).
/// No API keys needed — TokenWise forwards the client's own Authorization header.
async fn setup_save(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
    Json(_body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let (use_cn, lang_toggle_label, lang_toggle_url) = resolve_lang(&params, &state);
    let proxy_url = format!("http://{}/v1", state.config.proxy.listen);
    let demo_url = format!("http://{}/api/demo", state.config.proxy.admin);
    let chat_html = chat_widget::render(&proxy_url, &demo_url);

    if use_cn {
        let t = SetupTemplateCn {
            step: 2,
            lang_toggle_label,
            lang_toggle_url,
            proxy_url,
            chat_widget: chat_html,
            version: env!("CARGO_PKG_VERSION"),
        };
        Html(
            t.render()
                .unwrap_or_else(|e| format!("Template error: {e}")),
        )
        .into_response()
    } else {
        let t = SetupTemplate {
            step: 2,
            lang_toggle_label,
            lang_toggle_url,
            proxy_url,
            chat_widget: chat_html,
            version: env!("CARGO_PKG_VERSION"),
        };
        Html(
            t.render()
                .unwrap_or_else(|e| format!("Template error: {e}")),
        )
        .into_response()
    }
}

// ── Settings Handlers ──────────────────────────────────

/// GET /settings — edit routing keywords + webhook config.
async fn settings_page(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Html<String> {
    let (use_cn, lang_toggle_label, lang_toggle_url) = resolve_lang(&params, &state);

    let simple_keywords = state.config.routing.simple_keywords.join("\n");
    let complex_keywords = state.config.routing.complex_keywords.join("\n");
    let webhook_warning_pct = format!("{:.0}", state.config.webhook.budget_warning_pct * 100.0);
    let webhook_cooldown_secs = state.config.webhook.cooldown_secs.to_string();

    if use_cn {
        let t = SettingsTemplateCn {
            simple_keywords,
            complex_keywords,
            webhook_url: state.config.webhook.url.clone(),
            webhook_warning_pct,
            webhook_cooldown_secs,
            webhook_anomaly: state.config.webhook.anomaly_detection,
            tg_bot_token: state.config.webhook.tg_bot_token.clone(),
            tg_chat_id: state.config.webhook.tg_chat_id.clone(),
            lang_toggle_label,
            lang_toggle_url,
            version: env!("CARGO_PKG_VERSION"),
        };
        Html(
            t.render()
                .unwrap_or_else(|e| format!("Template error: {e}")),
        )
    } else {
        let t = SettingsTemplate {
            simple_keywords,
            complex_keywords,
            webhook_url: state.config.webhook.url.clone(),
            webhook_warning_pct,
            webhook_cooldown_secs,
            webhook_anomaly: state.config.webhook.anomaly_detection,
            tg_bot_token: state.config.webhook.tg_bot_token.clone(),
            tg_chat_id: state.config.webhook.tg_chat_id.clone(),
            lang_toggle_label,
            lang_toggle_url,
            version: env!("CARGO_PKG_VERSION"),
        };
        Html(
            t.render()
                .unwrap_or_else(|e| format!("Template error: {e}")),
        )
    }
}

#[derive(Deserialize)]
struct RoutingSavePayload {
    simple_keywords: Vec<String>,
    complex_keywords: Vec<String>,
}

/// POST /api/settings/routing — save routing keywords to config.yaml.
async fn save_routing(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RoutingSavePayload>,
) -> Json<serde_json::Value> {
    let mut cfg = (*state.config).clone();
    cfg.routing.simple_keywords = body.simple_keywords;
    cfg.routing.complex_keywords = body.complex_keywords;

    match cfg.save(&state.config_path) {
        Ok(()) => Json(serde_json::json!({"ok": true})),
        Err(e) => Json(serde_json::json!({"ok": false, "error": e.to_string()})),
    }
}

/// GET /api/webhook-config — return current webhook configuration.
async fn get_webhook_config(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "url": state.config.webhook.url,
        "budget_warning_pct": state.config.webhook.budget_warning_pct,
        "cooldown_secs": state.config.webhook.cooldown_secs,
        "anomaly_detection": state.config.webhook.anomaly_detection,
        "tg_bot_token": state.config.webhook.tg_bot_token,
        "tg_chat_id": state.config.webhook.tg_chat_id,
    }))
}

#[derive(Deserialize)]
struct WebhookConfigPayload {
    url: String,
    budget_warning_pct: Option<f64>,
    cooldown_secs: Option<u64>,
    anomaly_detection: Option<bool>,
    tg_bot_token: Option<String>,
    tg_chat_id: Option<String>,
}

/// POST /api/webhook-config — save webhook configuration to config.yaml.
async fn save_webhook_config(
    State(state): State<Arc<AppState>>,
    Json(body): Json<WebhookConfigPayload>,
) -> Json<serde_json::Value> {
    let mut cfg = (*state.config).clone();
    cfg.webhook.url = body.url;
    if let Some(pct) = body.budget_warning_pct {
        cfg.webhook.budget_warning_pct = pct.clamp(0.0, 1.0);
    }
    if let Some(secs) = body.cooldown_secs {
        cfg.webhook.cooldown_secs = secs;
    }
    if let Some(anomaly) = body.anomaly_detection {
        cfg.webhook.anomaly_detection = anomaly;
    }
    if let Some(tg_token) = body.tg_bot_token {
        cfg.webhook.tg_bot_token = tg_token;
    }
    if let Some(tg_chat) = body.tg_chat_id {
        cfg.webhook.tg_chat_id = tg_chat;
    }

    match cfg.save(&state.config_path) {
        Ok(()) => Json(serde_json::json!({"ok": true})),
        Err(e) => Json(serde_json::json!({"ok": false, "error": e.to_string()})),
    }
}

/// GET /api/budget-banner — returns an HTML banner if budget is >80% spent.
async fn budget_banner(State(state): State<Arc<AppState>>) -> Html<String> {
    let now = chrono::Utc::now();
    let today_start = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp();
    let spent_today = state.store.total_cost_since(today_start);
    let daily_limit = state.config.budget.daily_limit_usd;

    let month_start = now.format("%Y-%m-01").to_string();
    let month_start_ts = chrono::NaiveDate::parse_from_str(&month_start, "%Y-%m-%d")
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp();
    let spent_month = state.store.total_cost_since(month_start_ts);
    let monthly_limit = state.config.budget.monthly_limit_usd;

    let mut warnings = Vec::new();

    if daily_limit > 0.0 {
        let pct = (spent_today / daily_limit * 100.0).min(100.0);
        if pct >= 100.0 {
            warnings.push(format!(
                "Daily budget exceeded: ${:.4} / ${:.2} (100%)",
                spent_today, daily_limit
            ));
        } else if pct >= state.config.webhook.budget_warning_pct * 100.0 {
            warnings.push(format!(
                "Daily budget warning: ${:.4} / ${:.2} ({:.0}%)",
                spent_today, daily_limit, pct
            ));
        }
    }

    if monthly_limit > 0.0 {
        let pct = (spent_month / monthly_limit * 100.0).min(100.0);
        if pct >= 100.0 {
            warnings.push(format!(
                "Monthly budget exceeded: ${:.4} / ${:.2} (100%)",
                spent_month, monthly_limit
            ));
        } else if pct >= state.config.webhook.budget_warning_pct * 100.0 {
            warnings.push(format!(
                "Monthly budget warning: ${:.4} / ${:.2} ({:.0}%)",
                spent_month, monthly_limit, pct
            ));
        }
    }

    if warnings.is_empty() {
        return Html(String::new());
    }

    let is_critical = warnings.iter().any(|w| w.contains("exceeded"));
    let class = if is_critical {
        "budget-warning budget-critical"
    } else {
        "budget-warning"
    };
    let joined = warnings.join(" · ");

    Html(format!(
        r#"<div class="{class}"><span class="icon">{icon}</span>{msg}</div>"#,
        class = class,
        icon = if is_critical { "⛔" } else { "⚠️" },
        msg = joined,
    ))
}

// ── Export Handlers ────────────────────────────────────

#[derive(Deserialize)]
struct ExportQuery {
    format: Option<String>,
    range: Option<String>,
    tenant: Option<String>,
}

/// GET /api/export/calls — export call history as CSV or JSON.
async fn export_calls(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ExportQuery>,
) -> impl IntoResponse {
    let range_hours = match params.range.as_deref() {
        Some("24h") => Some(24u32),
        Some("7d") => Some(168u32),
        Some("30d") => Some(720u32),
        _ => None,
    };
    let tenant_id = params.tenant.as_deref();
    let calls = state
        .store
        .recent_calls_filtered(100_000, 0, range_hours, None, None, tenant_id)
        .unwrap_or_default();

    match params.format.as_deref() {
        Some("json") => {
            let data: Vec<serde_json::Value> = calls
                .iter()
                .map(|c| {
                    let ts = chrono::DateTime::from_timestamp(c.timestamp, 0)
                        .map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                        .unwrap_or_default();
                    serde_json::json!({
                        "timestamp": ts,
                        "model": c.model,
                        "provider": c.provider,
                        "complexity": c.complexity,
                        "prompt_tokens": c.prompt_tokens,
                        "completion_tokens": c.completion_tokens,
                        "cost_usd": c.cost_usd,
                        "latency_ms": c.latency_ms,
                        "was_routed": c.was_routed,
                        "finish_reason": c.finish_reason,
                    })
                })
                .collect();
            let body = serde_json::to_string_pretty(&data).unwrap_or_default();
            (axum::response::Response::builder()
                .header("content-type", "application/json; charset=utf-8")
                .header(
                    "content-disposition",
                    "attachment; filename=\"tokenwise_calls.json\"",
                )
                .body(axum::body::Body::from(body))
                .unwrap(),)
                .into_response()
        }
        _ => {
            let mut csv = String::from(
                "timestamp,model,provider,complexity,prompt_tokens,completion_tokens,cost_usd,latency_ms,was_routed,finish_reason\n",
            );
            for c in &calls {
                let ts = chrono::DateTime::from_timestamp(c.timestamp, 0)
                    .map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                    .unwrap_or_default();
                csv.push_str(&format!(
                    "{},{},{},{},{},{},{:.6},{},{},{}\n",
                    ts,
                    csv_escape(&c.model),
                    csv_escape(&c.provider),
                    c.complexity,
                    c.prompt_tokens,
                    c.completion_tokens,
                    c.cost_usd,
                    c.latency_ms,
                    if c.was_routed { "true" } else { "false" },
                    c.finish_reason.as_deref().unwrap_or(""),
                ));
            }
            (axum::response::Response::builder()
                .header("content-type", "text/csv; charset=utf-8")
                .header(
                    "content-disposition",
                    "attachment; filename=\"tokenwise_calls.csv\"",
                )
                .body(axum::body::Body::from(csv))
                .unwrap(),)
                .into_response()
        }
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// GET /api/export/savings — export savings summary as JSON.
async fn export_savings(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let tenant_id = params.get("tenant").map(|s| s.as_str());
    let stats = state.store.monthly_stats(tenant_id).unwrap_or_default();
    let cache_stats = state.store.cache_stats();
    let routing_count = if state.routing_enabled {
        state.store.routing_count(tenant_id)
    } else {
        0
    };

    Json(serde_json::json!({
        "month": chrono::Utc::now().format("%Y-%m").to_string(),
        "total_calls": stats.total_calls,
        "total_cost_usd": stats.total_cost,
        "total_prompt_tokens": stats.total_prompt_tokens,
        "total_completion_tokens": stats.total_completion_tokens,
        "cache_hits": cache_stats.total_hits.saturating_sub(cache_stats.total_entries).max(0),
        "cache_entries": cache_stats.total_entries,
        "routed_calls": routing_count,
        "cache_savings_estimate_usd": state.store.cache_savings_estimate(),
    }))
}

// ── Demo Chat Endpoint ──────────────────────────────────

#[derive(Deserialize)]
struct DemoRequest {
    message: String,
}

/// POST /api/demo — returns a mock AI response without needing an API key.
/// Records a synthetic call so the Dashboard shows cost-tracking in action.
async fn demo_chat(
    State(state): State<Arc<AppState>>,
    Json(body): Json<DemoRequest>,
) -> impl IntoResponse {
    let msg = body.message.trim().to_string();
    if msg.is_empty() {
        return Json(serde_json::json!({
            "error": "message is required"
        }))
        .into_response();
    }

    // Generate a mock response that echoes the message
    let reply = format!(
        "Hello! This is a TokenWise Core demo response.\n\n\
         You said: \"{}\"\n\n\
         In production, your real API key would be forwarded to your AI provider \
         (e.g. DeepSeek, OpenAI) and you would see the actual response here.\n\n\
         TokenWise Core tracks every call so you always know where your money goes. \
         Check the Dashboard — this demo call just appeared in your history!",
        msg
    );

    let prompt_tokens: u32 = 15;
    let completion_tokens: u32 = (reply.len() as u32).max(1);
    let total_tokens = prompt_tokens + completion_tokens;

    // Record a synthetic call so the user can see Dashboard tracking in action
    let rec = crate::recording::CallRecord::from_request("demo", "demo", "simple", false, 42)
        .with_usage(prompt_tokens, completion_tokens);
    let _ = state.store.record_call(
        &rec,
        &serde_json::json!({"messages":[{"role":"user","content":&msg}]}),
    );

    Json(serde_json::json!({
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": reply
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": total_tokens,
            "cost_usd": 0.0
        }
    }))
    .into_response()
}

// ── Error page handlers ───────────────────────────────

/// Fallback handler — returns a themed 404 for unknown routes.
/// Respects the locale setting from config or ?lang= query param.
async fn fallback_404(
    State(state): State<Arc<AppState>>,
    req: axum::http::Request<axum::body::Body>,
) -> impl IntoResponse {
    // Determine locale: check query string first, then config
    let use_cn = req
        .uri()
        .query()
        .map(|q| {
            q.split('&')
                .filter_map(|p| p.split_once('='))
                .any(|(k, v)| k == "lang" && (v == "zh" || v == "cn"))
        })
        .unwrap_or(false)
        || is_cn(&state);

    let response = axum::response::Response::builder().status(axum::http::StatusCode::NOT_FOUND);

    if use_cn {
        let t = Error404TemplateCn {
            version: env!("CARGO_PKG_VERSION"),
        };
        match t.render() {
            Ok(html) => response
                .header("content-type", "text/html; charset=utf-8")
                .body(axum::body::Body::from(html))
                .unwrap(),
            Err(_) => response
                .body(axum::body::Body::from("404 Not Found"))
                .unwrap(),
        }
    } else {
        let t = Error404Template {
            version: env!("CARGO_PKG_VERSION"),
        };
        match t.render() {
            Ok(html) => response
                .header("content-type", "text/html; charset=utf-8")
                .body(axum::body::Body::from(html))
                .unwrap(),
            Err(_) => response
                .body(axum::body::Body::from("404 Not Found"))
                .unwrap(),
        }
    }
}

// ── Default impl for MonthlyStats ──────────────────────

impl Default for crate::recording::store::MonthlyStats {
    fn default() -> Self {
        Self {
            total_calls: 0,
            total_prompt_tokens: 0,
            total_completion_tokens: 0,
            total_cost: 0.0,
            avg_latency_ms: 0.0,
            potential_savings: 0.0,
        }
    }
}
