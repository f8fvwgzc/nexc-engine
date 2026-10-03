//! Keyed token-bucket rate limiting.
//!
//! * register / login / refresh / logout: 60 per minute per IP (burst 20);
//! * failed logins: 5 per minute per IP — the sixth failure within a minute
//!   gets `429` until the bucket refills;
//! * authenticated API calls: 600 per minute per user (burst 120).

use std::hash::Hash;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use dashmap::DashMap;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::AppError;
use crate::dsa::token_bucket::TokenBucket;
use crate::http::extract::client_ip;

#[derive(Debug, Clone, Copy)]
struct Limit {
    burst: u32,
    per_minute: f64,
}

const AUTH_PER_IP: Limit = Limit {
    burst: 20,
    per_minute: 60.0,
};
const LOGIN_FAILURES_PER_IP: Limit = Limit {
    burst: 5,
    per_minute: 5.0,
};
const API_PER_USER: Limit = Limit {
    burst: 120,
    per_minute: 600.0,
};

#[derive(Debug)]
struct Keyed<K: Eq + Hash> {
    limit: Limit,
    buckets: DashMap<K, TokenBucket>,
}

impl<K: Eq + Hash> Keyed<K> {
    fn new(limit: Limit) -> Self {
        Keyed {
            limit,
            buckets: DashMap::new(),
        }
    }

    fn bucket(&self, key: K) -> dashmap::mapref::one::RefMut<'_, K, TokenBucket> {
        self.buckets
            .entry(key)
            .or_insert_with(|| TokenBucket::new(self.limit.burst, self.limit.per_minute / 60.0))
    }

    fn acquire(&self, key: K) -> Result<(), AppError> {
        self.bucket(key).try_acquire().map_err(retry_after)
    }

    fn peek(&self, key: K) -> Result<(), AppError> {
        match self.bucket(key).peek() {
            wait if wait.is_zero() => Ok(()),
            wait => Err(retry_after(wait)),
        }
    }

    fn prune(&self, now: Instant) {
        self.buckets.retain(|_, b| !b.is_idle(now));
    }
}

fn retry_after(wait: Duration) -> AppError {
    AppError::RateLimited {
        retry_after_secs: wait.as_secs_f64().ceil() as u64,
    }
}

/// All limiters of the process.
#[derive(Debug)]
pub struct RateLimiters {
    auth: Keyed<IpAddr>,
    login_failures: Keyed<IpAddr>,
    users: Keyed<Uuid>,
}

impl Default for RateLimiters {
    fn default() -> Self {
        RateLimiters {
            auth: Keyed::new(AUTH_PER_IP),
            login_failures: Keyed::new(LOGIN_FAILURES_PER_IP),
            users: Keyed::new(API_PER_USER),
        }
    }
}

impl RateLimiters {
    /// Charges one authenticated request to `user`.
    pub fn check_user(&self, user: Uuid) -> Result<(), AppError> {
        self.users.acquire(user)
    }

    /// Fails with 429 while `ip` has exhausted its failed-login allowance.
    pub fn check_login_allowed(&self, ip: IpAddr) -> Result<(), AppError> {
        self.login_failures.peek(ip)
    }

    /// Records a failed login from `ip`.
    pub fn record_login_failure(&self, ip: IpAddr) {
        let _ = self.login_failures.acquire(ip);
    }

    /// Forgets buckets that are full again (bounded memory).
    pub fn prune(&self) {
        let now = Instant::now();
        self.auth.prune(now);
        self.login_failures.prune(now);
        self.users.prune(now);
    }
}

/// Extractor that charges one request to the client's `/auth/*` bucket.
/// Add it as the first argument of every auth handler.
#[derive(Debug, Clone, Copy)]
pub struct AuthRateLimit;

impl FromRequestParts<AppState> for AuthRateLimit {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        state
            .limiters
            .auth
            .acquire(client_ip(parts, state.settings.trust_proxy))?;
        Ok(AuthRateLimit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_failures_lock_out_after_five() {
        let l = RateLimiters::default();
        let ip: IpAddr = "203.0.113.9".parse().unwrap();
        for _ in 0..5 {
            assert!(l.check_login_allowed(ip).is_ok());
            l.record_login_failure(ip);
        }
        match l.check_login_allowed(ip) {
            Err(AppError::RateLimited { retry_after_secs }) => {
                assert!((1..=12).contains(&retry_after_secs))
            }
            other => panic!("expected 429, got {other:?}"),
        }
        let other: IpAddr = "203.0.113.10".parse().unwrap();
        assert!(l.check_login_allowed(other).is_ok());
    }
}
