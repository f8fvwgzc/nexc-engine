//! The behaviour baseline at work: when a run finishes, each agent that took
//! part is scored against what it usually does, its baseline learns the run,
//! and the policy's thresholds turn the score into an alert or a quarantine.
//! A run of an external agent is also scored live, as its spans arrive
//! (`firewall::live`); crossing `quarantine_risk_score` there goes through
//! [`contain`], the same thresholds, while the run is still under way.

use std::collections::{BTreeMap, HashMap};

use uuid::Uuid;

use super::defend::{About, Finding};
use super::policy as policies;
use crate::app::AppState;
use crate::domain::alert::AlertKind;
use crate::domain::audit::AuditAction;
use crate::domain::policy::Rules;
use crate::domain::policy_document::Severity;
use crate::domain::risk::{self, Baseline, Risk, RunFeatures, Scope};
use crate::domain::trace::Span;
use crate::repo;
use crate::repo::audit::Subject;
use crate::repo::runs::RunRow;

/// What assessing a run did, for the log.
#[derive(Debug, Default)]
pub struct Assessed {
    pub agents: usize,
    pub alerts: usize,
    pub quarantined: usize,
}

/// Scores every agent that worked in the finished run, updates its risk and
/// baseline, and applies the thresholds. Never fails the run.
pub async fn assess(state: &AppState, row: &RunRow) -> Assessed {
    assess_in(state, row, Scope::Runs).await
}

/// [`assess`] against the agents' baselines of `scope`.
pub async fn assess_in(state: &AppState, row: &RunRow, scope: Scope) -> Assessed {
    let mut done = Assessed::default();
    let Some(workspace_id) = repo::graphs::workspace_of(&state.db, row.graph_id)
        .await
        .ok()
        .flatten()
    else {
        return done;
    };
    let spans = match repo::spans::chain(&state.db, row.id).await {
        Ok(spans) => spans,
        Err(err) => {
            tracing::error!(run_id = %row.id, error = %err, "spans not read for the risk score");
            return done;
        }
    };
    let artifacts = repo::artifacts::list_stored(&state.db, row.id)
        .await
        .unwrap_or_default();
    let mut by_agent: BTreeMap<Uuid, Vec<Span>> = BTreeMap::new();
    for span in spans {
        if let Some(agent) = span.agent_id {
            by_agent.entry(agent).or_default().push(span);
        }
    }
    let policies = state
        .caches
        .policies(&state.db, workspace_id)
        .await
        .unwrap_or_default();
    for (agent_id, spans) in by_agent {
        let Ok(Some(agent)) = repo::agents::find(&state.db, workspace_id, agent_id).await else {
            continue;
        };
        let nodes: HashMap<Uuid, ()> = spans.iter().map(|s| (s.node_id, ())).collect();
        // The egress payloads are counted from the spans; the files delivered are added.
        let artifact_bytes: i64 = artifacts
            .iter()
            .filter(|a| nodes.contains_key(&a.artifact.node_id))
            .map(|a| a.artifact.size)
            .sum::<i64>();
        let run = risk::features(&spans, artifact_bytes);
        let mut baseline = repo::baselines::find(&state.db, agent_id, scope)
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
        let scored = risk::score(&baseline, &run);
        baseline.learn(&run);
        if let Err(err) = repo::baselines::upsert(&state.db, agent_id, scope, &baseline).await {
            tracing::error!(%agent_id, error = %err, "baseline not saved");
        }
        if let Err(err) =
            repo::agents::set_risk(&state.db, agent_id, scored.score, &scored.reasons).await
        {
            tracing::error!(%agent_id, error = %err, "risk score not saved");
        }
        state
            .caches
            .invalidate(crate::app::caches::Key::Agent(agent_id));
        done.agents += 1;
        let levels = policies::levels_for(&policies, agent.team_id, Some(agent.id));
        let rules = policies::resolve(&levels, Some(&agent.capabilities)).rules;
        let about = About {
            workspace_id,
            agent_id: Some(agent.id),
            principal: &agent.principal_id,
            run_id: Some(row.id),
            node_id: None,
            graph_id: Some(row.graph_id),
            actor: Some(row.owner_id),
        };
        let raised = apply_thresholds(
            state,
            &about,
            &rules,
            &run,
            &scored,
            &baseline,
            Stage::RunEnd,
        )
        .await;
        done.alerts += raised.0;
        done.quarantined += usize::from(raised.1);
    }
    done
}

/// When a score is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// The run finished.
    RunEnd,
    /// The run is under way; the alerts point at the span that records it.
    Live { span_id: Uuid },
}

/// A run under way crossed the policy's `quarantine_risk_score` on its live
/// score ([`risk::score_live`]): the agent's risk is noted and the same
/// thresholds as at the end of a run apply (alerts, then the quarantine,
/// which stops whatever the agent runs here). `span_id` is the `policy`
/// span the caller records it with. Whether the agent was quarantined (it
/// may have been already).
pub async fn contain(
    state: &AppState,
    about: &About<'_>,
    rules: &Rules,
    run: &RunFeatures,
    scored: &Risk,
    baseline: &Baseline,
    span_id: Uuid,
) -> bool {
    if let Some(agent_id) = about.agent_id {
        if let Err(err) =
            repo::agents::set_risk(&state.db, agent_id, scored.score, &scored.reasons).await
        {
            tracing::error!(%agent_id, error = %err, "live risk score not saved");
        }
        state
            .caches
            .invalidate(crate::app::caches::Key::Agent(agent_id));
    }
    apply_thresholds(
        state,
        about,
        rules,
        run,
        scored,
        baseline,
        Stage::Live { span_id },
    )
    .await
    .1
}

/// Turns the score into what the policy asks for: `(alerts raised, quarantined)`.
async fn apply_thresholds(
    state: &AppState,
    about: &About<'_>,
    rules: &Rules,
    run: &RunFeatures,
    scored: &Risk,
    baseline: &Baseline,
    stage: Stage,
) -> (usize, bool) {
    let span_id = match stage {
        Stage::Live { span_id } => Some(span_id),
        Stage::RunEnd => None,
    };
    let score_name = if span_id.is_some() {
        "live risk score"
    } else {
        "risk score"
    };
    let thresholds = &rules.alerts;
    let mut alerts = 0;
    let reasons = if scored.reasons.is_empty() {
        "nothing unusual".to_owned()
    } else {
        scored.reasons.join("; ")
    };
    let over = |threshold: Option<f64>, value: f64| threshold.is_some_and(|t| value >= t);
    if over(thresholds.risk_score, scored.score) {
        let title = format!(
            "{score_name} {:.1} over {:.1}",
            scored.score,
            thresholds.risk_score.unwrap_or_default()
        );
        state_alert(
            state,
            about,
            span_id,
            Severity::High,
            "alerts.risk_score",
            &title,
            &reasons,
        )
        .await;
        alerts += 1;
    }
    if thresholds
        .denials_per_run
        .is_some_and(|t| run.denials >= f64::from(t))
    {
        let title = format!("{} policy denials in one run", run.denials as i64);
        state_alert(
            state,
            about,
            span_id,
            Severity::Medium,
            "alerts.denials_per_run",
            &title,
            &reasons,
        )
        .await;
        alerts += 1;
    }
    if baseline.runs > risk::MIN_RUNS && over(thresholds.novelty, scored.novelty) {
        let title = format!(
            "a run unlike any earlier one (novelty {:.2})",
            scored.novelty
        );
        state_alert(
            state,
            about,
            span_id,
            Severity::Medium,
            "alerts.novelty",
            &title,
            &reasons,
        )
        .await;
        alerts += 1;
    }
    let mut quarantined = false;
    if over(thresholds.quarantine_risk_score, scored.score)
        && let Some(agent_id) = about.agent_id
    {
        match repo::agents::quarantine(&state.db, about.workspace_id, agent_id).await {
            Ok(Some(agent)) => {
                quarantined = true;
                state
                    .caches
                    .invalidate(crate::app::caches::Key::Agent(agent_id));
                let cancelled = state.engine.quarantine_local(agent_id);
                let detail = format!(
                    "automatic{}: {score_name} {:.1} over {:.1} ({reasons}); policy version {}; {cancelled} running node(s) cancelled here",
                    if span_id.is_some() { ", mid-run" } else { "" },
                    scored.score,
                    thresholds.quarantine_risk_score.unwrap_or_default(),
                    agent.policy_version
                );
                let audited = match about.actor {
                    Some(actor) => {
                        repo::audit::record(
                            &state.db,
                            about.workspace_id,
                            actor,
                            AuditAction::AgentQuarantined,
                            Subject::Text(&agent.principal_id),
                            &detail,
                        )
                        .await
                    }
                    None => {
                        repo::audit::record_system(
                            &state.db,
                            about.workspace_id,
                            AuditAction::AgentQuarantined,
                            &agent.principal_id,
                            &detail,
                        )
                        .await
                    }
                };
                if let Err(err) = audited {
                    tracing::error!(%agent_id, error = %err, "automatic quarantine not audited");
                }
                let title = format!("quarantined: {score_name} {:.1}", scored.score);
                state_alert(
                    state,
                    about,
                    span_id,
                    Severity::Critical,
                    "alerts.quarantine_risk_score",
                    &title,
                    &reasons,
                )
                .await;
                alerts += 1;
            }
            Ok(None) => {}
            Err(err) => tracing::error!(%agent_id, error = %err, "automatic quarantine failed"),
        }
    }
    (alerts, quarantined)
}

async fn state_alert(
    state: &AppState,
    about: &About<'_>,
    span_id: Option<Uuid>,
    severity: Severity,
    rule: &str,
    title: &str,
    detail: &str,
) {
    super::defend::raise(
        state,
        about,
        &Finding {
            span_id,
            kind: AlertKind::Risk,
            severity,
            rule,
            title,
            detail,
        },
    )
    .await;
}
