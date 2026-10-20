//! Behaviour baselines and risk scores. Per agent, a rolling baseline of what
//! one of its runs usually looks like (tool calls per turn, tokens per turn,
//! distinct tools, distinct hosts, spawns, denials, bytes out, outbound
//! calls), updated when a run finishes; the risk of the latest run is the
//! capped sum of how many standard deviations each feature strayed,
//! explained in words. A MinHash of the run's tool sequence says how unlike
//! every earlier run it looked. While a run is still under way, its live
//! score ([`score_live`]) watches the counts that only grow, span by span.
//! Pure functions, no IO.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use super::trace::{Span, SpanKind};
use crate::kernel::{self, Signature};

/// Runs a baseline needs before it scores anything.
pub const MIN_RUNS: i32 = 3;
/// Runs a baseline needs before its live score may quarantine anyone: a
/// young baseline whose features never varied scores any change at full
/// cap, which is too eager to stop a run with.
pub const LIVE_MIN_RUNS: i32 = 5;
/// Most bytes one span's `payload_bytes` counts for (reported spans say what
/// they like; a huge value would make the baseline's spread meaningless).
pub const PAYLOAD_BYTES_MAX: i64 = 1 << 30;
/// Most hosts, and most tools, one run adds to its baseline's known sets.
pub const LEARNED_PER_RUN: usize = 64;
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
pub const FEATURES: [&str; 8] = [
    "tool_calls_per_turn",
    "tokens_per_turn",
    "distinct_tools",
    "distinct_hosts",
    "spawns",
    "denials",
    "bytes_out",
    "egress_calls",
];

/// The features the live score watches: counts that only grow as a run
/// goes on, so a partial run never scores above what it will score at its
/// end. (Per-turn ratios rise and fall; novelty needs the whole sequence.)
pub const LIVE_FEATURES: [&str; 6] = [
    "distinct_tools",
    "distinct_hosts",
    "spawns",
    "denials",
    "bytes_out",
    "egress_calls",
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
    /// Calls out of the platform: tool calls naming an `egress_host`,
    /// `egress` spans, and egress an external agent was allowed.
    #[serde(default)]
    pub egress_calls: f64,
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
            "egress_calls" => self.egress_calls,
            _ => 0.0,
        }
    }
}

/// Spans folded into a run's features one at a time: the end-of-run score
/// and the live score count the same way.
#[derive(Debug, Clone, Default)]
pub struct Fold {
    features: RunFeatures,
    tool_calls: f64,
    tokens: f64,
    /// Bytes the spans say went out (`payload_bytes`).
    payload_bytes: i64,
}

/// What folding one span changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pushed {
    /// A count the live score watches moved.
    pub moved: bool,
    /// A host the run had not called before (lower case).
    pub new_host: Option<String>,
    /// A tool the run had not used before.
    pub new_tool: Option<String>,
}

impl Fold {
    /// Folds one span in; what it changed.
    pub fn push(&mut self, kind: SpanKind, name: &str, attrs: &Value) -> Pushed {
        let f = &mut self.features;
        let text = |key: &str| attrs.get(key).and_then(Value::as_str);
        let mut moved = false;
        let (mut new_host, mut new_tool) = (None, None);
        let mut host = |hosts: &mut BTreeSet<String>, host: &str| {
            let host = host.to_ascii_lowercase();
            if !hosts.contains(&host) {
                hosts.insert(host.clone());
                new_host = Some(host);
            }
        };
        let mut tool = |tools: &mut BTreeSet<String>, tool: &str| {
            if !tools.contains(tool) {
                tools.insert(tool.to_owned());
                new_tool = Some(tool.to_owned());
            }
        };
        if let Some(bytes) = attrs
            .get("payload_bytes")
            .and_then(Value::as_i64)
            .filter(|b| *b > 0)
        {
            self.payload_bytes = self
                .payload_bytes
                .saturating_add(bytes.min(PAYLOAD_BYTES_MAX));
            moved = true;
        }
        match kind {
            SpanKind::LlmCall => {
                f.turns += 1.0;
                let tokens = |key: &str| attrs.get(key).and_then(Value::as_f64).unwrap_or(0.0);
                self.tokens += tokens("tokens_in") + tokens("tokens_out");
            }
            SpanKind::ToolCall => {
                self.tool_calls += 1.0;
                let called = text("tool").unwrap_or(name);
                tool(&mut f.tools, called);
                if !f.tool_sequence.is_empty() {
                    f.tool_sequence.push(' ');
                }
                f.tool_sequence.push_str(called);
                if let Some(egress) = text("egress_host") {
                    host(&mut f.hosts, egress);
                    f.egress_calls += 1.0;
                    moved = true;
                }
            }
            SpanKind::Egress => {
                host(&mut f.hosts, name);
                f.egress_calls += 1.0;
                moved = true;
            }
            SpanKind::Spawn => {
                f.spawns += 1.0;
                moved = true;
            }
            SpanKind::Policy => match text("decision") {
                // A refusal, or a refusal the runtime did not make itself (a mismatch).
                Some("deny" | "mismatch") => {
                    f.denials += 1.0;
                    moved = true;
                }
                // What an external agent was allowed to do is what it said it is about to
                // do: the firewall's decisions are all nexc sees of an agent that reports
                // no spans. (An agent that asks and then reports counts the egress twice,
                // the same way in every run, so its baseline absorbs it.)
                Some("allow" | "alert") if attrs.get("firewall") == Some(&Value::Bool(true)) => {
                    match (text("action"), text("subject")) {
                        (Some("egress"), Some(subject)) => {
                            host(&mut f.hosts, subject);
                            f.egress_calls += 1.0;
                            moved = true;
                        }
                        (Some("tool_call"), Some(subject)) => tool(&mut f.tools, subject),
                        _ => {}
                    }
                }
                _ => {}
            },
            SpanKind::DataRead | SpanKind::Approval => {}
        }
        Pushed {
            moved: moved || new_tool.is_some(),
            new_host,
            new_tool,
        }
    }

    /// The counts so far, without the hosts, tools and sequence themselves
    /// (what the live score reads, at no cost that grows with the run).
    pub fn counts(&self) -> RunFeatures {
        let f = &self.features;
        let turns = f.turns.max(1.0);
        RunFeatures {
            turns: f.turns,
            tool_calls_per_turn: self.tool_calls / turns,
            tokens_per_turn: self.tokens / turns,
            distinct_tools: f.tools.len() as f64,
            distinct_hosts: f.hosts.len() as f64,
            spawns: f.spawns,
            denials: f.denials,
            bytes_out: self.payload_bytes.max(0) as f64,
            egress_calls: f.egress_calls,
            ..RunFeatures::default()
        }
    }

    /// The features so far; `bytes_out` is the payloads seen plus
    /// `artifact_bytes` (the files the run delivered).
    pub fn features(&self, artifact_bytes: i64) -> RunFeatures {
        let mut f = self.features.clone();
        let turns = f.turns.max(1.0);
        f.tool_calls_per_turn = self.tool_calls / turns;
        f.tokens_per_turn = self.tokens / turns;
        f.distinct_tools = f.tools.len() as f64;
        f.distinct_hosts = f.hosts.len() as f64;
        f.bytes_out = self.payload_bytes.saturating_add(artifact_bytes).max(0) as f64;
        f
    }
}

/// The features of a run from the spans of one agent in it; `bytes_out`
/// adds `artifact_bytes` (the files it delivered) to the egress payloads.
pub fn features(spans: &[Span], artifact_bytes: i64) -> RunFeatures {
    let mut fold = Fold::default();
    for span in spans {
        fold.push(span.kind, &span.name, &span.attributes);
    }
    fold.features(artifact_bytes)
}

/// The running statistics of one feature (Welford).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Moments {
    pub mean: f64,
    /// Sum of squared deviations from the mean.
    pub m2: f64,
    /// Runs this feature was learned from; 0 in a baseline saved before
    /// features kept their own count, when every feature learned every run.
    #[serde(default)]
    pub n: i32,
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
        "egress_calls" => "outbound calls",
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
    score_with(baseline, run, &FEATURES, true, None)
}

/// Scores a run still under way: the counts that only grow
/// ([`LIVE_FEATURES`]), the hosts and tools it never used before, no
/// novelty. Every part only grows as spans arrive, so the live score of a
/// partial run never exceeds the score the same spans get when the run ends:
/// a run it stops is one the end-of-run score would have flagged too.
pub fn score_live(baseline: &Baseline, run: &RunFeatures) -> Risk {
    score_with(baseline, run, &LIVE_FEATURES, false, None)
}

/// [`score_live`] from a run's counts ([`Fold::counts`]) and the hosts and
/// tools of the run its baseline never saw, kept as they arrived: the cost
/// does not grow with the run.
pub fn score_live_counts(
    baseline: &Baseline,
    counts: &RunFeatures,
    unseen_hosts: &[String],
    unseen_tools: &[String],
) -> Risk {
    score_with(
        baseline,
        counts,
        &LIVE_FEATURES,
        false,
        Some((unseen_hosts, unseen_tools)),
    )
}

fn score_with(
    baseline: &Baseline,
    run: &RunFeatures,
    features: &[&str],
    novelty: bool,
    unseen: Option<(&[String], &[String])>,
) -> Risk {
    let novelty = if novelty {
        let signature = kernel::minhash(&run.tool_sequence);
        let closest = baseline
            .signatures
            .iter()
            .filter_map(|s| <&[u64; kernel::MINHASH_K]>::try_from(s.as_slice()).ok())
            .map(|s| kernel::jaccard(s, &signature))
            .fold(None, |best: Option<f64>, j| {
                Some(best.map_or(j, |b| b.max(j)))
            });
        match closest {
            Some(j) if !run.tool_sequence.is_empty() => 1.0 - j,
            _ => 0.0,
        }
    } else {
        0.0
    };
    let mut risk = Risk {
        novelty,
        ..Risk::default()
    };
    if baseline.runs < MIN_RUNS {
        return risk;
    }
    let mut total = 0.0;
    for &name in features {
        // A feature the baseline has not learned often enough yet says nothing.
        let Some(moments) = baseline.features.get(name).copied() else {
            continue;
        };
        let learned = if moments.n == 0 {
            baseline.runs
        } else {
            moments.n
        };
        if learned < MIN_RUNS {
            continue;
        }
        let value = run.get(name);
        let sd = moments.stddev(f64::from(learned));
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
    let (new_hosts, new_tools): (Vec<&String>, Vec<&String>) = match unseen {
        Some((hosts, tools)) => (hosts.iter().collect(), tools.iter().collect()),
        None => (
            run.hosts.difference(&baseline.hosts).collect(),
            run.tools.difference(&baseline.tools).collect(),
        ),
    };
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
        let before = self.runs;
        self.runs += 1;
        for name in FEATURES {
            let known = self.features.contains_key(name);
            let moments = self.features.entry(name.to_owned()).or_default();
            // A feature saved without its own count learned every run so far.
            let n = if moments.n == 0 && known {
                before
            } else {
                moments.n
            } + 1;
            moments.n = n;
            moments.push(f64::from(n), run.get(name));
        }
        let signature: Signature = kernel::minhash(&run.tool_sequence);
        self.signatures.push(signature.to_vec());
        if self.signatures.len() > SIGNATURES_KEPT {
            let extra = self.signatures.len() - SIGNATURES_KEPT;
            self.signatures.drain(..extra);
        }
        // A run teaches at most so many new hosts and tools: one run cannot make the
        // whole internet "usual".
        let hosts: Vec<String> = run
            .hosts
            .difference(&self.hosts)
            .take(LEARNED_PER_RUN)
            .cloned()
            .collect();
        self.hosts.extend(hosts);
        let tools: Vec<String> = run
            .tools
            .difference(&self.tools)
            .take(LEARNED_PER_RUN)
            .cloned()
            .collect();
        self.tools.extend(tools);
    }
}

/// Which of an agent's baselines a run is scored against and learned into:
/// its runs, or the decision sessions of an external agent (a window of
/// firewall decisions, not a run: the two are not compared with each other).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Runs,
    Sessions,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Runs => "runs",
            Scope::Sessions => "sessions",
        }
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

    #[test]
    fn the_live_score_watches_growing_counts_and_never_exceeds_the_final_one() {
        let mut baseline = Baseline::default();
        for bytes in [900, 1000, 1100, 1000, 950] {
            let mut spans = usual(&["api.partner.example"]);
            spans.push(span(
                SpanKind::ToolCall,
                "http_post",
                json!({"tool": "http_post", "egress_host": "api.partner.example", "payload_bytes": bytes}),
            ));
            baseline.learn(&features(&spans, 0));
        }
        // A run that starts like the usual ones scores nothing as its spans arrive.
        let mut fold = Fold::default();
        for s in usual(&["api.partner.example"]) {
            fold.push(s.kind, &s.name, &s.attributes);
            assert_eq!(score_live(&baseline, &fold.features(0)).score, 0.0);
        }
        // Then it sends a lot, to hosts it never called: the live score climbs.
        let mut scores = Vec::new();
        let mut unseen = Vec::new();
        for i in 0..6 {
            let host = format!("drop{i}.evil.example");
            let pushed = fold.push(
                SpanKind::ToolCall,
                "http_post",
                &json!({"tool": "http_post", "egress_host": host, "payload_bytes": 50_000}),
            );
            assert!(pushed.moved, "an egress moves a watched count");
            assert_eq!(pushed.new_host.as_deref(), Some(host.as_str()));
            unseen.push(host);
            let live = score_live(&baseline, &fold.features(0));
            // Scored from the counts and the unseen hosts kept as they came: the same.
            assert_eq!(
                score_live_counts(&baseline, &fold.counts(), &unseen, &[]),
                live
            );
            scores.push(live.score);
        }
        assert!(scores.windows(2).all(|w| w[0] <= w[1]), "{scores:?}");
        let live = score_live(&baseline, &fold.features(0));
        assert!(live.score >= 7.0, "{live:?}");
        let text = live.reasons.join("; ");
        assert!(text.contains("hosts it never called"), "{text}");
        assert!(text.contains("its usual outbound calls"), "{text}");
        assert!(text.contains("its usual bytes out"), "{text}");
        assert_eq!(live.novelty, 0.0, "the live score has no novelty");
        // The same spans scored when the run ends score at least as high.
        let end = score(&baseline, &fold.features(0));
        assert!(end.score >= live.score, "{end:?} vs {live:?}");
        // A model turn moves nothing the live score watches.
        assert_eq!(
            fold.push(SpanKind::LlmCall, "m", &json!({"tokens_in": 5})),
            Pushed::default()
        );
    }

    #[test]
    fn one_run_teaches_a_bounded_number_of_hosts_and_bytes() {
        let mut spans = Vec::new();
        for i in 0..200 {
            spans.push(span(
                SpanKind::ToolCall,
                "http_post",
                json!({"tool": "http_post", "egress_host": format!("h{i:03}.example"),
                    "payload_bytes": i64::MAX}),
            ));
        }
        let run = features(&spans, 0);
        assert_eq!(run.distinct_hosts, 200.0);
        assert_eq!(run.bytes_out, (PAYLOAD_BYTES_MAX as f64) * 200.0);
        let mut baseline = Baseline::default();
        baseline.learn(&run);
        assert_eq!(baseline.hosts.len(), LEARNED_PER_RUN);
        assert!(baseline.features["bytes_out"].mean.is_finite());
    }

    #[test]
    fn firewall_decisions_count_what_an_external_agent_was_allowed_to_do() {
        let decided = |action: &str, subject: &str, decision: &str| {
            span(
                SpanKind::Policy,
                subject,
                json!({"decision": decision, "action": action, "subject": subject, "firewall": true}),
            )
        };
        let f = features(
            &[
                decided("egress", "API.Partner.example", "allow"),
                decided("egress", "api.partner.example", "alert"),
                decided("tool_call", "send_parcel", "allow"),
                decided("egress", "evil.example", "deny"),
                decided("tool_call", "wire_money", "require_approval"),
                // A native runtime's own decision says nothing about hosts.
                span(
                    SpanKind::Policy,
                    "x",
                    json!({"decision": "allow", "action": "egress", "subject": "native.example"}),
                ),
            ],
            0,
        );
        assert_eq!(f.egress_calls, 2.0);
        assert_eq!(f.distinct_hosts, 1.0);
        assert!(f.hosts.contains("api.partner.example"));
        assert_eq!(f.distinct_tools, 1.0);
        assert_eq!(f.denials, 1.0);
        assert_eq!(f.tool_sequence, "", "decisions are not calls");
    }

    #[test]
    fn a_baseline_saved_before_a_feature_existed_learns_it_before_scoring_it() {
        // Saved by an earlier version: no per-feature count, no `egress_calls`.
        let legacy: Baseline = serde_json::from_value(json!({
            "runs": 5,
            "features": {
                "tool_calls_per_turn": {"mean": 1.0, "m2": 0.0},
                "tokens_per_turn": {"mean": 135.0, "m2": 0.0},
                "distinct_tools": {"mean": 2.0, "m2": 0.0},
                "distinct_hosts": {"mean": 1.0, "m2": 0.0},
                "spawns": {"mean": 0.0, "m2": 0.0},
                "denials": {"mean": 0.0, "m2": 0.0},
                "bytes_out": {"mean": 1000.0, "m2": 0.0}
            },
            "hosts": ["known.example"],
            "tools": ["write_file", "finish"]
        }))
        .unwrap();
        let run = features(&usual(&[]), 1000);
        assert_eq!(score(&legacy, &run).score, 0.0);
        let mut spans = usual(&[]);
        spans.push(span(SpanKind::Egress, "known.example", json!({})));
        let mut baseline = legacy.clone();
        // One outbound call where the old baseline knew nothing of them: not scored yet.
        let risk = score(&baseline, &features(&spans, 1000));
        assert!(!risk.z.contains_key("egress_calls"), "{risk:?}");
        assert_eq!(risk.score, 0.0, "{risk:?}");
        // The legacy features keep their count; the new one starts at one.
        baseline.learn(&run);
        assert_eq!(baseline.runs, 6);
        assert_eq!(baseline.features["bytes_out"].n, 6);
        assert_eq!(baseline.features["egress_calls"].n, 1);
        for _ in 0..2 {
            baseline.learn(&run);
        }
        let risk = score(&baseline, &features(&spans, 1000));
        assert_eq!(risk.z["egress_calls"], Z_CAP, "{risk:?}");
        assert!(
            risk.reasons
                .iter()
                .any(|r| r.contains("1 outbound calls where it usually has none")),
            "{risk:?}"
        );
    }
}
