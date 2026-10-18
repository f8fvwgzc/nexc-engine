//! Prometheus metrics maintained with atomics and rendered in the text
//! exposition format (no metrics crate needed).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const LATENCY_BUCKETS: [f64; 10] = [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 10.0];
const RUN_BUCKETS: [f64; 9] = [1.0, 5.0, 15.0, 30.0, 60.0, 120.0, 300.0, 900.0, 3600.0];
const SPAN_BUCKETS: [f64; 10] = [
    0.0001, 0.00025, 0.0005, 0.001, 0.0025, 0.005, 0.01, 0.05, 0.1, 1.0,
];

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

/// Histogram with one label set per series.
#[derive(Debug, Default)]
pub struct LabeledHistogram(Mutex<BTreeMap<String, Histogram>>);

impl LabeledHistogram {
    /// Records one observation on the series identified by `labels`.
    pub fn observe(&self, labels: &[(&str, &str)], value: Duration, bounds: &'static [f64]) {
        let key = labels
            .iter()
            .map(|(k, v)| format!("{k}=\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(",");
        self.0
            .lock()
            .expect("metrics lock")
            .entry(key)
            .or_insert_with(|| Histogram::new(bounds))
            .observe(value);
    }

    fn render(&self, out: &mut String, name: &str) {
        for (labels, histogram) in self.0.lock().expect("metrics lock").iter() {
            for (bound, bucket) in histogram.bounds.iter().zip(&histogram.buckets) {
                let _ = writeln!(
                    out,
                    "{name}_bucket{{{labels},le=\"{bound}\"}} {}",
                    bucket.load(Ordering::Relaxed)
                );
            }
            let count = histogram.count.load(Ordering::Relaxed);
            let _ = writeln!(out, "{name}_bucket{{{labels},le=\"+Inf\"}} {count}");
            let _ = writeln!(
                out,
                "{name}_sum{{{labels}}} {}",
                histogram.sum_micros.load(Ordering::Relaxed) as f64 / 1e6
            );
            let _ = writeln!(out, "{name}_count{{{labels}}} {count}");
        }
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
    /// Actions the agent runtime reported that the policy minted for the run did not allow.
    pub policy_mismatches: Counter,
    /// Alerts raised, by kind.
    pub alerts: LabeledCounter,
    /// Artifacts the DLP classifier kept in.
    pub artifacts_quarantined: Counter,
    pub runs: LabeledCounter,
    pub run_duration: Histogram,
    /// Trace spans recorded, by kind.
    pub spans: LabeledCounter,
    /// From a span being emitted by an executor to its `trace.span` fan-out.
    pub span_fanout: Histogram,
    /// Audit questions answered, by mode (`deterministic`, `llm`).
    pub onto_asks: LabeledCounter,
    /// Decisions the Agent Firewall API handed out, by verdict.
    pub firewall_decisions: LabeledCounter,
    /// Gateway calls performed, by tool kind (`fetch_url`, `mcp`) and outcome.
    pub gateway_calls: LabeledCounter,
    /// Alert deliveries attempted, by destination kind and outcome (`delivered`, `retry`, `failed`).
    pub alert_deliveries: LabeledCounter,
    /// A2A tasks exchanged, by direction (`inbound`, `outbound`) and outcome
    /// (`accepted`, `refused`, `completed`, `failed`).
    pub a2a_tasks: LabeledCounter,
    /// Hot-read cache lookups, by cache and result (`hit`, `miss`).
    pub cache_lookups: LabeledCounter,
    /// Latency of the gRPC calls this backend makes, by service and method.
    pub grpc_client_latency: LabeledHistogram,
    /// The LLM circuit breaker's events, by provider and event
    /// (`opened`, `half_open`, `closed`, `refused`).
    pub llm_breaker: LabeledCounter,
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
            policy_mismatches: Counter::default(),
            alerts: LabeledCounter::default(),
            artifacts_quarantined: Counter::default(),
            runs: LabeledCounter::default(),
            run_duration: Histogram::new(&RUN_BUCKETS),
            spans: LabeledCounter::default(),
            span_fanout: Histogram::new(&SPAN_BUCKETS),
            onto_asks: LabeledCounter::default(),
            firewall_decisions: LabeledCounter::default(),
            gateway_calls: LabeledCounter::default(),
            alert_deliveries: LabeledCounter::default(),
            a2a_tasks: LabeledCounter::default(),
            cache_lookups: LabeledCounter::default(),
            grpc_client_latency: LabeledHistogram::default(),
            llm_breaker: LabeledCounter::default(),
        }
    }
}

impl Metrics {
    /// Records one gRPC call this backend made (`nexc_grpc_client_latency_seconds`).
    pub fn grpc_client(&self, service: &str, method: &str, took: Duration) {
        self.grpc_client_latency.observe(
            &[("service", service), ("method", method)],
            took,
            &LATENCY_BUCKETS,
        );
    }

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
            "nexc_policy_mismatches_total",
            "counter",
            "Actions reported by the agent runtime that the run's policy did not allow.",
        );
        let _ = writeln!(
            out,
            "nexc_policy_mismatches_total {}",
            self.policy_mismatches.get()
        );
        header(
            &mut out,
            "nexc_alerts_total",
            "counter",
            "Alerts raised, by kind.",
        );
        self.alerts.render(&mut out, "nexc_alerts_total");
        header(
            &mut out,
            "nexc_artifacts_quarantined_total",
            "counter",
            "Artifacts the DLP classifier kept in.",
        );
        let _ = writeln!(
            out,
            "nexc_artifacts_quarantined_total {}",
            self.artifacts_quarantined.get()
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
        header(
            &mut out,
            "nexc_trace_spans_total",
            "counter",
            "Trace spans recorded, by kind.",
        );
        self.spans.render(&mut out, "nexc_trace_spans_total");
        header(
            &mut out,
            "nexc_trace_span_fanout_seconds",
            "histogram",
            "Span emitted by an executor to its trace.span fan-out.",
        );
        self.span_fanout
            .render(&mut out, "nexc_trace_span_fanout_seconds");
        header(
            &mut out,
            "nexc_audit_asks_total",
            "counter",
            "Audit questions answered, by mode (deterministic, llm).",
        );
        self.onto_asks.render(&mut out, "nexc_audit_asks_total");
        header(
            &mut out,
            "nexc_firewall_decisions_total",
            "counter",
            "Decisions of the Agent Firewall API, by verdict.",
        );
        self.firewall_decisions
            .render(&mut out, "nexc_firewall_decisions_total");
        header(
            &mut out,
            "nexc_gateway_calls_total",
            "counter",
            "Tools gateway calls, by kind and outcome.",
        );
        self.gateway_calls
            .render(&mut out, "nexc_gateway_calls_total");
        header(
            &mut out,
            "nexc_alert_deliveries_total",
            "counter",
            "Alert deliveries attempted, by destination kind and outcome.",
        );
        self.alert_deliveries
            .render(&mut out, "nexc_alert_deliveries_total");
        header(
            &mut out,
            "nexc_a2a_tasks_total",
            "counter",
            "A2A tasks exchanged, by direction and outcome.",
        );
        self.a2a_tasks.render(&mut out, "nexc_a2a_tasks_total");
        header(
            &mut out,
            "nexc_cache_lookups_total",
            "counter",
            "Hot-read cache lookups, by cache and result (hit, miss).",
        );
        self.cache_lookups
            .render(&mut out, "nexc_cache_lookups_total");
        header(
            &mut out,
            "nexc_grpc_client_latency_seconds",
            "histogram",
            "Latency of the gRPC calls this backend makes, by service and method.",
        );
        self.grpc_client_latency
            .render(&mut out, "nexc_grpc_client_latency_seconds");
        header(
            &mut out,
            "nexc_llm_breaker_total",
            "counter",
            "LLM circuit breaker events, by provider and event (opened, half_open, closed, refused).",
        );
        self.llm_breaker.render(&mut out, "nexc_llm_breaker_total");
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
        m.grpc_client("IntelliService", "Ask", Duration::from_millis(3));
        let text = m.render();
        assert!(text.contains(
            r#"nexc_grpc_client_latency_seconds_bucket{service="IntelliService",method="Ask",le="0.005"} 1"#
        ));
        assert!(text.contains(
            r#"nexc_grpc_client_latency_seconds_count{service="IntelliService",method="Ask"} 1"#
        ));
    }
}
