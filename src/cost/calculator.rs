//! Cost calculator — computes actual cost and estimates savings.

use crate::config::ModelConfig;

/// Compute the cost of a single API call.
pub fn compute_cost(prompt_tokens: u32, completion_tokens: u32, model: &ModelConfig) -> f64 {
    let prompt_cost = (prompt_tokens as f64 / 1000.0) * model.cost_per_1k_prompt;
    let completion_cost = (completion_tokens as f64 / 1000.0) * model.cost_per_1k_completion;
    prompt_cost + completion_cost
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
}
