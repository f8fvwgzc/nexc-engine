//! Per-model token prices (USD per million tokens) for cost estimation.

/// `(model prefix, input $/MTok, output $/MTok)`. Longest matching prefix
/// wins so that dated or suffixed ids resolve to their family.
const PRICES: [(&str, f64, f64); 5] = [
    ("claude-opus-5-5", 4.0, 20.0),
    ("claude-opus-5", 5.0, 25.0),
    ("claude-sonnet-5", 2.0, 10.0),
    ("claude-haiku-4-5", 1.0, 5.0),
    ("claude-fable-5-1", 10.0, 50.0),
];

/// Estimated cost of a call; unknown models (local, demo) cost 0.
pub fn cost_usd(model: &str, input_tokens: u64, output_tokens: u64) -> f64 {
    let Some((_, input, output)) = PRICES
        .iter()
        .filter(|(prefix, _, _)| model.starts_with(prefix))
        .max_by_key(|(prefix, _, _)| prefix.len())
    else {
        return 0.0;
    };
    (input_tokens as f64 * input + output_tokens as f64 * output) / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_known_models() {
        assert!((cost_usd("claude-opus-5", 1_000_000, 1_000_000) - 30.0).abs() < 1e-9);
        assert!((cost_usd("claude-opus-5-5", 1_000_000, 0) - 4.0).abs() < 1e-9);
        assert!((cost_usd("claude-sonnet-5", 0, 1_000_000) - 10.0).abs() < 1e-9);
        assert!((cost_usd("claude-haiku-4-5", 2_000_000, 0) - 2.0).abs() < 1e-9);
        assert!((cost_usd("claude-fable-5-1", 0, 100_000) - 5.0).abs() < 1e-9);
        assert_eq!(cost_usd("llama3.1:8b", 1_000, 1_000), 0.0);
    }
}
