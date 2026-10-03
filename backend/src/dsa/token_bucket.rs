//! Token-bucket rate limiter: O(1) per check, refilled lazily on access.

use std::time::{Duration, Instant};

/// A bucket of `capacity` tokens refilled at `refill_per_sec`.
#[derive(Debug, Clone)]
pub struct TokenBucket {
    capacity: f64,
    tokens: f64,
    refill_per_sec: f64,
    updated: Instant,
}

impl TokenBucket {
    /// A full bucket.
    pub fn new(capacity: u32, refill_per_sec: f64) -> Self {
        Self::new_at(capacity, refill_per_sec, Instant::now())
    }

    fn new_at(capacity: u32, refill_per_sec: f64, now: Instant) -> Self {
        TokenBucket {
            capacity: f64::from(capacity),
            tokens: f64::from(capacity),
            refill_per_sec,
            updated: now,
        }
    }

    fn refill(&mut self, now: Instant) {
        let elapsed = now.saturating_duration_since(self.updated).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_per_sec).min(self.capacity);
        self.updated = now;
    }

    /// Takes one token, or returns how long to wait until one is available.
    pub fn try_acquire(&mut self) -> Result<(), Duration> {
        self.try_acquire_at(Instant::now())
    }

    fn try_acquire_at(&mut self, now: Instant) -> Result<(), Duration> {
        self.refill(now);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Ok(())
        } else {
            Err(self.wait_for_token())
        }
    }

    /// Time until one token is available without taking it (zero when available).
    pub fn peek(&mut self) -> Duration {
        self.refill(Instant::now());
        if self.tokens >= 1.0 {
            Duration::ZERO
        } else {
            self.wait_for_token()
        }
    }

    fn wait_for_token(&self) -> Duration {
        Duration::from_secs_f64((1.0 - self.tokens) / self.refill_per_sec.max(f64::EPSILON))
    }

    /// True when the bucket has been idle long enough to be full again
    /// (and can therefore be forgotten by keyed limiters).
    pub fn is_idle(&self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.updated).as_secs_f64();
        self.tokens + elapsed * self.refill_per_sec >= self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_burst_then_limits_then_refills() {
        let t0 = Instant::now();
        let mut b = TokenBucket::new_at(5, 5.0 / 60.0, t0);
        for _ in 0..5 {
            assert!(b.try_acquire_at(t0).is_ok());
        }
        let wait = b.try_acquire_at(t0).unwrap_err();
        assert!((11.9..=12.1).contains(&wait.as_secs_f64()), "{wait:?}");
        assert!(b.try_acquire_at(t0 + Duration::from_secs(12)).is_ok());
        assert!(!b.is_idle(t0 + Duration::from_secs(12)));
        assert!(b.is_idle(t0 + Duration::from_secs(120)));
    }
}
