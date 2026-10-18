//! The behaviour baseline at work: when a run finishes, each agent that took
//! part is scored against what it usually does, its baseline learns the run,
//! and the policy's thresholds turn the score into an alert or a quarantine.

use std::collections::{BTreeMap, HashMap};

use uuid::Uuid;

use super::defend::{About, Finding};
use super::policy as policies;
use crate::app::AppState;
use crate::domain::alert::AlertKind;
use crate::domain::audit::AuditAction;
use crate::domain::policy::Rules;
use crate::domain::policy_document::Severity;
use crate::domain::risk::{self, Baseline, Risk, RunFeatures};
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
    let policies = repo::policies::latest_all(&state.db, workspace_id)
        .await
        .unwrap_or_default();
    for (agent_id, spans) in by_agent {
        let Ok(Some(agent)) = repo::agents::find(&state.db, workspace_id, agent_id).await else {
            continue;
        };
        let nodes: HashMap<Uuid, ()> = spans.iter().map(|s| (s.node_id, ())).collect();
        let bytes_out: i64 = artifacts
            .iter()
            .filter(|a| nodes.contains_key(&a.artifact.node_id))
            .map(|a| a.artifact.size)
            .sum::<i64>()
            + spans
                .iter()
                .filter_map(|s| s.attributes.get("payload_bytes").and_then(|v| v.as_i64()))
                .sum::<i64>();
        let run = risk::features(&spans, bytes_out);
        let mut baseline = repo::baselines::find(&state.db, agent_id)
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
        let scored = risk::score(&baseline, &run);
        baseline.learn(&run);
        if let Err(err) = repo::baselines::upsert(&state.db, agent_id, &baseline).await {
            tracing::error!(%agent_id, error = %err, "baseline not saved");
        }
        if let Err(err) =
            repo::agents::set_risk(&state.db, agent_id, scored.score, &scored.reasons).await
        {
            tracing::error!(%agent_id, error = %err, "risk score not saved");
        }
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
            actor: row.owner_id,
        };
        let raised = apply_thresholds(state, &about, &rules, &run, &scored, &baseline).await;
        done.alerts += raised.0;
        done.quarantined += usize::from(raised.1);
    }
    done
}

/// Turns the score into what the policy asks for: `(alerts raised, quarantined)`.
async fn apply_thresholds(
    state: &AppState,
    about: &About<'_>,
    rules: &Rules,
    run: &RunFeatures,
    scored: &Risk,
    baseline: &Baseline,
) -> (usize, bool) {
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
            "risk score {:.1} over {:.1}",
            scored.score,
            thresholds.risk_score.unwrap_or_default()
        );
        state_alert(
            state,
            about,
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
                let cancelled = state.engine.quarantine_local(agent_id);
                let detail = format!(
                    "automatic: risk score {:.1} over {:.1} ({reasons}); policy version {}; {cancelled} running node(s) cancelled here",
                    scored.score,
                    thresholds.quarantine_risk_score.unwrap_or_default(),
                    agent.policy_version
                );
                if let Err(err) = repo::audit::record(
                    &state.db,
                    about.workspace_id,
                    about.actor,
                    AuditAction::AgentQuarantined,
                    Subject::Text(&agent.principal_id),
                    &detail,
                )
                .await
                {
                    tracing::error!(%agent_id, error = %err, "automatic quarantine not audited");
                }
                let title = format!("quarantined: risk score {:.1}", scored.score);
                state_alert(
                    state,
                    about,
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
    severity: Severity,
    rule: &str,
    title: &str,
    detail: &str,
) {
    super::defend::raise(
        state,
        about,
        &Finding {
            span_id: None,
            kind: AlertKind::Risk,
            severity,
            rule,
            title,
            detail,
        },
    )
    .await;
}
