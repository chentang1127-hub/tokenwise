//! REST API + HTMX dashboard endpoints.
//! Supports EN and ZH locales — switched via config.locale.

use std::sync::Arc;

use askama::Template;
use axum::{Router, extract::State, response::Html, routing::get};

use super::AppState;

pub fn make_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(dashboard))
        .route("/calls", get(calls_page))
        .route("/savings", get(savings_page))
        .route("/health", get(health))
        .with_state(state)
}

// ── English Templates ──────────────────────────────────

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    total_calls: i64,
    month_cost: String,
    estimated_savings: String,
    savings_pct: String,
    avg_latency: String,
    recent_calls: Vec<CallRow>,
}

#[derive(Template)]
#[template(path = "calls.html")]
struct CallsTemplate {
    calls: Vec<CallRow>,
}

#[derive(Template)]
#[template(path = "savings.html")]
struct SavingsTemplate {
    month_cost: String,
    estimated_savings: String,
    savings_pct: String,
}

// ── Chinese Templates ──────────────────────────────────

#[derive(Template)]
#[template(path = "cn/dashboard.html")]
struct DashboardTemplateCn {
    total_calls: i64,
    month_cost: String,
    estimated_savings: String,
    savings_pct: String,
    avg_latency: String,
    recent_calls: Vec<CallRow>,
}

#[derive(Template)]
#[template(path = "cn/calls.html")]
struct CallsTemplateCn {
    calls: Vec<CallRow>,
}

#[derive(Template)]
#[template(path = "cn/savings.html")]
struct SavingsTemplateCn {
    month_cost: String,
    estimated_savings: String,
    savings_pct: String,
}

// ── Shared row type ────────────────────────────────────

struct CallRow {
    model: String,
    complexity: String,
    latency: String,
    cost: String,
}

/// Whether the configured locale is Chinese.
fn is_cn(state: &Arc<AppState>) -> bool {
    state.config.locale == "zh" || state.config.locale == "cn"
}

// ── Handlers ───────────────────────────────────────────

async fn dashboard(State(state): State<Arc<AppState>>) -> Html<String> {
    let stats = state.store.monthly_stats().unwrap_or_default();
    let calls = state.store.recent_calls(20).unwrap_or_default();

    // Estimate savings: assume ~5x savings vs all-premium routing
    let estimated_savings = stats.total_cost * 5.0;
    let savings_pct = if stats.total_cost > 0.0 {
        format!(
            "{:.0}%",
            (estimated_savings / (stats.total_cost + estimated_savings)) * 100.0
        )
    } else {
        "N/A".to_string()
    };

    let total_calls = stats.total_calls;
    let month_cost = crate::cost::calculator::format_usd(stats.total_cost);
    let estimated_savings_fmt = crate::cost::calculator::format_usd(estimated_savings);
    let avg_latency = format!("{:.0}ms", stats.avg_latency_ms);
    let recent_calls: Vec<CallRow> = calls
        .into_iter()
        .map(|c| CallRow {
            model: format!("{}/{}", c.provider, c.model),
            complexity: c.complexity,
            latency: format!("{}ms", c.latency_ms),
            cost: crate::cost::calculator::format_usd(c.cost_usd),
        })
        .collect();

    if is_cn(&state) {
        let template = DashboardTemplateCn {
            total_calls,
            month_cost,
            estimated_savings: estimated_savings_fmt,
            savings_pct,
            avg_latency,
            recent_calls,
        };
        Html(
            template
                .render()
                .unwrap_or_else(|e| format!("Template error: {e}")),
        )
    } else {
        let template = DashboardTemplate {
            total_calls,
            month_cost,
            estimated_savings: estimated_savings_fmt,
            savings_pct,
            avg_latency,
            recent_calls,
        };
        Html(
            template
                .render()
                .unwrap_or_else(|e| format!("Template error: {e}")),
        )
    }
}

async fn calls_page(State(state): State<Arc<AppState>>) -> Html<String> {
    let calls = state.store.recent_calls(100).unwrap_or_default();
    let call_rows: Vec<CallRow> = calls
        .into_iter()
        .map(|c| CallRow {
            model: format!("{}/{}", c.provider, c.model),
            complexity: c.complexity,
            latency: format!("{}ms", c.latency_ms),
            cost: crate::cost::calculator::format_usd(c.cost_usd),
        })
        .collect();

    if is_cn(&state) {
        let template = CallsTemplateCn { calls: call_rows };
        Html(
            template
                .render()
                .unwrap_or_else(|e| format!("Template error: {e}")),
        )
    } else {
        let template = CallsTemplate { calls: call_rows };
        Html(
            template
                .render()
                .unwrap_or_else(|e| format!("Template error: {e}")),
        )
    }
}

async fn savings_page(State(state): State<Arc<AppState>>) -> Html<String> {
    let stats = state.store.monthly_stats().unwrap_or_default();
    let estimated_savings = stats.total_cost * 5.0;
    let savings_pct = if stats.total_cost > 0.0 {
        format!(
            "{:.0}%",
            (estimated_savings / (stats.total_cost + estimated_savings)) * 100.0
        )
    } else {
        "N/A".to_string()
    };

    let month_cost = crate::cost::calculator::format_usd(stats.total_cost);
    let estimated_savings_fmt = crate::cost::calculator::format_usd(estimated_savings);

    if is_cn(&state) {
        let template = SavingsTemplateCn {
            month_cost,
            estimated_savings: estimated_savings_fmt,
            savings_pct,
        };
        Html(
            template
                .render()
                .unwrap_or_else(|e| format!("Template error: {e}")),
        )
    } else {
        let template = SavingsTemplate {
            month_cost,
            estimated_savings: estimated_savings_fmt,
            savings_pct,
        };
        Html(
            template
                .render()
                .unwrap_or_else(|e| format!("Template error: {e}")),
        )
    }
}

async fn health() -> &'static str {
    "OK"
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
        }
    }
}
