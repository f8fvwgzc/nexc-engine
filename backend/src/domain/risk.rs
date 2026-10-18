//! Behaviour baselines and risk scores. Per agent, a rolling baseline of what
//! one of its runs usually looks like (tool calls per turn, tokens per turn,
//! distinct tools, distinct hosts, spawns, denials, bytes out), updated when a
//! run finishes; the risk of the latest run is the capped sum of how many
//! standard deviations each feature strayed, explained in words. A MinHash of
//! the run's tool sequence says how unlike every earlier run it looked. Pure
//! functions, no IO.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::trace::{Span, SpanKind};
use crate::kernel::{self, Signature};

/// Runs a baseline needs before it scores anything.
pub const MIN_RUNS: i32 = 3;
/// Most standard deviations one feature may add to the score.
pub const Z_CAP: f64 = 3.0;
/// Most the whole score may be.
pub const SCORE_CAP: f64 = 10.0;
/// Signatures kept per agent: the last runs the novelty is measured against.
pub const SIGNATURES_KEPT: usize = 20;
/// How many standard deviations count as unusual.
const NOTABLE_Z: f64 = 2.0;
/// Below this similarity to its closest earlier run, a run is novel.
const NOVEL_BELOW: f64 = 0.2;
/// Mean below which a feature is "usually nothing" and a ratio is not spoken of.
const NEAR_ZERO: f64 = 0.05;

/// The features of one run, in the order they are reported.
pub const FEATURES: [&str; 7] = [
    "tool_calls_per_turn",
    "tokens_per_turn",
    "distinct_tools",
    "distinct_hosts",
    "spawns",
    "denials",
    "bytes_out",
];

/// What one run of an agent looked like.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RunFeatures {
    pub turns: f64,
    pub tool_calls_per_turn: f64,
    pub tokens_per_turn: f64,
    pub distinct_tools: f64,
    pub distinct_hosts: f64,
    pub spawns: f64,
    pub denials: f64,
    pub bytes_out: f64,
    /// The tools called, in order, as one line (`write_file read_file make_docx`).
    #[serde(skip)]
    pub tool_sequence: String,
    #[serde(skip)]
    pub hosts: BTreeSet<String>,
    #[serde(skip)]
    pub tools: BTreeSet<String>,
}

impl RunFeatures {
    fn get(&self, name: &str) -> f64 {
        match name {
            "tool_calls_per_turn" => self.tool_calls_per_turn,
            "tokens_per_turn" => self.tokens_per_turn,
            "distinct_tools" => self.distinct_tools,
            "distinct_hosts" => self.distinct_hosts,
            "spawns" => self.spawns,
            "denials" => self.denials,
            "bytes_out" => self.bytes_out,
            _ => 0.0,
        }
    }
}

/// The features of a run from the spans of one agent in it (`bytes_out`
/// comes from the artifacts and the egress payloads, passed in).
pub fn features(spans: &[Span], bytes_out: i64) -> RunFeatures {
    let mut f = RunFeatures::default();
    let mut tool_calls = 0.0;
    let mut tokens = 0.0;
    let mut sequence: Vec<&str> = Vec::new();
    for span in spans {
        let attrs = &span.attributes;
        match span.kind {
            SpanKind::LlmCall => {
                f.turns += 1.0;
                tokens += attrs
                    .get("tokens_in")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
                    + attrs
                        .get("tokens_out")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
            }
            SpanKind::ToolCall => {
                tool_calls += 1.0;
                let tool = attrs
                    .get("tool")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&span.name);
                f.tools.insert(tool.to_owned());
                sequence.push(tool);
                if let Some(host) = attrs.get("egress_host").and_then(|v| v.as_str()) {
                    f.hosts.insert(host.to_ascii_lowercase());
                }
            }
            SpanKind::Egress => {
                f.hosts.insert(span.name.to_ascii_lowercase());
            }
            SpanKind::Spawn => f.spawns += 1.0,
            SpanKind::Policy => {
                // A refusal, or a refusal the runtime did not make itself (a mismatch).
                if matches!(
                    attrs.get("decision").and_then(|v| v.as_str()),
                    Some("deny" | "mismatch")
                ) {
                    f.denials += 1.0;
                }
            }
            SpanKind::DataRead | SpanKind::Approval => {}
        }
    }
    let turns = f.turns.max(1.0);
    f.tool_calls_per_turn = tool_calls / turns;
    f.tokens_per_turn = tokens / turns;
    f.distinct_tools = f.tools.len() as f64;
    f.distinct_hosts = f.hosts.len() as f64;
    f.bytes_out = bytes_out.max(0) as f64;
    f.tool_sequence = sequence.join(" ");
    f
}

/// The running statistics of one feature (Welford).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Moments {
    pub mean: f64,
    /// Sum of squared deviations from the mean.
    pub m2: f64,
}

impl Moments {
    fn push(&mut self, n: f64, x: f64) {
        let delta = x - self.mean;
        self.mean += delta / n;
        self.m2 += delta * (x - self.mean);
    }

    /// The standard deviation after `n` observations.
    fn stddev(&self, n: f64) -> f64 {
        if n < 2.0 {
            0.0
        } else {
            (self.m2 / (n - 1.0)).sqrt()
        }
    }
}

/// What an agent usually does per run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct Baseline {
    pub runs: i32,
    pub features: BTreeMap<String, Moments>,
    /// MinHash signatures of the tool sequences of the last runs, newest last.
    #[schema(value_type = Vec<Vec<String>>)]
    pub signatures: Vec<Vec<u64>>,
    /// The hosts seen across every run so far.
    #[serde(default)]
    pub hosts: BTreeSet<String>,
    /// The tools called across every run so far.
    #[serde(default)]
    pub tools: BTreeSet<String>,
}

/// How far a run strayed, in numbers and in words.
#[derive(Debug, Clone, Default, PartialEq, Serialize, ToSchema)]
pub struct Risk {
    /// The capped sum of the z-scores, 0 when the baseline is too young.
    pub score: f64,
    /// Each unusual feature, in words.
    pub reasons: Vec<String>,
    /// 1 - the Jaccard similarity to the closest earlier run; 0 for the first runs.
    pub novelty: f64,
    /// The z-score of every feature, for the record.
    pub z: BTreeMap<String, f64>,
}

fn words(name: &str) -> &'static str {
    match name {
        "tool_calls_per_turn" => "tool calls per turn",
        "tokens_per_turn" => "tokens per turn",
        "distinct_tools" => "distinct tools",
        "distinct_hosts" => "distinct hosts",
        "spawns" => "sub-agents",
        "denials" => "policy denials",
        "bytes_out" => "bytes out",
        _ => "",
    }
}

fn ratio(value: f64, mean: f64) -> String {
    let r = value / mean;
    if r >= 10.0 {
        format!("{r:.0}x")
    } else {
        format!("{r:.1}x")
    }
}

/// Scores `run` against `baseline` (before the run is folded in) and says
/// how unlike every earlier run its tool sequence looked.
pub fn score(baseline: &Baseline, run: &RunFeatures) -> Risk {
    let signature = kernel::minhash(&run.tool_sequence);
    let closest = baseline
        .signatures
        .iter()
        .filter_map(|s| <&[u64; kernel::MINHASH_K]>::try_from(s.as_slice()).ok())
        .map(|s| kernel::jaccard(s, &signature))
        .fold(None, |best: Option<f64>, j| {
            Some(best.map_or(j, |b| b.max(j)))
        });
    let novelty = match closest {
        Some(j) if !run.tool_sequence.is_empty() => 1.0 - j,
        _ => 0.0,
    };
    let mut risk = Risk {
        novelty,
        ..Risk::default()
    };
    if baseline.runs < MIN_RUNS {
        return risk;
    }
    let n = f64::from(baseline.runs);
    let mut total = 0.0;
    for name in FEATURES {
        let value = run.get(name);
        let moments = baseline.features.get(name).copied().unwrap_or_default();
        let sd = moments.stddev(n);
        // Features that never varied: anything new is a full cap away.
        let z = if sd < f64::EPSILON {
            if (value - moments.mean).abs() < f64::EPSILON {
                0.0
            } else if value > moments.mean {
                Z_CAP
            } else {
                -Z_CAP
            }
        } else {
            (value - moments.mean) / sd
        };
        risk.z.insert(name.to_owned(), (z * 100.0).round() / 100.0);
        if z >= NOTABLE_Z {
            total += z.min(Z_CAP);
            let what = words(name);
            risk.reasons.push(if moments.mean < NEAR_ZERO {
                format!("{value:.0} {what} where it usually has none")
            } else {
                format!("{} its usual {what}", ratio(value, moments.mean))
            });
        }
    }
    let new_hosts: Vec<&String> = run.hosts.difference(&baseline.hosts).collect();
    if !new_hosts.is_empty() {
        total += (new_hosts.len() as f64).min(Z_CAP);
        risk.reasons.push(format!(
            "{} host{} it never called: {}",
            new_hosts.len(),
            if new_hosts.len() == 1 { "" } else { "s" },
            new_hosts
                .iter()
                .take(3)
                .map(|h| h.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let new_tools: Vec<&String> = run.tools.difference(&baseline.tools).collect();
    if !new_tools.is_empty() {
        total += 1.0;
        risk.reasons.push(format!(
            "{} tool{} it never used: {}",
            new_tools.len(),
            if new_tools.len() == 1 { "" } else { "s" },
            new_tools
                .iter()
                .take(3)
                .map(|t| t.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if novelty > 1.0 - NOVEL_BELOW {
        total += 1.0;
        risk.reasons
            .push("a sequence of tool calls unlike any earlier run".to_owned());
    }
    risk.score = (total.min(SCORE_CAP) * 100.0).round() / 100.0;
    risk
}

impl Baseline {
    /// Folds a finished run in.
    pub fn learn(&mut self, run: &RunFeatures) {
        self.runs += 1;
        let n = f64::from(self.runs);
        for name in FEATURES {
            self.features
                .entry(name.to_owned())
                .or_default()
                .push(n, run.get(name));
        }
        let signature: Signature = kernel::minhash(&run.tool_sequence);
        self.signatures.push(signature.to_vec());
        if self.signatures.len() > SIGNATURES_KEPT {
            let extra = self.signatures.len() - SIGNATURES_KEPT;
            self.signatures.drain(..extra);
        }
        self.hosts.extend(run.hosts.iter().cloned());
        self.tools.extend(run.tools.iter().cloned());
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::domain::trace::SpanStatus;

    fn span(kind: SpanKind, name: &str, attrs: serde_json::Value) -> Span {
        Span {
            id: Uuid::now_v7(),
            run_id: Uuid::nil(),
            node_id: Uuid::nil(),
            agent_id: None,
            principal: "agent://ws/A".into(),
            parent_id: None,
            kind,
            name: name.into(),
            started_at: Utc::now(),
            ended_at: None,
            status: SpanStatus::Ok,
            attributes: attrs,
            seq: 0,
            prev_hash: [0; 32],
            hash: [0; 32],
        }
    }

    fn usual(hosts: &[&str]) -> Vec<Span> {
        let mut spans = vec![
            span(
                SpanKind::LlmCall,
                "m",
                json!({"tokens_in": 100, "tokens_out": 20}),
            ),
            span(
                SpanKind::ToolCall,
                "write_file",
                json!({"tool": "write_file"}),
            ),
            span(
                SpanKind::LlmCall,
                "m",
                json!({"tokens_in": 120, "tokens_out": 30}),
            ),
            span(SpanKind::ToolCall, "finish", json!({"tool": "finish"})),
        ];
        for host in hosts {
            spans.push(span(
                SpanKind::ToolCall,
                "http_get",
                json!({"tool": "http_get", "egress_host": host}),
            ));
        }
        spans
    }

    #[test]
    fn features_count_what_a_run_did() {
        let f = features(&usual(&["api.example.com"]), 2048);
        assert_eq!(f.turns, 2.0);
        assert_eq!(f.tool_calls_per_turn, 1.5);
        assert_eq!(f.tokens_per_turn, 135.0);
        assert_eq!(f.distinct_tools, 3.0);
        assert_eq!(f.distinct_hosts, 1.0);
        assert_eq!(f.bytes_out, 2048.0);
        assert_eq!(f.tool_sequence, "write_file finish http_get");
        let empty = features(&[], 0);
        assert_eq!(empty.turns, 0.0);
        assert_eq!(empty.tool_sequence, "");
    }

    #[test]
    fn a_young_baseline_scores_nothing_and_a_usual_run_scores_low() {
        let mut baseline = Baseline::default();
        let run = features(&usual(&[]), 1000);
        assert_eq!(score(&baseline, &run).score, 0.0);
        for _ in 0..MIN_RUNS {
            baseline.learn(&run);
        }
        assert_eq!(baseline.runs, 3);
        assert_eq!(baseline.signatures.len(), 3);
        let same = score(&baseline, &run);
        assert_eq!(same.score, 0.0, "{same:?}");
        assert!(same.reasons.is_empty());
        assert_eq!(same.novelty, 0.0, "the same sequence is not novel");
    }

    #[test]
    fn five_times_the_usual_egress_and_new_hosts_raise_the_score_in_words() {
        let mut baseline = Baseline::default();
        for bytes in [900, 1000, 1100, 1000, 950] {
            baseline.learn(&features(&usual(&[]), bytes));
        }
        let mut spans = usual(&["a.evil.example", "b.evil.example"]);
        for _ in 0..6 {
            spans.push(span(
                SpanKind::Policy,
                "run_python",
                json!({"decision": "deny"}),
            ));
        }
        let loud = features(&spans, 5_000);
        let risk = score(&baseline, &loud);
        assert!(risk.score > 5.0, "{risk:?}");
        assert!(risk.score <= SCORE_CAP);
        let text = risk.reasons.join("; ");
        assert!(text.contains("its usual bytes out"), "{text}");
        assert!(
            text.contains("2 hosts it never called: a.evil.example, b.evil.example"),
            "{text}"
        );
        assert!(
            text.contains("6 policy denials where it usually has none"),
            "{text}"
        );
        assert!(text.contains("1 tool it never used: http_get"), "{text}");
        assert!(risk.novelty > 0.0, "{risk:?}");
        assert!(risk.z["bytes_out"] > NOTABLE_Z);
        // A quiet run after that is still quiet.
        assert_eq!(score(&baseline, &features(&usual(&[]), 1000)).score, 0.0);
        // The baseline keeps only the last signatures.
        for _ in 0..SIGNATURES_KEPT {
            baseline.learn(&loud);
        }
        assert_eq!(baseline.signatures.len(), SIGNATURES_KEPT);
        assert!(baseline.hosts.contains("a.evil.example"));
    }

    #[test]
    fn an_unseen_tool_sequence_is_novel() {
        let mut baseline = Baseline::default();
        for _ in 0..MIN_RUNS {
            baseline.learn(&features(&usual(&[]), 1000));
        }
        let mut spans = Vec::new();
        for tool in [
            "read_file",
            "list_files",
            "run_python",
            "spawn_subagent",
            "make_docx",
        ] {
            spans.push(span(
                SpanKind::LlmCall,
                "m",
                json!({"tokens_in": 100, "tokens_out": 20}),
            ));
            spans.push(span(SpanKind::ToolCall, tool, json!({"tool": tool})));
        }
        let risk = score(&baseline, &features(&spans, 1000));
        assert!(risk.novelty > 0.8, "{risk:?}");
        assert!(
            risk.reasons
                .iter()
                .any(|r| r.contains("unlike any earlier run")),
            "{risk:?}"
        );
        let roundtrip: Baseline =
            serde_json::from_value(serde_json::to_value(&baseline).unwrap()).unwrap();
        assert_eq!(roundtrip, baseline);
    }
}
