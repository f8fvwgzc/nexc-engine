//! Prometheus metrics maintained with atomics and rendered in the text
//! exposition format (no metrics crate needed).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const LATENCY_BUCKETS: [f64; 10] = [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 10.0];
const RUN_BUCKETS: [f64; 9] = [1.0, 5.0, 15.0, 30.0, 60.0, 120.0, 300.0, 900.0, 3600.0];

/// Monotonic counter.
#[derive(Debug, Default)]
pub struct Counter(AtomicU64);

impl Counter {
    /// Adds `n`.
    pub fn add(&self, n: u64) {
        self.0.fetch_add(n, Ordering::Relaxed);
    }

    fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// Counter with one label set per series.
#[derive(Debug, Default)]
pub struct LabeledCounter(Mutex<BTreeMap<String, u64>>);

impl LabeledCounter {
    /// Adds `n` to the series identified by `labels` (`[("status","200")]`).
    pub fn add(&self, labels: &[(&str, &str)], n: u64) {
        let key = labels
            .iter()
            .map(|(k, v)| format!("{k}=\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(",");
        *self.0.lock().expect("metrics lock").entry(key).or_default() += n;
    }

    fn render(&self, out: &mut String, name: &str) {
        for (labels, value) in self.0.lock().expect("metrics lock").iter() {
            let _ = writeln!(out, "{name}{{{labels}}} {value}");
        }
    }
}

/// Cumulative histogram with fixed buckets.
#[derive(Debug)]
pub struct Histogram {
    bounds: &'static [f64],
    buckets: Vec<AtomicU64>,
    count: AtomicU64,
    sum_micros: AtomicU64,
}

impl Histogram {
    fn new(bounds: &'static [f64]) -> Self {
        Histogram {
            bounds,
            buckets: bounds.iter().map(|_| AtomicU64::new(0)).collect(),
            count: AtomicU64::new(0),
            sum_micros: AtomicU64::new(0),
        }
    }

    /// Records one observation.
    pub fn observe(&self, value: Duration) {
        let secs = value.as_secs_f64();
        for (bound, bucket) in self.bounds.iter().zip(&self.buckets) {
            if secs <= *bound {
                bucket.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.count.fetch_add(1, Ordering::Relaxed);
        self.sum_micros
            .fetch_add(value.as_micros() as u64, Ordering::Relaxed);
    }

    fn render(&self, out: &mut String, name: &str) {
        for (bound, bucket) in self.bounds.iter().zip(&self.buckets) {
            let _ = writeln!(
                out,
                "{name}_bucket{{le=\"{bound}\"}} {}",
                bucket.load(Ordering::Relaxed)
            );
        }
        let count = self.count.load(Ordering::Relaxed);
        let _ = writeln!(out, "{name}_bucket{{le=\"+Inf\"}} {count}");
        let _ = writeln!(
            out,
            "{name}_sum {}",
            self.sum_micros.load(Ordering::Relaxed) as f64 / 1e6
        );
        let _ = writeln!(out, "{name}_count {count}");
    }
}

/// All metrics of the process.
#[derive(Debug)]
pub struct Metrics {
    pub http_requests: LabeledCounter,
    pub http_latency: Histogram,
    pub llm_tokens: LabeledCounter,
    pub llm_cache_hits: Counter,
    pub llm_cache_misses: Counter,
    pub node_runs: LabeledCounter,
    pub node_cache_hits: Counter,
    pub runs: LabeledCounter,
    pub run_duration: Histogram,
}

impl Default for Metrics {
    fn default() -> Self {
        Metrics {
            http_requests: LabeledCounter::default(),
            http_latency: Histogram::new(&LATENCY_BUCKETS),
            llm_tokens: LabeledCounter::default(),
            llm_cache_hits: Counter::default(),
            llm_cache_misses: Counter::default(),
            node_runs: LabeledCounter::default(),
            node_cache_hits: Counter::default(),
            runs: LabeledCounter::default(),
            run_duration: Histogram::new(&RUN_BUCKETS),
        }
    }
}

impl Metrics {
    /// Prometheus text exposition of every metric.
    pub fn render(&self) -> String {
        let mut out = String::new();
        let header = |out: &mut String, name: &str, kind: &str, help: &str| {
            let _ = writeln!(out, "# HELP {name} {help}\n# TYPE {name} {kind}");
        };
        header(
            &mut out,
            "nexc_http_requests_total",
            "counter",
            "HTTP requests by method, route and status.",
        );
        self.http_requests
            .render(&mut out, "nexc_http_requests_total");
        header(
            &mut out,
            "nexc_http_request_duration_seconds",
            "histogram",
            "HTTP request latency.",
        );
        self.http_latency
            .render(&mut out, "nexc_http_request_duration_seconds");
        header(
            &mut out,
            "nexc_llm_tokens_total",
            "counter",
            "LLM tokens by direction.",
        );
        self.llm_tokens.render(&mut out, "nexc_llm_tokens_total");
        header(
            &mut out,
            "nexc_llm_cache_hits_total",
            "counter",
            "LLM response cache hits.",
        );
        let _ = writeln!(
            out,
            "nexc_llm_cache_hits_total {}",
            self.llm_cache_hits.get()
        );
        header(
            &mut out,
            "nexc_llm_cache_misses_total",
            "counter",
            "LLM response cache misses.",
        );
        let _ = writeln!(
            out,
            "nexc_llm_cache_misses_total {}",
            self.llm_cache_misses.get()
        );
        header(
            &mut out,
            "nexc_node_runs_total",
            "counter",
            "Finished node executions by status.",
        );
        self.node_runs.render(&mut out, "nexc_node_runs_total");
        header(
            &mut out,
            "nexc_node_cache_hits_total",
            "counter",
            "Node results served from the content-hash cache.",
        );
        let _ = writeln!(
            out,
            "nexc_node_cache_hits_total {}",
            self.node_cache_hits.get()
        );
        header(
            &mut out,
            "nexc_runs_total",
            "counter",
            "Finished runs by status.",
        );
        self.runs.render(&mut out, "nexc_runs_total");
        header(
            &mut out,
            "nexc_run_duration_seconds",
            "histogram",
            "Run wall-clock duration.",
        );
        self.run_duration
            .render(&mut out, "nexc_run_duration_seconds");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_exposition_format() {
        let m = Metrics::default();
        m.http_requests.add(
            &[("method", "GET"), ("route", "/a\"b"), ("status", "200")],
            2,
        );
        m.http_latency.observe(Duration::from_millis(30));
        m.llm_cache_hits.add(1);
        let text = m.render();
        assert!(
            text.contains(r#"nexc_http_requests_total{method="GET",route="/a\"b",status="200"} 2"#)
        );
        assert!(text.contains(r#"nexc_http_request_duration_seconds_bucket{le="0.05"} 1"#));
        assert!(text.contains(r#"nexc_http_request_duration_seconds_bucket{le="0.025"} 0"#));
        assert!(text.contains("nexc_http_request_duration_seconds_count 1"));
        assert!(text.contains("nexc_llm_cache_hits_total 1"));
    }
}
