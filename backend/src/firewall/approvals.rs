//! Approvals asked for through the Agent Firewall. A `require_approval`
//! decision of `AuthorizeAction` is an approval of the approvals engine like
//! a native run's: recorded in `approvals` (its run and node are those the
//! decision is recorded in), put in the approvers' inbox and sent to the
//! alert destinations that take approvals; an approver decides it in the
//! console or through `POST /approvals/{id}/approve|deny`. The agent awaits
//! the decision with `AwaitApproval` on any instance (the table is the
//! source of truth); the instance holding the run watches it too, expires
//! it when its time is up and records it as an `approval` span. The final
//! decision is signed like every other one, from the record alone and the
//! span id the first to settle it chose, so every instance hands out the
//! same signed bytes.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use chrono::{SecondsFormat, Utc};
use serde_json::json;
use tonic::Status;
use uuid::Uuid;

use super::{Caller, OpenRun, Sessions, Signed, internal, versions_to};
use crate::app::AppState;
use crate::domain::alert_routing::DeliveryEvent;
use crate::domain::approval::{Approval, ApprovalStatus};
use crate::domain::policy::{Decision, Effect};
use crate::domain::trace::{SpanDraft, SpanKind, SpanStatus};
use crate::repo;
use crate::repo::approvals::ApprovalRequest;
use crate::rpc::firewall as pb;

/// Longest an `AwaitApproval` waits.
pub const WAIT_MAX: Duration = Duration::from_secs(60);
/// How often the instance holding the run looks at a pending approval.
const WATCH_POLL: Duration = Duration::from_secs(1);
/// How often an `AwaitApproval` looks.
const AWAIT_POLL: Duration = Duration::from_millis(200);

/// What a decision held for a human asks for.
pub(super) struct Asked<'a> {
    pub caller: &'a Caller,
    pub run: &'a Arc<OpenRun>,
    pub principal: &'a str,
    /// The `policy` span that records the decision.
    pub span_id: Uuid,
    pub tool: &'a str,
    pub args_sha256: Option<&'a str>,
    pub decision: &'a Decision,
}

/// Records the approval a decision asks for, tells the approvers and starts
/// watching it. `None` when it could not be recorded: the decision stays
/// `require_approval` with nothing to await, and the action may not go.
pub(super) async fn open(state: &AppState, asked: Asked<'_>) -> Option<Approval> {
    let caller = asked.caller;
    let rules = &caller.resolved.rules.require_approval;
    let named = match &asked.decision.effect {
        Effect::RequireApproval { approvers } => approvers.clone(),
        _ => Vec::new(),
    };
    let approvers = if named.is_empty() {
        repo::workspaces::admins(&state.db, caller.workspace_id)
            .await
            .unwrap_or_default()
    } else {
        named
    };
    let expires_at = Utc::now() + chrono::Duration::seconds(i64::from(rules.timeout_s));
    let record = ApprovalRequest {
        id: Uuid::now_v7(),
        workspace_id: caller.workspace_id,
        run_id: asked.run.run_id,
        node_id: asked.run.node_id,
        graph_id: asked.run.graph_id,
        agent_id: Some(caller.agent.id),
        principal: asked.principal,
        span_id: asked.span_id,
        tool: asked.tool,
        args_sha256: asked.args_sha256,
        rule: asked.decision.rule,
        reason: &asked.decision.reason,
        approver_ids: &approvers,
        expires_at,
        policy_versions: &caller.versions,
    };
    let recorded = async {
        let mut tx = state.db.begin().await?;
        let approval = repo::approvals::insert(&mut *tx, &record).await?;
        repo::approvals::notify(&mut *tx, approval.id, caller.workspace_id, &approvers).await?;
        tx.commit().await?;
        Ok::<Approval, sqlx::Error>(approval)
    }
    .await;
    let approval = match recorded {
        Ok(approval) => approval,
        Err(err) => {
            tracing::error!(run_id = %asked.run.run_id, error = %err, "firewall approval not recorded");
            return None;
        }
    };
    crate::alerting::enqueue_approval(state, &approval, DeliveryEvent::ApprovalRequested).await;
    asked.run.pending.fetch_add(1, Ordering::SeqCst);
    tokio::spawn(watch(state.clone(), asked.run.clone(), approval.id));
    Some(approval)
}

/// Watches a pending approval from the instance holding its run: expires
/// it when its time is up, records the decision once taken. Stops with the
/// run (which settles what was decided when it closes).
async fn watch(state: AppState, run: Arc<OpenRun>, id: Uuid) {
    loop {
        tokio::select! {
            biased;
            _ = run.stop.cancelled() => break,
            _ = tokio::time::sleep(WATCH_POLL) => {}
        }
        let approval = match repo::approvals::find(&state.db, id).await {
            Ok(Some(approval)) => approval,
            Ok(None) => break,
            Err(err) => {
                tracing::warn!(approval = %id, error = %err, "approval not read; trying again");
                continue;
            }
        };
        if approval.status == ApprovalStatus::Pending {
            if Utc::now() > approval.expires_at {
                expire(&state, id).await;
            }
            continue;
        }
        if let Err(err) = settle(&state, Some(&run), &approval).await {
            tracing::warn!(approval = %id, error = %err, "approval decision not settled; trying again");
            continue;
        }
        break;
    }
    run.pending.fetch_sub(1, Ordering::SeqCst);
}

/// Marks a pending approval expired and tells the destinations; whether it did.
async fn expire(state: &AppState, id: Uuid) -> bool {
    match repo::approvals::expire(&state.db, id).await {
        Ok(true) => {
            if let Ok(Some(expired)) = repo::approvals::find(&state.db, id).await {
                crate::alerting::enqueue_approval(state, &expired, DeliveryEvent::ApprovalDecided)
                    .await;
            }
            true
        }
        Ok(false) => false,
        Err(err) => {
            tracing::warn!(approval = %id, error = %err, "approval not expired");
            false
        }
    }
}

/// The rule a final decision names: the one that asked.
fn rule_of(rule: &str) -> &'static str {
    match rule {
        "approvals.labels" => "approvals.labels",
        _ => "approvals.tools",
    }
}

/// What a decided approval lets the agent do: allow when approved, deny
/// when denied or expired. Built from the record alone.
fn final_decision(approval: &Approval) -> Decision {
    let rule = rule_of(&approval.rule);
    let who = approval
        .decided_by
        .as_ref()
        .map_or("nobody", |d| d.name.as_str());
    let said = if approval.decision_reason.is_empty() {
        String::new()
    } else {
        format!(": {}", approval.decision_reason)
    };
    match approval.status {
        ApprovalStatus::Approved => {
            Decision::allow(rule, format!("{} approved by {who}{said}", approval.tool))
        }
        ApprovalStatus::Denied => {
            Decision::deny(rule, format!("{} denied by {who}{said}", approval.tool))
        }
        ApprovalStatus::Expired | ApprovalStatus::Pending => Decision::deny(
            rule,
            format!(
                "{}: nobody decided before the approval expired",
                approval.tool
            ),
        ),
    }
}

/// The final decision on `approval` (no longer pending), signed. The first
/// to settle it names the `approval` span that records it; the instance
/// holding the run (`run`) records that span, once.
pub(super) async fn settle(
    state: &AppState,
    run: Option<&OpenRun>,
    approval: &Approval,
) -> Result<pb::AuthorizeReply, sqlx::Error> {
    let candidate = Uuid::now_v7();
    let span_id = repo::approvals::claim_decision_span(&state.db, approval.id, candidate)
        .await?
        .unwrap_or(candidate);
    let decision = final_decision(approval);
    let decided = approval.decided_at.unwrap_or(approval.expires_at);
    let decided_at = decided.to_rfc3339_opts(SecondsFormat::Micros, true);
    let signed = Signed {
        principal: &approval.principal,
        action: "tool_call",
        subject: &approval.tool,
        labels: &[],
        decision: &decision,
        versions: &approval.policy_versions,
        span_id,
        run_id: approval.run_id,
        decided_at: &decided_at,
        approval_id: Some(approval.id),
    }
    .bytes();
    let signature = state.firewall_signer.sign(&signed);
    if let Some(run) = run.filter(|r| r.run_id == approval.run_id)
        && run.settled.lock().expect("settled").insert(approval.id)
    {
        let decider = approval.decided_by.as_ref();
        run.handle.emit(SpanDraft {
            id: span_id,
            node_id: run.node_id,
            agent_id: approval.agent_id,
            principal: approval.principal.clone(),
            parent_id: None,
            kind: SpanKind::Approval,
            name: approval.tool.clone(),
            started_at: approval.requested_at,
            ended_at: Some(decided),
            status: if decision.allowed {
                SpanStatus::Ok
            } else {
                SpanStatus::Error
            },
            attributes: json!({
                "approval_id": approval.id,
                "decision": approval.status.as_str(),
                "verdict": decision.verdict(),
                "tool": approval.tool,
                "rule": approval.rule,
                "request_span_id": approval.span_id,
                "decided_by": decider.and_then(|d| d.id),
                "decided_by_name": decider.map_or("", |d| d.name.as_str()),
                "reason": approval.decision_reason,
                "waited_ms": (decided - approval.requested_at).num_milliseconds(),
                "policy_versions": approval.policy_versions,
                "firewall": true,
                "signature": hex::encode(&signature),
            }),
        });
    }
    Ok(pb::AuthorizeReply {
        decision: decision.verdict().into(),
        rule: decision.rule.into(),
        reason: decision.reason,
        policy_versions: Some(versions_to(&approval.policy_versions)),
        span_id: span_id.to_string(),
        run_id: approval.run_id.to_string(),
        decided_at,
        signed,
        signature,
        public_key: state.firewall_signer.public_key().to_vec(),
        approval_id: approval.id.to_string(),
        approval_expires_at: approval
            .expires_at
            .to_rfc3339_opts(SecondsFormat::Micros, true),
    })
}

/// Before a run is sealed: what was decided and not yet recorded is
/// recorded; when the run was stopped (its agent quarantined), what is
/// still pending expires, as a quarantined agent's request is no longer
/// for anyone to grant. Other pending requests keep their time: any
/// instance answers them from the record.
pub(super) async fn close_out(state: &AppState, run: &OpenRun, stopped: bool) {
    let approvals = match repo::approvals::list_for_run(&state.db, run.run_id).await {
        Ok(approvals) => approvals,
        Err(err) => {
            tracing::warn!(run_id = %run.run_id, error = %err, "approvals of the run not read");
            return;
        }
    };
    for mut approval in approvals {
        if approval.status == ApprovalStatus::Pending {
            if !stopped || !expire(state, approval.id).await {
                continue;
            }
            match repo::approvals::find(&state.db, approval.id).await {
                Ok(Some(now)) => approval = now,
                _ => continue,
            }
        }
        if run.settled.lock().expect("settled").contains(&approval.id) {
            continue;
        }
        if let Err(err) = settle(state, Some(run), &approval).await {
            tracing::warn!(approval = %approval.id, error = %err, "approval not settled at the run's close");
        }
    }
}

/// `AwaitApproval`: the approval, decided or still pending after `wait`.
/// Only the agent that asked sees it.
pub(super) async fn await_reply(
    state: &AppState,
    sessions: &Sessions,
    caller: &Caller,
    id: Uuid,
    wait: Duration,
) -> Result<pb::ApprovalReply, Status> {
    let deadline = Instant::now() + wait.min(WAIT_MAX);
    loop {
        let approval = repo::approvals::find(&state.db, id)
            .await
            .map_err(internal)?
            .filter(|a| {
                a.agent_id == Some(caller.agent.id) && a.workspace_id == caller.workspace_id
            })
            .ok_or_else(|| Status::not_found("approval"))?;
        let local = sessions.by_run(approval.run_id);
        if let Some(run) = &local {
            run.touch().await;
        }
        let pending = approval.status == ApprovalStatus::Pending;
        if pending && Utc::now() > approval.expires_at {
            if !expire(state, id).await {
                tokio::time::sleep(AWAIT_POLL).await;
            }
            continue;
        }
        if pending {
            let now = Instant::now();
            if now >= deadline {
                return Ok(reply(&approval, None));
            }
            tokio::time::sleep(AWAIT_POLL.min(deadline - now)).await;
            continue;
        }
        let decision = settle(state, local.as_deref(), &approval)
            .await
            .map_err(internal)?;
        return Ok(reply(&approval, Some(decision)));
    }
}

fn reply(approval: &Approval, decision: Option<pb::AuthorizeReply>) -> pb::ApprovalReply {
    let decided = approval.status != ApprovalStatus::Pending;
    pb::ApprovalReply {
        approval_id: approval.id.to_string(),
        status: approval.status.as_str().into(),
        decided_by: approval
            .decided_by
            .as_ref()
            .filter(|_| decided)
            .map(|d| d.name.clone())
            .unwrap_or_default(),
        reason: approval.decision_reason.clone(),
        expires_at: approval
            .expires_at
            .to_rfc3339_opts(SecondsFormat::Micros, true),
        decision,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::approval::Decider;
    use crate::domain::policy::PolicyVersions;

    fn approval(status: ApprovalStatus, reason: &str) -> Approval {
        Approval {
            id: Uuid::now_v7(),
            workspace_id: Uuid::now_v7(),
            run_id: Uuid::now_v7(),
            node_id: Uuid::now_v7(),
            graph_id: Uuid::now_v7(),
            agent_id: None,
            principal: "agent://ws/Courier".into(),
            span_id: Uuid::now_v7(),
            tool: "wire_money".into(),
            args_sha256: None,
            rule: "approvals.tools".into(),
            reason: String::new(),
            approver_ids: Vec::new(),
            status,
            requested_at: Utc::now(),
            expires_at: Utc::now(),
            decided_by: (status != ApprovalStatus::Expired).then(|| Decider {
                id: None,
                name: "Ada".into(),
            }),
            decided_at: Some(Utc::now()),
            decision_reason: reason.into(),
            decision_span_id: None,
            policy_versions: PolicyVersions::default(),
        }
    }

    #[test]
    fn the_final_decision_follows_the_record() {
        let approved = final_decision(&approval(ApprovalStatus::Approved, ""));
        assert_eq!(
            (approved.verdict(), approved.rule),
            ("allow", "approvals.tools")
        );
        assert_eq!(approved.reason, "wire_money approved by Ada");
        let denied = final_decision(&approval(ApprovalStatus::Denied, "not this month"));
        assert_eq!(denied.verdict(), "deny");
        assert_eq!(denied.reason, "wire_money denied by Ada: not this month");
        let expired = final_decision(&approval(ApprovalStatus::Expired, ""));
        assert_eq!(expired.verdict(), "deny");
        assert!(
            expired.reason.contains("nobody decided"),
            "{}",
            expired.reason
        );
        let mut labels = approval(ApprovalStatus::Approved, "");
        labels.rule = "approvals.labels".into();
        assert_eq!(final_decision(&labels).rule, "approvals.labels");
    }
}
