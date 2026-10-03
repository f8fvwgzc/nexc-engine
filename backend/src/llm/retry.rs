//! Exponential backoff with full jitter (AWS architecture blog), shared by
//! LLM HTTP retries and the scheduler's node retries.

use std::time::Duration;

use crate::security::random::random_below;

/// Delay before retry `attempt` (1-based): uniform in `[0, min(cap, base·2^(attempt-1))]`.
pub fn full_jitter(attempt: u32, base: Duration, cap: Duration) -> Duration {
    let exp = base.saturating_mul(2u32.saturating_pow(attempt.saturating_sub(1).min(20)));
    let ceiling = exp.min(cap).as_millis() as u64;
    Duration::from_millis(random_below(ceiling + 1))
}

/// Parses a `retry-after` header given in seconds (HTTP-dates are ignored).
pub fn parse_retry_after(value: Option<&str>) -> Option<Duration> {
    let secs: f64 = value?.trim().parse().ok()?;
    (secs.is_finite() && secs >= 0.0).then(|| Duration::from_secs_f64(secs.min(300.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_is_bounded() {
        let base = Duration::from_millis(500);
        let cap = Duration::from_secs(8);
        for attempt in 1..10 {
            let d = full_jitter(attempt, base, cap);
            let ceiling = base * 2u32.pow(attempt - 1);
            assert!(d <= ceiling.min(cap), "{attempt}: {d:?}");
        }
    }

    #[test]
    fn parses_retry_after() {
        assert_eq!(parse_retry_after(Some("3")), Some(Duration::from_secs(3)));
        assert_eq!(
            parse_retry_after(Some("1.5")),
            Some(Duration::from_millis(1500))
        );
        assert_eq!(
            parse_retry_after(Some("Wed, 21 Oct 2015 07:28:00 GMT")),
            None
        );
        assert_eq!(
            parse_retry_after(Some("100000")),
            Some(Duration::from_secs(300))
        );
        assert_eq!(parse_retry_after(None), None);
    }
}
