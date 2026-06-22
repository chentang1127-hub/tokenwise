//! Model router — selects the best model for a given classification.

use tracing::info;

use crate::config::Config;
use crate::proxy::classifier::Complexity;

/// Routing decision: which provider + model to use.
#[derive(Debug, Clone)]
pub struct Route {
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub api_key: String,
    pub tier: String,
}

/// Route a request based on complexity classification.
///
/// Picks the cheapest model in the target tier across ALL providers.
/// This is used for the "recommended" route display (what Pro would do)
/// and for cross-provider routing when TokenWise manages its own API keys.
pub fn route(complexity: Complexity, config: &Config) -> Route {
    let tier_name = match complexity {
        Complexity::Simple => &config.routing.tier_simple,
        Complexity::Medium => &config.routing.tier_default,
        Complexity::Complex => &config.routing.tier_complex,
    };

    let (provider, model) = config.cheapest_model_in_tier(tier_name).unwrap_or_else(|| {
        // Fallback: use first available provider
        let p = config.providers.first().expect("No providers configured");
        let m = p.models.first().expect("No models configured for provider");
        (p, m)
    });

    // api_key left empty — try_upstream will forward the client's
    // Authorization header instead. TokenWise never holds keys.
    info!(
        "📌 Routed [{}] → {}/{} (${:.6}/1K prompt, ${:.6}/1K completion)",
        tier_name, provider.name, model.id, model.cost_per_1k_prompt, model.cost_per_1k_completion,
    );

    Route {
        provider: provider.name.clone(),
        model: model.id.clone(),
        base_url: provider.base_url.clone(),
        api_key: String::new(),
        tier: tier_name.to_string(),
    }
}

/// Route within a specific provider — picks the cheapest model in the
/// target tier from THAT provider only. Essential for zero-trust mode:
/// the client's API key only works for their own provider.
pub fn route_within_provider(
    complexity: Complexity,
    config: &Config,
    provider_name: &str,
) -> Option<Route> {
    let tier_name = match complexity {
        Complexity::Simple => &config.routing.tier_simple,
        Complexity::Medium => &config.routing.tier_default,
        Complexity::Complex => &config.routing.tier_complex,
    };

    let model = config.cheapest_model_in_tier_for_provider(provider_name, tier_name)?;
    let provider = config.providers.iter().find(|p| p.name == provider_name)?;

    info!(
        "📌 Routed [{}] within {} → {}/{} (${:.6}/1K prompt, ${:.6}/1K completion)",
        tier_name,
        provider_name,
        provider.name,
        model.id,
        model.cost_per_1k_prompt,
        model.cost_per_1k_completion,
    );

    Some(Route {
        provider: provider.name.clone(),
        model: model.id.clone(),
        base_url: provider.base_url.clone(),
        api_key: String::new(),
        tier: tier_name.to_string(),
    })
}

/// Get fallback route: if a cheap model fails, escalate to the next tier.
/// Searches ALL providers for the cheapest model in the fallback tier.
pub fn fallback_route(previous_tier: &str, config: &Config) -> Option<Route> {
    let next_tier = config.safety_net.fallback_map.get(previous_tier)?;

    let (provider, model) = config.cheapest_model_in_tier(next_tier)?;

    // api_key left empty — try_upstream forwards the client's Authorization header.
    info!(
        "🔄 Fallback [{}] → [{}] {}/{}",
        previous_tier, next_tier, provider.name, model.id,
    );

    Some(Route {
        provider: provider.name.clone(),
        model: model.id.clone(),
        base_url: provider.base_url.clone(),
        api_key: String::new(),
        tier: next_tier.to_string(),
    })
}

/// Get fallback route within a specific provider. Escalates to the next tier
/// but only considers models from the SAME provider. In zero-trust mode,
/// the client's API key only works for their own provider.
pub fn fallback_route_within_provider(
    previous_tier: &str,
    config: &Config,
    provider_name: &str,
) -> Option<Route> {
    let next_tier = config.safety_net.fallback_map.get(previous_tier)?;
    let model = config.cheapest_model_in_tier_for_provider(provider_name, next_tier)?;
    let provider = config.providers.iter().find(|p| p.name == provider_name)?;

    info!(
        "🔄 Fallback [{}] → [{}] within {} → {}/{}",
        previous_tier, next_tier, provider_name, provider.name, model.id,
    );

    Some(Route {
        provider: provider.name.clone(),
        model: model.id.clone(),
        base_url: provider.base_url.clone(),
        api_key: String::new(),
        tier: next_tier.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        BudgetConfig, Config, ModelConfig, ProviderConfig, RoutingConfig, SafetyNetConfig,
    };
    use std::collections::HashMap;

    fn test_config() -> Config {
        Config {
            proxy: crate::config::ProxyConfig {
                listen: "127.0.0.1:9401".into(),
                admin: "127.0.0.1:9400".into(),
                timeout_secs: 120,
            },
            providers: vec![
                ProviderConfig {
                    name: "budget".into(),
                    base_url: "https://budget.api/v1".into(),
                    api_key_env: "BUDGET_KEY".into(),
                    models: vec![ModelConfig {
                        id: "budget-model".into(),
                        tier: "cheap".into(),
                        cost_per_1k_prompt: 0.0001,
                        cost_per_1k_completion: 0.0005,
                    }],
                },
                ProviderConfig {
                    name: "premium".into(),
                    base_url: "https://premium.api/v1".into(),
                    api_key_env: "PREMIUM_KEY".into(),
                    models: vec![ModelConfig {
                        id: "premium-model".into(),
                        tier: "premium".into(),
                        cost_per_1k_prompt: 0.003,
                        cost_per_1k_completion: 0.015,
                    }],
                },
            ],
            routing: RoutingConfig {
                simple_max_tokens: 300,
                complex_min_tokens: 1500,
                simple_keywords: vec!["summarize".into()],
                complex_keywords: vec!["implement".into()],
                tier_simple: "cheap".into(),
                tier_complex: "premium".into(),
                tier_default: "cheap".into(),
            },
            safety_net: SafetyNetConfig {
                enabled: true,
                max_fallback_retries: 1,
                fallback_map: {
                    let mut m = HashMap::new();
                    m.insert("cheap".to_string(), "premium".to_string());
                    m
                },
                fallback_on_empty_response: true,
                fallback_on_truncated: true,
            },
            license: crate::config::LicenseConfig { key: "".into() },
            storage: crate::config::StorageConfig {
                db_path: ":memory:".into(),
                retention_days: 90,
            },
            cache: crate::config::CacheConfig {
                ttl_hours: 24,
                max_entries: 10000,
            },
            locale: "en".into(),
            headless: false,
            budget: BudgetConfig::default(),
            webhook: crate::webhooks::WebhookConfig::default(),
        }
    }

    #[test]
    fn test_route_simple_to_cheap() {
        let cfg = test_config();
        let r = route(Complexity::Simple, &cfg);
        assert_eq!(r.tier, "cheap");
        assert_eq!(r.model, "budget-model");
    }

    #[test]
    fn test_route_complex_to_premium() {
        let cfg = test_config();
        let r = route(Complexity::Complex, &cfg);
        assert_eq!(r.tier, "premium");
        assert_eq!(r.model, "premium-model");
    }

    #[test]
    fn test_fallback_chain() {
        let cfg = test_config();
        let fb = fallback_route("cheap", &cfg);
        assert!(fb.is_some());
        let fb = fb.unwrap();
        assert_eq!(fb.tier, "premium");
    }

    #[test]
    fn test_fallback_none_for_unknown_tier() {
        let cfg = test_config();
        assert!(fallback_route("premium", &cfg).is_none());
    }
}
