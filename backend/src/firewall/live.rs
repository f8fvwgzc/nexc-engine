//! Live risk on the Agent Firewall: every span and decision of an open run
//! of an external agent is folded into its partial-run features as it
//! arrives, and the counts that only grow (outbound calls, hosts, denials,
//! bytes out, tools, sub-agents) are scored against the agent's baseline
//! ([`risk::score_live`]). When the score crosses the policy's
//! `quarantine_risk_score`, the agent is quarantined and the run cancelled
//! while it is still under way, and the time it took is put on the trace.
//! Folding a span is a few set insertions; a score is taken only when a
//! watched count moved and the policy sets a quarantine threshold.

use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use serde_json::{Value, json};
use tokio::sync::OwnedMutexGuard;
use uuid::Uuid;

use super::{Caller, OpenRun};
use crate::app::AppState;
use crate::domain::risk::{self, Baseline, Fold, Risk, RunFeatures, Scope};
use crate::domain::trace::{SpanDraft, SpanKind, SpanStatus};
use crate::engine::defend::About;
use crate::engine::policy::Resolved;

/// The rule a live containment is recorded under.
pub const RULE: &str = "alerts.quarantine_risk_score";

/// The live score of one open run.
pub struct Live {
    pub scope: Scope,
    baseline: Baseline,
    resolved: Arc<Resolved>,
    fold: Fold,
    /// The span that first raised the score above nothing, and when it arrived.
    first_anomaly: Option<(Uuid, Instant)>,
    /// The score crossed the threshold once: the run is being contained.
    tripped: bool,
    last: Risk,
}

/// A run whose live score just crossed the quarantine threshold.
pub struct Crossing {
    /// The span that took it over.
    pub trigger: Uuid,
    pub detected: Instant,
    pub first_anomaly: (Uuid, Instant),
    pub threshold: f64,
    pub features: RunFeatures,
    pub risk: Risk,
    pub baseline: Baseline,
    pub resolved: Arc<Resolved>,
}

impl Crossing {
    /// The reasons, in words.
    pub fn reasons(&self) -> String {
        if self.risk.reasons.is_empty() {
            "nothing named".to_owned()
        } else {
            self.risk.reasons.join("; ")
        }
    }
}

impl Live {
    pub fn new(scope: Scope, baseline: Baseline, resolved: Arc<Resolved>) -> Self {
        Live {
            scope,
            baseline,
            resolved,
            fold: Fold::default(),
            first_anomaly: None,
            tripped: false,
            last: Risk::default(),
        }
    }

    /// Whether the score crossed the threshold (the run is, or is being, contained).
    pub fn tripped(&self) -> bool {
        self.tripped
    }

    /// The policy in force changed: the threshold follows it.
    pub fn refresh(&mut self, resolved: Arc<Resolved>) {
        self.resolved = resolved;
    }

    fn threshold(&self) -> Option<f64> {
        self.resolved.rules.alerts.quarantine_risk_score
    }

    /// Folds in span `id`; the crossing when it took the live score over
    /// the quarantine threshold (once per run).
    pub fn observe(
        &mut self,
        id: Uuid,
        kind: SpanKind,
        name: &str,
        attrs: &Value,
    ) -> Option<Crossing> {
        let moved = self.fold.push(kind, name, attrs);
        let threshold = self.threshold()?;
        if !moved || self.tripped || self.baseline.runs < risk::MIN_RUNS {
            return None;
        }
        let features = self.fold.features(0);
        let scored = risk::score_live(&self.baseline, &features);
        let now = Instant::now();
        if scored.score > 0.0 && self.first_anomaly.is_none() {
            self.first_anomaly = Some((id, now));
        }
        self.last = scored.clone();
        if scored.score < threshold {
            return None;
        }
        self.tripped = true;
        Some(Crossing {
            trigger: id,
            detected: now,
            first_anomaly: self.first_anomaly.unwrap_or((id, now)),
            threshold,
            features,
            risk: scored,
            baseline: self.baseline.clone(),
            resolved: self.resolved.clone(),
        })
    }

    /// The live score now (taken afresh when no threshold kept it up to date).
    pub fn score(&self) -> f64 {
        if self.threshold().is_some() || self.baseline.runs < risk::MIN_RUNS {
            return self.last.score;
        }
        risk::score_live(&self.baseline, &self.fold.features(0)).score
    }
}

/// What containing a run did, for the run's summary.
#[derive(Debug, Clone)]
pub struct Contained {
    pub score: f64,
    pub reason: String,
    pub time_to_contain_ms: u64,
}

/// Contains a run whose live score crossed the threshold: the agent's risk
/// is noted, the thresholds apply as at the end of a run (alerts, then the
/// quarantine, which cancels whatever the agent runs here, this run
/// included), and a `policy` span `live_risk` records the score, why, the
/// span that tripped it, the first anomalous span and how long it took from
/// that span to the run being stopped. `slot` is the run's containment,
/// held throughout so the run is not sealed before the span is recorded.
pub async fn contain(
    state: &AppState,
    caller: &Caller,
    run: &OpenRun,
    crossing: Crossing,
    mut slot: OwnedMutexGuard<Option<Contained>>,
) -> Contained {
    let span_id = Uuid::now_v7();
    let about = About {
        workspace_id: caller.workspace_id,
        agent_id: Some(caller.agent.id),
        principal: &caller.agent.principal_id,
        run_id: Some(run.run_id),
        node_id: Some(run.node_id),
        graph_id: Some(run.graph_id),
        actor: Some(run.owner),
    };
    let reasons = crossing.reasons();
    let quarantined = crate::engine::risk::contain(
        state,
        &about,
        &crossing.resolved.rules,
        &crossing.features,
        &crossing.risk,
        &crossing.baseline,
        span_id,
    )
    .await;
    // Already quarantined (another run got there first): this run stops all the same.
    run.stop.cancel();
    let stopped = Instant::now();
    let time_to_contain = stopped.duration_since(crossing.first_anomaly.1);
    let detect_to_contain = stopped.duration_since(crossing.detected);
    let reason = format!(
        "live risk score {:.1} over {:.1}: {reasons}",
        crossing.risk.score, crossing.threshold
    );
    let now = Utc::now();
    run.handle.emit(SpanDraft {
        id: span_id,
        node_id: run.node_id,
        agent_id: Some(caller.agent.id),
        principal: caller.agent.principal_id.clone(),
        parent_id: None,
        kind: SpanKind::Policy,
        name: "live_risk".into(),
        started_at: now,
        ended_at: Some(now),
        status: SpanStatus::Error,
        attributes: json!({
            "decision": "quarantine",
            "rule": RULE,
            "reason": reason,
            "action": "live_risk",
            "risk_score": crossing.risk.score,
            "threshold": crossing.threshold,
            "reasons": crossing.risk.reasons,
            "z": crossing.risk.z,
            "trigger_span_id": crossing.trigger,
            "first_anomaly_span_id": crossing.first_anomaly.0,
            "time_to_contain_ms": time_to_contain.as_millis() as u64,
            "detect_to_contain_ms": detect_to_contain.as_millis() as u64,
            "quarantined": quarantined,
            "baseline_runs": crossing.baseline.runs,
            "policy_version": caller.agent.policy_version,
            "policy_versions": caller.versions,
            "firewall": true,
        }),
    });
    state.metrics.firewall_contained.observe(time_to_contain);
    tracing::warn!(
        run_id = %run.run_id,
        agent = %caller.agent.principal_id,
        score = crossing.risk.score,
        time_to_contain_ms = time_to_contain.as_millis() as u64,
        "external run contained mid-run"
    );
    let contained = Contained {
        score: crossing.risk.score,
        reason,
        time_to_contain_ms: time_to_contain.as_millis() as u64,
    };
    *slot = Some(contained.clone());
    contained
}
