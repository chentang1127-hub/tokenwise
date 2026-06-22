//! Webhook notifications for budget alerts and anomaly detection.
//!
//! Fires HTTP POST callbacks to a configurable URL when:
//!   - Daily/monthly budget exceeds a warning threshold (e.g., 80%)
//!   - Budget cap is hit (100%)
//!   - Anomalous spending is detected (sudden spike)
//!
//! Webhooks are idempotent — each alert type fires at most once per
//! cooldown period to avoid notification storms.

use serde::{Deserialize, Serialize};

/// Configuration for webhook notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    /// Webhook URL to POST to (e.g., Slack, Discord, custom endpoint).
    pub url: String,
    /// Budget warning threshold (0.0–1.0). Fire when spending reaches
    /// this fraction of the limit. Default 0.80 = 80%.
    #[serde(default = "default_warning_threshold")]
    pub budget_warning_pct: f64,
    /// Cooldown in seconds between duplicate alerts.
    #[serde(default = "default_cooldown")]
    pub cooldown_secs: u64,
    /// Enable anomaly detection (spending spike alerts).
    #[serde(default)]
    pub anomaly_detection: bool,
}

fn default_warning_threshold() -> f64 {
    0.80
}
fn default_cooldown() -> u64 {
    3600 // 1 hour
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            budget_warning_pct: 0.80,
            cooldown_secs: 3600,
            anomaly_detection: false,
        }
    }
}

/// Types of webhook events.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum WebhookEvent {
    BudgetWarning {
        scope: String, // "daily" or "monthly"
        spent: f64,
        limit: f64,
        pct: f64,
    },
    BudgetExceeded {
        scope: String,
        spent: f64,
        limit: f64,
    },
    AnomalyDetected {
        message: String,
        current_cost: f64,
        avg_cost: f64,
    },
}

/// Stateful webhook dispatcher with cooldown tracking.
pub struct WebhookDispatcher {
    config: WebhookConfig,
    /// Timestamps of last alert per event type (for cooldown).
    last_budget_warning_daily: i64,
    last_budget_warning_monthly: i64,
    last_budget_exceeded_daily: i64,
    last_budget_exceeded_monthly: i64,
    #[allow(dead_code)]
    last_anomaly: i64,
}

impl WebhookDispatcher {
    /// Create a new dispatcher. Returns None if no URL is configured.
    pub fn new(config: WebhookConfig) -> Option<Self> {
        if config.url.is_empty() {
            return None;
        }
        Some(Self {
            config,
            last_budget_warning_daily: 0,
            last_budget_warning_monthly: 0,
            last_budget_exceeded_daily: 0,
            last_budget_exceeded_monthly: 0,
            last_anomaly: 0,
        })
    }

    /// Check and fire budget-related alerts.
    /// Called after every request that increments spending.
    pub async fn check_budget(
        &mut self,
        spent_today: f64,
        daily_limit: f64,
        spent_month: f64,
        monthly_limit: f64,
        now: i64,
    ) {
        let cooldown = self.config.cooldown_secs as i64;

        // Daily budget checks
        if daily_limit > 0.0 {
            let pct = spent_today / daily_limit;
            if pct >= 1.0 && now - self.last_budget_exceeded_daily > cooldown {
                self.last_budget_exceeded_daily = now;
                let _ = self
                    .send(WebhookEvent::BudgetExceeded {
                        scope: "daily".into(),
                        spent: spent_today,
                        limit: daily_limit,
                    })
                    .await;
            } else if pct >= self.config.budget_warning_pct
                && now - self.last_budget_warning_daily > cooldown
            {
                self.last_budget_warning_daily = now;
                let _ = self
                    .send(WebhookEvent::BudgetWarning {
                        scope: "daily".into(),
                        spent: spent_today,
                        limit: daily_limit,
                        pct: pct * 100.0,
                    })
                    .await;
            }
        }

        // Monthly budget checks
        if monthly_limit > 0.0 {
            let pct = spent_month / monthly_limit;
            if pct >= 1.0 && now - self.last_budget_exceeded_monthly > cooldown {
                self.last_budget_exceeded_monthly = now;
                let _ = self
                    .send(WebhookEvent::BudgetExceeded {
                        scope: "monthly".into(),
                        spent: spent_month,
                        limit: monthly_limit,
                    })
                    .await;
            } else if pct >= self.config.budget_warning_pct
                && now - self.last_budget_warning_monthly > cooldown
            {
                self.last_budget_warning_monthly = now;
                let _ = self
                    .send(WebhookEvent::BudgetWarning {
                        scope: "monthly".into(),
                        spent: spent_month,
                        limit: monthly_limit,
                        pct: pct * 100.0,
                    })
                    .await;
            }
        }
    }

    /// Send a webhook event via HTTP POST.
    async fn send(&self, event: WebhookEvent) -> Result<(), String> {
        let client = reqwest::Client::new();
        let payload = serde_json::to_vec(&event).map_err(|e| e.to_string())?;

        let resp = client
            .post(&self.config.url)
            .header("Content-Type", "application/json")
            .header("User-Agent", "TokenWise-Webhook/1.0")
            .body(payload)
            .send()
            .await
            .map_err(|e| format!("Webhook POST failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("Webhook returned {}", resp.status()));
        }

        tracing::info!("Webhook sent: {:?}", event);
        Ok(())
    }
}
