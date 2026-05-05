//! Cost calculation: converts token counts × pricing rates → USD cost.

use crate::{lookup_pricing, TokenStats};

/// Calculate the USD cost for a model usage given raw token counts.
///
/// Returns `None` if `model_id` is not in the pricing table.
/// Negative token values are clamped to zero before calculation.
pub fn calculate_cost(
    model_id: &str,
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
) -> Option<f64> {
    let pricing = lookup_pricing(model_id)?;

    let cost = (input_tokens.max(0) as f64 / 1_000_000.0) * pricing.input_per_million
        + (output_tokens.max(0) as f64 / 1_000_000.0) * pricing.output_per_million
        + (cache_creation_tokens.max(0) as f64 / 1_000_000.0) * pricing.cache_creation_per_million
        + (cache_read_tokens.max(0) as f64 / 1_000_000.0) * pricing.cache_read_per_million;

    Some(cost)
}

/// Calculate the USD cost for a [`TokenStats`] snapshot under a given model.
///
/// Returns `None` if `model_id` is not in the pricing table.
pub fn calculate_stats_cost(stats: &TokenStats, model_id: &str) -> Option<f64> {
    calculate_cost(
        model_id,
        stats.input_tokens,
        stats.output_tokens,
        stats.cache_creation_tokens,
        stats.cache_read_tokens,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODEL: &str = "claude-3-haiku-20240307";

    // Helper for f64 equality with epsilon
    fn approx_eq(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    // calculate_cost tests

    #[test]
    fn test_all_buckets_one_million() {
        // input=0.25, output=1.25, cache_creation=0.3, cache_read=0.03 → sum=1.83
        let result = calculate_cost(MODEL, 1_000_000, 1_000_000, 1_000_000, 1_000_000);
        assert!(result.is_some());
        assert!(approx_eq(result.unwrap(), 1.83), "got {}", result.unwrap());
    }

    #[test]
    fn test_zero_tokens() {
        let result = calculate_cost(MODEL, 0, 0, 0, 0);
        assert!(result.is_some());
        assert!(approx_eq(result.unwrap(), 0.0));
    }

    #[test]
    fn test_negative_tokens_clamped() {
        let result = calculate_cost(MODEL, -500, -500, -500, -500);
        assert!(result.is_some());
        assert!(approx_eq(result.unwrap(), 0.0));
    }

    #[test]
    fn test_input_only_two_million() {
        // 2_000_000 input × 0.25/million = 0.5
        let result = calculate_cost(MODEL, 2_000_000, 0, 0, 0);
        assert!(result.is_some());
        assert!(approx_eq(result.unwrap(), 0.5), "got {}", result.unwrap());
    }

    #[test]
    fn test_unknown_model_returns_none() {
        let result = calculate_cost("not-a-real-model-xyz", 1000, 1000, 1000, 1000);
        assert!(result.is_none());
    }

    // calculate_stats_cost tests

    #[test]
    fn test_stats_all_buckets_one_million() {
        let stats = TokenStats {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            cache_creation_tokens: 1_000_000,
            cache_read_tokens: 1_000_000,
        };
        let result = calculate_stats_cost(&stats, MODEL);
        assert!(result.is_some());
        assert!(approx_eq(result.unwrap(), 1.83), "got {}", result.unwrap());
    }

    #[test]
    fn test_stats_default_zero() {
        let result = calculate_stats_cost(&TokenStats::default(), MODEL);
        assert!(result.is_some());
        assert!(approx_eq(result.unwrap(), 0.0));
    }

    #[test]
    fn test_stats_unknown_model_returns_none() {
        let stats = TokenStats {
            input_tokens: 100,
            output_tokens: 100,
            cache_creation_tokens: 100,
            cache_read_tokens: 100,
        };
        let result = calculate_stats_cost(&stats, "unknown-model-xyz");
        assert!(result.is_none());
    }
}
