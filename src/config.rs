//! Configuration loader — YAML file + env var overrides.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Top-level config structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub proxy: ProxyConfig,
    pub providers: Vec<ProviderConfig>,
    pub routing: RoutingConfig,
    pub safety_net: SafetyNetConfig,
    pub license: LicenseConfig,
    pub storage: StorageConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default = "default_locale")]
    pub locale: String,
    /// Run without opening a browser (Docker, headless servers, CI).
    #[serde(default)]
    pub headless: bool,
    /// Budget caps (0 = unlimited).
    #[serde(default)]
    pub budget: BudgetConfig,
    /// Webhook notifications for budget alerts and anomaly detection.
    #[serde(default)]
    pub webhook: crate::webhooks::WebhookConfig,
}

fn default_locale() -> String {
    "en".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    pub listen: String,
    pub admin: String,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub base_url: String,
    pub api_key_env: String,
    pub models: Vec<ModelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub id: String,
    pub tier: String,
    pub cost_per_1k_prompt: f64,
    pub cost_per_1k_completion: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingConfig {
    pub simple_max_tokens: usize,
    pub complex_min_tokens: usize,
    pub simple_keywords: Vec<String>,
    pub complex_keywords: Vec<String>,
    pub tier_simple: String,
    pub tier_complex: String,
    pub tier_default: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyNetConfig {
    pub enabled: bool,
    /// Max fallback retries (implemented in proxy::server).
    #[allow(dead_code)]
    pub max_fallback_retries: u32,
    pub fallback_map: HashMap<String, String>,
    /// Fallback on empty upstream response (implemented in proxy::server streaming safety net).
    #[allow(dead_code)]
    pub fallback_on_empty_response: bool,
    /// Fallback on truncated upstream response (implemented in proxy::server streaming safety net).
    #[allow(dead_code)]
    pub fallback_on_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseConfig {
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub db_path: String,
    pub retention_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Hours before a cache entry expires (default 24).
    #[serde(default = "default_cache_ttl")]
    pub ttl_hours: u32,
    /// Maximum cache entries (default 10,000).
    #[serde(default = "default_cache_max")]
    pub max_entries: u32,
}

/// Budget caps for cost control. Set to 0 to disable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    /// Daily spending limit in USD (0 = unlimited).
    #[serde(default)]
    pub daily_limit_usd: f64,
    /// Monthly spending limit in USD (0 = unlimited).
    #[serde(default)]
    pub monthly_limit_usd: f64,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            daily_limit_usd: 0.0,
            monthly_limit_usd: 0.0,
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            ttl_hours: 24,
            max_entries: 10_000,
        }
    }
}

fn default_cache_ttl() -> u32 {
    24
}
fn default_cache_max() -> u32 {
    10_000
}

impl Config {
    /// Load config from a YAML file.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = fs::read_to_string(path)?;
        let cfg: Config = serde_yaml::from_str(&contents)?;
        Ok(cfg)
    }

    /// Get a provider by name (reserved for future API use).
    #[allow(dead_code)]
    pub fn provider(&self, name: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| p.name == name)
    }

    /// Find a model config by provider name and model ID.
    pub fn model_config(&self, provider_name: &str, model_id: &str) -> Option<&ModelConfig> {
        for p in &self.providers {
            if p.name == provider_name {
                for m in &p.models {
                    if m.id == model_id {
                        return Some(m);
                    }
                }
            }
        }
        None
    }

    /// Find the cheapest model in a given tier (global, across all providers).
    pub fn cheapest_model_in_tier(&self, tier: &str) -> Option<(&ProviderConfig, &ModelConfig)> {
        let mut best: Option<(&ProviderConfig, &ModelConfig, f64)> = None;
        for p in &self.providers {
            for m in &p.models {
                if m.tier == tier {
                    let cost = m.cost_per_1k_prompt + m.cost_per_1k_completion;
                    if best.is_none() || cost < best.unwrap().2 {
                        best = Some((p, m, cost));
                    }
                }
            }
        }
        best.map(|(p, m, _)| (p, m))
    }

    /// Find the cheapest model in a given tier, scoped to a single provider.
    /// In zero-trust mode the client's API key only works for its own provider,
    /// so routing must stay within that provider.
    pub fn cheapest_model_in_tier_for_provider(
        &self,
        provider_name: &str,
        tier: &str,
    ) -> Option<&ModelConfig> {
        let provider = self.providers.iter().find(|p| p.name == provider_name)?;
        let mut best: Option<(&ModelConfig, f64)> = None;
        for m in &provider.models {
            if m.tier == tier {
                let cost = m.cost_per_1k_prompt + m.cost_per_1k_completion;
                if best.is_none() || cost < best.unwrap().1 {
                    best = Some((m, cost));
                }
            }
        }
        best.map(|(m, _)| m)
    }

    /// Apply TW_* environment variable overrides.
    /// Called after loading from YAML, before the config is used.
    /// Supported vars:
    ///   TW_HEADLESS         → headless (true/false/1/0)
    ///   TW_PROXY_LISTEN     → proxy.listen
    ///   TW_PROXY_ADMIN      → proxy.admin
    ///   TW_DB_PATH          → storage.db_path
    ///   TW_LICENSE_KEY      → license.key
    ///   TW_LOCALE           → locale
    ///   TW_BUDGET_DAILY     → budget.daily_limit_usd
    ///   TW_BUDGET_MONTHLY   → budget.monthly_limit_usd
    ///   TW_CACHE_TTL        → cache.ttl_hours
    pub fn apply_env_overrides(&mut self) {
        use std::env;

        if let Ok(v) = env::var("TW_HEADLESS") {
            self.headless = v == "true" || v == "1";
        }
        if let Ok(v) = env::var("TW_PROXY_LISTEN") {
            self.proxy.listen = v;
        }
        if let Ok(v) = env::var("TW_PROXY_ADMIN") {
            self.proxy.admin = v;
        }
        if let Ok(v) = env::var("TW_DB_PATH") {
            self.storage.db_path = v;
        }
        if let Ok(v) = env::var("TW_LICENSE_KEY") {
            self.license.key = v;
        }
        if let Ok(v) = env::var("TW_LOCALE") {
            self.locale = v;
        }
        if let Ok(v) = env::var("TW_BUDGET_DAILY")
            && let Ok(val) = v.parse::<f64>()
        {
            self.budget.daily_limit_usd = val;
        }
        if let Ok(v) = env::var("TW_BUDGET_MONTHLY")
            && let Ok(val) = v.parse::<f64>()
        {
            self.budget.monthly_limit_usd = val;
        }
        if let Ok(v) = env::var("TW_CACHE_TTL")
            && let Ok(val) = v.parse::<u32>()
        {
            self.cache.ttl_hours = val;
        }
    }

    /// Returns true on first run — checks for a marker file written by the
    /// setup wizard. In zero-trust mode, env vars are never set (TokenWise
    /// forwards the client's Authorization header directly), so we use a
    /// `.tokenwise_setup_done` file instead of checking for keys.
    pub fn is_first_run(&self) -> bool {
        !Path::new(".tokenwise_setup_done").exists()
    }

    /// Serialize config back to YAML on disk (used by Settings page and Pro setup wizard).
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), Box<dyn std::error::Error>> {
        let yaml = serde_yaml::to_string(self)?;
        fs::write(path, yaml)?;
        Ok(())
    }
}
