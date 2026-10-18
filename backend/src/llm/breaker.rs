//! A circuit breaker per LLM provider endpoint: after `failures_to_open`
//! consecutive failed calls the circuit opens and every call to that
//! endpoint fails at once, instead of each waiting on its own retries and
//! timeouts; after `open_for` one probe is let through (half-open), and its
//! outcome closes the circuit again or opens it for another period.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::observability::metrics::Metrics;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Closed,
    Open {
        until: Instant,
    },
    /// One probe is on its way; the rest wait for its verdict.
    HalfOpen {
        probing: bool,
    },
}

#[derive(Debug)]
struct Circuit {
    consecutive_failures: u32,
    state: State,
}

/// The breaker of every provider endpoint, keyed by `provider@base_url`.
#[derive(Debug)]
pub struct CircuitBreaker {
    failures_to_open: u32,
    open_for: Duration,
    circuits: Mutex<HashMap<String, Circuit>>,
    metrics: Arc<Metrics>,
}

/// Why a call was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refused {
    /// How long until the next probe is let through.
    pub retry_after: Duration,
}

impl CircuitBreaker {
    /// A breaker that opens after `failures_to_open` consecutive failures
    /// (0: never) for `open_for`.
    pub fn new(failures_to_open: u32, open_for: Duration, metrics: Arc<Metrics>) -> Self {
        CircuitBreaker {
            failures_to_open,
            open_for,
            circuits: Mutex::new(HashMap::new()),
            metrics,
        }
    }

    /// Whether the breaker is on at all.
    pub fn is_enabled(&self) -> bool {
        self.failures_to_open > 0
    }

    /// Whether a call to `key` may go out now: yes while the circuit is
    /// closed, once as a probe when it is half-open, no while it is open.
    pub fn admit(&self, key: &str) -> Result<(), Refused> {
        self.admit_at(key, Instant::now())
    }

    fn admit_at(&self, key: &str, now: Instant) -> Result<(), Refused> {
        if !self.is_enabled() {
            return Ok(());
        }
        let mut circuits = self.circuits.lock().expect("breaker lock");
        let Some(circuit) = circuits.get_mut(key) else {
            return Ok(());
        };
        match circuit.state {
            State::Closed => Ok(()),
            State::Open { until } if now < until => {
                self.metrics
                    .llm_breaker
                    .add(&[("provider", key), ("event", "refused")], 1);
                Err(Refused {
                    retry_after: until - now,
                })
            }
            State::Open { .. } => {
                circuit.state = State::HalfOpen { probing: true };
                self.metrics
                    .llm_breaker
                    .add(&[("provider", key), ("event", "half_open")], 1);
                Ok(())
            }
            State::HalfOpen { probing: false } => {
                circuit.state = State::HalfOpen { probing: true };
                Ok(())
            }
            State::HalfOpen { probing: true } => {
                self.metrics
                    .llm_breaker
                    .add(&[("provider", key), ("event", "refused")], 1);
                Err(Refused {
                    retry_after: Duration::from_secs(1),
                })
            }
        }
    }

    /// A call to `key` answered: the circuit closes.
    pub fn record_success(&self, key: &str) {
        if !self.is_enabled() {
            return;
        }
        let mut circuits = self.circuits.lock().expect("breaker lock");
        if let Some(circuit) = circuits.get_mut(key) {
            if circuit.state != State::Closed {
                self.metrics
                    .llm_breaker
                    .add(&[("provider", key), ("event", "closed")], 1);
            }
            circuit.consecutive_failures = 0;
            circuit.state = State::Closed;
        }
    }

    /// A call to `key` failed in a way a retry might cure (the provider is
    /// down, overloaded or unreachable): one more towards opening, or the
    /// probe that failed opens the circuit again.
    pub fn record_failure(&self, key: &str) {
        self.record_failure_at(key, Instant::now());
    }

    fn record_failure_at(&self, key: &str, now: Instant) {
        if !self.is_enabled() {
            return;
        }
        let mut circuits = self.circuits.lock().expect("breaker lock");
        let circuit = circuits.entry(key.to_owned()).or_insert(Circuit {
            consecutive_failures: 0,
            state: State::Closed,
        });
        circuit.consecutive_failures = circuit.consecutive_failures.saturating_add(1);
        let open = match circuit.state {
            State::HalfOpen { .. } => true,
            State::Closed => circuit.consecutive_failures >= self.failures_to_open,
            State::Open { .. } => false,
        };
        if open {
            circuit.state = State::Open {
                until: now + self.open_for,
            };
            self.metrics
                .llm_breaker
                .add(&[("provider", key), ("event", "opened")], 1);
            tracing::warn!(
                provider = key,
                failures = circuit.consecutive_failures,
                open_for_secs = self.open_for.as_secs(),
                "LLM circuit opened: calls fail at once until a probe succeeds"
            );
        }
    }

    /// The circuits that are not closed, for the operator: `(key, state)`.
    pub fn open_circuits(&self) -> Vec<(String, String)> {
        let now = Instant::now();
        self.circuits
            .lock()
            .expect("breaker lock")
            .iter()
            .filter_map(|(key, circuit)| match circuit.state {
                State::Closed => None,
                State::Open { until } => Some((
                    key.clone(),
                    format!(
                        "open, {} s until a probe",
                        until.saturating_duration_since(now).as_secs()
                    ),
                )),
                State::HalfOpen { .. } => Some((key.clone(), "half-open".into())),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn breaker(failures: u32) -> CircuitBreaker {
        CircuitBreaker::new(
            failures,
            Duration::from_secs(30),
            Arc::new(Metrics::default()),
        )
    }

    #[test]
    fn opens_after_consecutive_failures_and_probes_after_the_period() {
        let b = breaker(3);
        let t0 = Instant::now();
        assert!(b.admit_at("anthropic@api", t0).is_ok());
        b.record_failure_at("anthropic@api", t0);
        b.record_failure_at("anthropic@api", t0);
        assert!(b.admit_at("anthropic@api", t0).is_ok(), "two are not three");
        b.record_success("anthropic@api");
        b.record_failure_at("anthropic@api", t0);
        b.record_failure_at("anthropic@api", t0);
        assert!(
            b.admit_at("anthropic@api", t0).is_ok(),
            "a success resets the count"
        );
        b.record_failure_at("anthropic@api", t0);
        let refused = b
            .admit_at("anthropic@api", t0 + Duration::from_secs(1))
            .unwrap_err();
        assert_eq!(refused.retry_after, Duration::from_secs(29));
        assert!(
            b.admit_at("openai@local", t0).is_ok(),
            "another endpoint is its own circuit"
        );
        assert_eq!(b.open_circuits().len(), 1);

        // After the period one probe goes out; the others wait for its verdict.
        let later = t0 + Duration::from_secs(31);
        assert!(b.admit_at("anthropic@api", later).is_ok());
        assert!(
            b.admit_at("anthropic@api", later).is_err(),
            "one probe at a time"
        );
        b.record_failure_at("anthropic@api", later);
        assert!(
            b.admit_at("anthropic@api", later + Duration::from_secs(1))
                .is_err(),
            "the failed probe reopened it"
        );
        let again = later + Duration::from_secs(31);
        assert!(b.admit_at("anthropic@api", again).is_ok());
        b.record_success("anthropic@api");
        assert!(b.admit_at("anthropic@api", again).is_ok(), "closed again");
        assert!(b.admit_at("anthropic@api", again).is_ok());
        assert!(b.open_circuits().is_empty());
        let rendered = b.metrics.render();
        assert!(
            rendered
                .contains("nexc_llm_breaker_total{provider=\"anthropic@api\",event=\"opened\"} 2")
        );
        assert!(
            rendered
                .contains("nexc_llm_breaker_total{provider=\"anthropic@api\",event=\"closed\"} 1")
        );
    }

    #[test]
    fn zero_failures_turns_it_off() {
        let b = breaker(0);
        for _ in 0..10 {
            b.record_failure("anthropic@api");
        }
        assert!(b.admit("anthropic@api").is_ok());
        assert!(!b.is_enabled());
    }
}
