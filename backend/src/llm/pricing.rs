//! Per-model token prices (USD per million tokens) for cost estimation.

/// `(model prefix, input $/MTok, output $/MTok, cache read $/MTok)`.
/// Longest matching prefix wins so that dated or suffixed ids resolve to
/// their family. Cache reads are not a fixed share of the input price: the
/// newest models discount them further.
const PRICES: [(&str, f64, f64, f64); 11] = [
    ("claude-fable-5-1", 10.0, 50.0, 0.25),
    ("claude-fable-5", 10.0, 50.0, 1.0),
    ("claude-opus-5-5", 4.0, 20.0, 0.20),
    ("claude-opus-5", 5.0, 25.0, 0.50),
    ("claude-opus-4", 5.0, 25.0, 0.50),
    ("claude-sonnet-5-5", 2.0, 10.0, 0.20),
    ("claude-sonnet-5", 2.0, 10.0, 0.20),
    ("claude-sonnet-4", 3.0, 15.0, 0.30),
    // Prompts up to 100K tokens; longer ones cost five times as much (not modelled).
    ("claude-haiku-5-5", 0.10, 0.50, 0.01),
    ("claude-haiku-4-5", 1.0, 5.0, 0.10),
    ("claude-haiku-4", 1.0, 5.0, 0.10),
];

/// Estimated cost of a call; unknown models (local, demo) cost 0.
pub fn cost_usd(model: &str, input_tokens: u64, output_tokens: u64) -> f64 {
    cost_usd_cached(model, input_tokens, output_tokens, 0)
}

/// Like [`cost_usd`], with `cached_tokens` of the input priced as cache reads.
pub fn cost_usd_cached(
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    cached_tokens: u64,
) -> f64 {
    let Some((_, input, output, read)) = PRICES
        .iter()
        .filter(|(prefix, ..)| model.starts_with(prefix))
        .max_by_key(|(prefix, ..)| prefix.len())
    else {
        return 0.0;
    };
    let cached = cached_tokens.min(input_tokens) as f64;
    let fresh = input_tokens as f64 - cached;
    (fresh * input + cached * read + output_tokens as f64 * output) / 1_000_000.0
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

    #[test]
    fn current_models_have_prices() {
        assert!((cost_usd("claude-sonnet-5-5", 1_000_000, 1_000_000) - 12.0).abs() < 1e-9);
        assert!((cost_usd("claude-haiku-5-5", 1_000_000, 1_000_000) - 0.6).abs() < 1e-9);
        assert!((cost_usd("claude-opus-4-8", 1_000_000, 0) - 5.0).abs() < 1e-9);
        assert!((cost_usd("claude-sonnet-4-6", 1_000_000, 0) - 3.0).abs() < 1e-9);
        // The longest prefix wins: Fable 5.1 is not priced as Fable 5.
        assert!((cost_usd_cached("claude-fable-5-1", 1_000_000, 0, 1_000_000) - 0.25).abs() < 1e-9);
        assert!((cost_usd_cached("claude-fable-5", 1_000_000, 0, 1_000_000) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cache_reads_cost_what_the_model_charges_for_them() {
        // A million input tokens, 900k of them cache reads: 100k fresh + 900k at $0.20/MTok.
        let cost = cost_usd_cached("claude-sonnet-5", 1_000_000, 0, 900_000);
        assert!((cost - (0.1 * 2.0 + 0.9 * 0.20)).abs() < 1e-9);
        // Opus 5.5 reads at a twentieth of its input price.
        let cost = cost_usd_cached("claude-opus-5-5", 1_000_000, 0, 1_000_000);
        assert!((cost - 0.20).abs() < 1e-9);
        // Reads cannot exceed input, however they were reported.
        assert!((cost_usd_cached("claude-sonnet-5", 1_000, 0, 5_000) - 0.0002).abs() < 1e-9);
        assert_eq!(
            cost_usd_cached("claude-sonnet-5", 1_000, 0, 0),
            cost_usd("claude-sonnet-5", 1_000, 0)
        );
    }
}
