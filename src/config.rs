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
    #[serde(default = "default_locale")]
    pub locale: String,
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
    pub max_fallback_retries: u32,
    pub fallback_map: HashMap<String, String>,
    pub fallback_on_empty_response: bool,
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

impl Config {
    /// Load config from a YAML file.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = fs::read_to_string(path)?;
        let mut cfg: Config = serde_yaml::from_str(&contents)?;
        cfg.resolve_env_overrides();
        Ok(cfg)
    }

    /// Replace `${ENV_VAR}` placeholders in API key refs with actual env vars.
    /// Providers reference env vars by name in `api_key_env`, so values are
    /// resolved at runtime — nothing to substitute in the YAML itself.
    fn resolve_env_overrides(&mut self) {
        // Future: allow ${VAR} interpolation in base_url etc.
        let _ = self;
    }

    /// Get a provider by name.
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

    /// Find the cheapest model in a given tier.
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
}
