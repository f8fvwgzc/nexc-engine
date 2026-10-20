//! Approvals asked for through the Agent Firewall. A `require_approval`
//! decision of `AuthorizeAction` is an approval of the approvals engine like
//! a native run's: recorded in `approvals` (its run and node are those the
//! decision is recorded in), put in the approvers' inbox and sent to the
//! alert destinations that take approvals; an approver decides it in the
//! console or through `POST /approvals/{id}/approve|deny`. The agent awaits
//! the decision with `AwaitApproval` on any instance (the table is the
//! source of truth); the instance that asked watches it too, expires it
//! when its time is up and records the decision as an `approval` span on
//! the agent's run open there (the one that asked, or the agent's current
//! decisions session when that one closed meanwhile). The final decision is
//! signed like every other one, from the record alone and the span id the
//! first to record it chose, so every instance hands out the same bytes.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{SecondsFormat, Utc};
use serde_json::json;
use tokio::sync::Semaphore;
use tonic::Status;
use uuid::Uuid;

use super::{Caller, OpenRun, Sessions, Signed, internal, quarantined, versions_to};
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
/// Approvals one agent may have pending at once; asking for more is refused.
pub const PENDING_MAX: i64 = 16;
/// `AwaitApproval` calls one agent may have under way at once.
pub const WAITS_MAX: usize = 4;
/// How often the instance that asked looks at a pending approval.
const WATCH_POLL: Duration = Duration::from_secs(1);
/// How often an `AwaitApproval` looks.
const AWAIT_POLL: Duration = Duration::from_millis(500);
/// How long a decision may go unrecorded (the instance that holds the run
/// records it) before an instance without the run signs it all the same.
const RECORD_GRACE_MS: i64 = 3_000;

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

/// What asking for an approval came to.
pub(super) enum Opened {
    /// Recorded (or the same request, still pending, answers it).
    Asked(Box<Approval>),
    /// Too many of the agent's approvals are pending: the action is refused.
    Refused(Decision),
    /// Not recorded: the decision stays `require_approval` with nothing to
    /// await, and the action may not go.
    Failed,
}

/// Records the approval a decision asks for, tells the approvers and starts
/// watching it. The same request still pending in the same run is answered
/// by that approval; past [`PENDING_MAX`] pending ones, the action is refused.
pub(super) async fn open(state: &AppState, asked: Asked<'_>) -> Opened {
    let caller = asked.caller;
    let db = &state.db;
    match repo::approvals::find_pending_like(
        db,
        asked.run.run_id,
        caller.agent.id,
        asked.tool,
        asked.args_sha256,
    )
    .await
    {
        Ok(Some(same)) => {
            state
                .metrics
                .firewall_approvals
                .add(&[("event", "reused")], 1);
            return Opened::Asked(Box::new(same));
        }
        Ok(None) => {}
        Err(err) => {
            tracing::error!(run_id = %asked.run.run_id, error = %err, "firewall approval not looked up");
            return Opened::Failed;
        }
    }
    match repo::approvals::pending_of_agent(db, caller.agent.id).await {
        Ok(pending) if pending >= PENDING_MAX => {
            state
                .metrics
                .firewall_approvals
                .add(&[("event", "refused")], 1);
            return Opened::Refused(Decision::deny(
                "approvals.pending",
                format!(
                    "{pending} approvals of the agent are still pending; {} needs one more",
                    asked.tool
                ),
            ));
        }
        Ok(_) => {}
        Err(err) => {
            tracing::error!(error = %err, "pending approvals not counted");
            return Opened::Failed;
        }
    }
    let rules = &caller.resolved.rules.require_approval;
    let named = match &asked.decision.effect {
        Effect::RequireApproval { approvers } => approvers.clone(),
        _ => Vec::new(),
    };
    let approvers = if named.is_empty() {
        repo::workspaces::admins(db, caller.workspace_id)
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
        let mut tx = db.begin().await?;
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
            return Opened::Failed;
        }
    };
    state
        .metrics
        .firewall_approvals
        .add(&[("event", "requested")], 1);
    crate::alerting::enqueue_approval(state, &approval, DeliveryEvent::ApprovalRequested).await;
    tokio::spawn(watch(state.clone(), approval.id));
    Opened::Asked(Box::new(approval))
}

/// Watches a pending approval from the instance that asked: expires it when
/// its time is up, records the decision once taken. Ends with the approval
/// (a quarantine expires it; the sweeper expires one whose watcher is gone).
async fn watch(state: AppState, id: Uuid) {
    loop {
        tokio::time::sleep(WATCH_POLL).await;
        let approval = match repo::approvals::find(&state.db, id).await {
            Ok(Some(approval)) => approval,
            Ok(None) => return,
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
        match settle(&state, &approval).await {
            Ok(Some(_)) => return,
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(approval = %id, error = %err, "approval decision not settled; trying again");
            }
        }
    }
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

/// What a decided approval lets the agent do: allow when approved in time,
/// deny when denied, expired or approved after it expired. Built from the
/// record alone.
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
    let late = approval
        .decided_at
        .is_some_and(|at| at > approval.expires_at);
    match approval.status {
        ApprovalStatus::Approved if late => Decision::deny(
            rule,
            format!(
                "{} was approved by {who} after the request expired",
                approval.tool
            ),
        ),
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

/// The final decision on `approval` (no longer pending), signed. On the
/// instance holding the agent's run (the one that asked, else its current
/// decisions session), the first to settle it names the `approval` span
/// and records it there. Elsewhere it is signed with the span the holder
/// named, or, when nobody recorded it within [`RECORD_GRACE_MS`] (the
/// holder is gone), with one of its own that no trace records. `None` while
/// it waits for the holder.
pub(super) async fn settle(
    state: &AppState,
    approval: &Approval,
) -> Result<Option<pb::AuthorizeReply>, sqlx::Error> {
    let sessions = &state.firewall_sessions;
    let target = sessions.by_run(approval.run_id).or_else(|| {
        approval
            .agent_id
            .and_then(|agent| sessions.session_of(agent))
    });
    // The run stays open until the span is emitted.
    let target = match target {
        Some(run) => {
            let gate = run.gate.clone().read_owned().await;
            (!*gate).then_some((run, gate))
        }
        None => None,
    };
    let decided = approval.decided_at.unwrap_or(approval.expires_at);
    let candidate = Uuid::now_v7();
    let span_id =
        if target.is_some() || (Utc::now() - decided).num_milliseconds() >= RECORD_GRACE_MS {
            let mut conn = state.db.acquire().await?;
            repo::approvals::claim_decision_span(&mut conn, approval.id, candidate).await?
        } else {
            approval.decision_span_id
        };
    let Some(span_id) = span_id else {
        return Ok(None);
    };
    let decision = final_decision(approval);
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
        args_sha256: approval.args_sha256.as_deref(),
        kid: state.firewall_signer.kid(),
    }
    .bytes();
    let signature = state.firewall_signer.sign(&signed);
    if let Some((run, _gate)) = &target
        && span_id == candidate
        && run.settled.lock().expect("settled").insert(approval.id)
    {
        record(
            run,
            approval,
            span_id,
            &decision,
            &signature,
            state.firewall_signer.kid(),
        );
    }
    state
        .metrics
        .firewall_approvals
        .add(&[("event", decision.verdict())], 1);
    Ok(Some(pb::AuthorizeReply {
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
        kid: state.firewall_signer.kid().to_owned(),
        approval_id: approval.id.to_string(),
        approval_expires_at: approval
            .expires_at
            .to_rfc3339_opts(SecondsFormat::Micros, true),
    }))
}

/// The decision as an `approval` span of `run`.
fn record(
    run: &OpenRun,
    approval: &Approval,
    span_id: Uuid,
    decision: &Decision,
    signature: &[u8],
    kid: &str,
) {
    let decider = approval.decided_by.as_ref();
    let decided = approval.decided_at.unwrap_or(approval.expires_at);
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
            "request_run_id": approval.run_id,
            "args_sha256": approval.args_sha256,
            "decided_by": decider.and_then(|d| d.id),
            "decided_by_name": decider.map_or("", |d| d.name.as_str()),
            "reason": approval.decision_reason,
            "waited_ms": (decided - approval.requested_at).num_milliseconds(),
            "policy_versions": approval.policy_versions,
            "firewall": true,
            "signature": hex::encode(signature),
            "kid": kid,
        }),
    });
}

/// `AwaitApproval`: the approval, decided or still pending after `wait`.
/// Only the agent that asked sees it, and only approvals it asked for
/// through the firewall; an allow is not handed to an agent quarantined
/// meanwhile. At most [`WAITS_MAX`] calls of one agent wait at once.
pub(super) async fn await_reply(
    state: &AppState,
    sessions: &Sessions,
    caller: &Caller,
    id: Uuid,
    wait: Duration,
) -> Result<pb::ApprovalReply, Status> {
    let permits = sessions
        .waits
        .entry(caller.agent.id)
        .or_insert_with(|| Arc::new(Semaphore::new(WAITS_MAX)))
        .clone();
    let _permit = permits.try_acquire_owned().map_err(|_| {
        Status::resource_exhausted(format!(
            "the agent already awaits {WAITS_MAX} approvals at once"
        ))
    })?;
    let deadline = Instant::now() + wait.min(WAIT_MAX);
    loop {
        let approval = repo::approvals::find_external(&state.db, id, caller.agent.id)
            .await
            .map_err(internal)?
            .filter(|a| a.workspace_id == caller.workspace_id)
            .ok_or_else(|| Status::not_found("approval"))?;
        if let Some(run) = sessions.by_run(approval.run_id) {
            run.touch().await;
        }
        let settled = if approval.status == ApprovalStatus::Pending {
            if Utc::now() > approval.expires_at && expire(state, id).await {
                continue;
            }
            None
        } else {
            settle(state, &approval).await.map_err(internal)?
        };
        if let Some(decision) = settled {
            if decision.decision == "allow" && quarantined(state, caller).await {
                return Err(Status::permission_denied("the agent is quarantined"));
            }
            return Ok(reply(&approval, Some(decision)));
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(reply(&approval, None));
        }
        tokio::time::sleep(AWAIT_POLL.min(deadline - now)).await;
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
            expires_at: Utc::now() + chrono::Duration::seconds(60),
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
        // A yes given after the request ran out lets nothing go.
        let mut late = approval(ApprovalStatus::Approved, "");
        late.decided_at = Some(late.expires_at + chrono::Duration::seconds(1));
        let late = final_decision(&late);
        assert_eq!(late.verdict(), "deny");
        assert!(
            late.reason.contains("after the request expired"),
            "{}",
            late.reason
        );
    }
}
