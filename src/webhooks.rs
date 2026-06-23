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
    #[serde(default)]
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
    /// Telegram Bot token (from @BotFather). Leave empty to disable TG alerts.
    #[serde(default)]
    pub tg_bot_token: String,
    /// Telegram chat ID to send alerts to.
    #[serde(default)]
    pub tg_chat_id: String,
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
            tg_bot_token: String::new(),
            tg_chat_id: String::new(),
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
    /// Daily usage summary (fired once per day by background task).
    UsageReport {
        date: String,
        total_calls: i64,
        total_cost: f64,
        total_prompt_tokens: i64,
        total_completion_tokens: i64,
        cache_hits: i64,
        routed_calls: i64,
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
    /// Date of last usage report (YYYY-MM-DD) to prevent duplicate daily reports.
    last_usage_report_date: String,
    /// Running average of cost delta per check interval (for anomaly detection).
    avg_cost_delta: f64,
    /// Total cost at last anomaly check (to compute delta).
    last_total_cost: f64,
    /// Number of anomaly samples collected (for running average).
    anomaly_samples: u64,
}

impl WebhookDispatcher {
    /// Create a new dispatcher. Returns None if no webhook URL or TG bot is configured.
    pub fn new(config: WebhookConfig) -> Option<Self> {
        if config.url.is_empty() && config.tg_bot_token.is_empty() {
            return None;
        }
        Some(Self {
            config,
            last_budget_warning_daily: 0,
            last_budget_warning_monthly: 0,
            last_budget_exceeded_daily: 0,
            last_budget_exceeded_monthly: 0,
            last_anomaly: 0,
            last_usage_report_date: String::new(),
            avg_cost_delta: 0.0,
            last_total_cost: 0.0,
            anomaly_samples: 0,
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
                let event = WebhookEvent::BudgetExceeded {
                    scope: "daily".into(),
                    spent: spent_today,
                    limit: daily_limit,
                };
                let _ = self.send(event.clone()).await;
                self.notify_telegram(&event).await;
            } else if pct >= self.config.budget_warning_pct
                && now - self.last_budget_warning_daily > cooldown
            {
                self.last_budget_warning_daily = now;
                let event = WebhookEvent::BudgetWarning {
                    scope: "daily".into(),
                    spent: spent_today,
                    limit: daily_limit,
                    pct: pct * 100.0,
                };
                let _ = self.send(event.clone()).await;
                self.notify_telegram(&event).await;
            }
        }

        // Monthly budget checks
        if monthly_limit > 0.0 {
            let pct = spent_month / monthly_limit;
            if pct >= 1.0 && now - self.last_budget_exceeded_monthly > cooldown {
                self.last_budget_exceeded_monthly = now;
                let event = WebhookEvent::BudgetExceeded {
                    scope: "monthly".into(),
                    spent: spent_month,
                    limit: monthly_limit,
                };
                let _ = self.send(event.clone()).await;
                self.notify_telegram(&event).await;
            } else if pct >= self.config.budget_warning_pct
                && now - self.last_budget_warning_monthly > cooldown
            {
                self.last_budget_warning_monthly = now;
                let event = WebhookEvent::BudgetWarning {
                    scope: "monthly".into(),
                    spent: spent_month,
                    limit: monthly_limit,
                    pct: pct * 100.0,
                };
                let _ = self.send(event.clone()).await;
                self.notify_telegram(&event).await;
            }
        }
    }

    /// Check for anomalous spending (sudden spike).
    /// Compares current cost delta against a running average.
    /// Fires when current delta exceeds 3x the average and anomaly_detection is enabled.
    pub async fn check_anomaly(&mut self, current_total_cost: f64, now: i64) {
        if !self.config.anomaly_detection {
            return;
        }
        let cooldown = self.config.cooldown_secs as i64;
        let delta = current_total_cost - self.last_total_cost;
        self.last_total_cost = current_total_cost;

        // Build running average over first 12 samples (1 hour at 5-min intervals)
        if self.anomaly_samples < 12 {
            self.avg_cost_delta =
                (self.avg_cost_delta * self.anomaly_samples as f64 + delta)
                    / (self.anomaly_samples + 1) as f64;
            self.anomaly_samples += 1;
            return;
        }

        // Update running average with decay
        self.avg_cost_delta = self.avg_cost_delta * 0.9 + delta * 0.1;

        // Fire if current delta > 3x average AND cooldown has passed
        if delta > self.avg_cost_delta * 3.0
            && self.avg_cost_delta > 0.001
            && now - self.last_anomaly > cooldown
        {
            self.last_anomaly = now;
            let event = WebhookEvent::AnomalyDetected {
                message: format!(
                    "Spending spike: ${delta:.4} in last interval vs ${:.4} avg",
                    self.avg_cost_delta
                ),
                current_cost: delta,
                avg_cost: self.avg_cost_delta,
            };
            let _ = self.send(event.clone()).await;
            self.notify_telegram(&event).await;
        }
    }

    /// Fire a daily UsageReport webhook if the date has changed since the last report.
    /// Returns true if a report was sent.
    #[allow(clippy::too_many_arguments)]
    pub async fn send_usage_report(
        &mut self,
        _now: i64,
        total_calls: i64,
        total_cost: f64,
        total_prompt_tokens: i64,
        total_completion_tokens: i64,
        cache_hits: i64,
        routed_calls: i64,
    ) -> bool {
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        if today == self.last_usage_report_date {
            return false;
        }
        self.last_usage_report_date = today.clone();
        let event = WebhookEvent::UsageReport {
            date: today,
            total_calls,
            total_cost,
            total_prompt_tokens,
            total_completion_tokens,
            cache_hits,
            routed_calls,
        };
        let _ = self.send(event.clone()).await;
        self.notify_telegram(&event).await;
        true
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

    /// Send a message via Telegram Bot API.
    async fn send_telegram(&self, text: &str) -> bool {
        if self.config.tg_bot_token.is_empty() || self.config.tg_chat_id.is_empty() {
            return false;
        }
        let url = format!(
            "https://api.telegram.org/bot{}/sendMessage",
            self.config.tg_bot_token
        );
        let payload = serde_json::json!({
            "chat_id": self.config.tg_chat_id,
            "text": text,
            "parse_mode": "HTML",
            "disable_web_page_preview": true,
        });
        match reqwest::Client::new()
            .post(&url)
            .header("Content-Type", "application/json")
            .body(serde_json::to_vec(&payload).unwrap_or_default())
            .send()
            .await
        {
            Ok(resp) => {
                if resp.status().is_success() {
                    tracing::info!("TG sent");
                    true
                } else {
                    tracing::warn!("TG fail {}", resp.status());
                    false
                }
            }
            Err(e) => {
                tracing::warn!("TG error: {e}");
                false
            }
        }
    }

    /// Send an event via Telegram if configured.
    pub async fn notify_telegram(&self, event: &WebhookEvent) {
        let text = match event {
            WebhookEvent::BudgetWarning { scope, spent, limit, pct } => {
                format!("⚠️ Budget Warning ({scope}): ${spent:.2}/${limit:.2} ({pct:.0}%)")
            }
            WebhookEvent::BudgetExceeded { scope, spent, limit } => {
                format!("🚫 Budget EXCEEDED ({scope}): ${spent:.2}/${limit:.2}")
            }
            WebhookEvent::AnomalyDetected { message, current_cost, avg_cost } => {
                format!("🔴 Anomaly: {message}\nCurrent ${current_cost:.4} vs avg ${avg_cost:.4}")
            }
            WebhookEvent::UsageReport { date, total_calls, total_cost, total_prompt_tokens, total_completion_tokens, cache_hits, routed_calls } => {
                format!(
                    "📊 Daily Report {date}\nCalls: {total_calls} | Cost: ${total_cost:.4}\nTokens: {total_prompt_tokens}/{total_completion_tokens} | Cache: {cache_hits} | Routed: {routed_calls}"
                )
            }
        };
        self.send_telegram(&text).await;
    }
}
