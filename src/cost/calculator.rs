//! Cost calculator — computes actual cost and estimates savings.

use crate::config::{ModelConfig, ProviderConfig};

/// Compute the cost of a single API call.
pub fn compute_cost(prompt_tokens: u32, completion_tokens: u32, model: &ModelConfig) -> f64 {
    let prompt_cost = (prompt_tokens as f64 / 1000.0) * model.cost_per_1k_prompt;
    let completion_cost = (completion_tokens as f64 / 1000.0) * model.cost_per_1k_completion;
    prompt_cost + completion_cost
}

/// Estimate savings: what if this call used the most expensive model instead?
#[allow(dead_code)]
pub fn estimate_savings(
    prompt_tokens: u32,
    completion_tokens: u32,
    actual_model: &ModelConfig,
    all_models: &[(&ProviderConfig, &ModelConfig)],
) -> f64 {
    // Find the most expensive model across all providers
    let most_expensive = all_models.iter().max_by(|a, b| {
        let cost_a = a.1.cost_per_1k_prompt + a.1.cost_per_1k_completion;
        let cost_b = b.1.cost_per_1k_prompt + b.1.cost_per_1k_completion;
        cost_a.partial_cmp(&cost_b).unwrap()
    });

    if let Some((_, expensive_model)) = most_expensive {
        let premium_cost = compute_cost(prompt_tokens, completion_tokens, expensive_model);
        let actual_cost = compute_cost(prompt_tokens, completion_tokens, actual_model);
        (premium_cost - actual_cost).max(0.0)
    } else {
        0.0
    }
}

/// Format cost as USD string.
pub fn format_usd(cost: f64) -> String {
    if cost <= 0.0 {
        "$0.00".to_string()
    } else if cost < 0.01 {
        format!("${:.6}", cost)
    } else if cost < 1.0 {
        format!("${:.4}", cost)
    } else {
        format!("${:.2}", cost)
    }
}

/// Savings percentage string.
#[allow(dead_code)]
pub fn savings_pct(estimated_savings: f64, actual_cost: f64) -> String {
    if actual_cost <= 0.0 {
        return "N/A".to_string();
    }
    let hypothetical = actual_cost + estimated_savings;
    if hypothetical <= 0.0 {
        return "N/A".to_string();
    }
    let pct = (estimated_savings / hypothetical) * 100.0;
    format!("{:.0}%", pct)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_model() -> ModelConfig {
        ModelConfig {
            id: "test-model".into(),
            tier: "cheap".into(),
            cost_per_1k_prompt: 0.00027,
            cost_per_1k_completion: 0.0011,
        }
    }

    #[test]
    fn test_compute_cost() {
        let m = test_model();
        // 1000 prompt + 1000 completion
        let cost = compute_cost(1000, 1000, &m);
        assert!((cost - 0.00137).abs() < 0.00001);
    }

    #[test]
    fn test_estimate_savings() {
        let cheap = test_model();
        let expensive = ModelConfig {
            id: "expensive".into(),
            tier: "premium".into(),
            cost_per_1k_prompt: 0.003,
            cost_per_1k_completion: 0.015,
        };
        let savings = estimate_savings(
            1000,
            1000,
            &cheap,
            &[
                (
                    &ProviderConfig {
                        name: "test".into(),
                        base_url: "".into(),
                        api_key_env: "".into(),
                        models: vec![cheap.clone()],
                    },
                    &cheap,
                ),
                (
                    &ProviderConfig {
                        name: "expensive".into(),
                        base_url: "".into(),
                        api_key_env: "".into(),
                        models: vec![expensive.clone()],
                    },
                    &expensive,
                ),
            ],
        );
        assert!(savings > 0.0);
    }
}
