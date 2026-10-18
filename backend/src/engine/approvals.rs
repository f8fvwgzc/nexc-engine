//! The approval flow on the backend's side. The runtime asks (a `policy` span
//! whose decision is `require_approval`), the backend records the request,
//! pauses the node, tells the approvers, and watches the approvals table
//! until someone decides or the wait runs out; then it hands the decision to
//! the runtime (an `approval_decision` command down the gRPC stream, or
//! `POST /v1/approvals/{id}` on the NDJSON path), records it as an
//! `approval` span and resumes the node. The table is the source of truth,
//! so the handler that takes the decision may run on any backend instance.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::executor::ExecContext;
use crate::domain::approval::{Approval, ApprovalStatus};
use crate::domain::graph::NodeStatus;
use crate::domain::trace::{SpanDraft, SpanKind, SpanStatus};
use crate::realtime::events::LogLevel;
use crate::repo;
use crate::repo::approvals::ApprovalRequest;
use crate::rpc::runtime::{BackendCommand, backend_command};

/// How often a pending approval is looked at.
const POLL: Duration = Duration::from_secs(1);

/// Ids of the approvals granted during one node attempt: a `tool_call` that
/// names one of them ran with permission.
pub type Granted = Arc<Mutex<HashSet<Uuid>>>;

/// How a decision reaches the runtime holding the call.
#[derive(Debug, Clone)]
pub enum Delivery {
    /// `POST /v1/approvals/{id}` with the runtime's bearer token (NDJSON).
    Http,
    /// An `approval_decision` command down the attempt's gRPC stream.
    Stream(mpsc::UnboundedSender<BackendCommand>),
}

/// What the runtime asked for, read from its `policy` span.
#[derive(Debug, Clone)]
pub struct Request {
    pub approval_id: Uuid,
    pub span_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub tool: String,
    pub rule: String,
    pub reason: String,
    pub args_sha256: Option<String>,
}

impl Request {
    /// Reads a request out of a runtime `policy` span, if it is one.
    pub fn from_span(span: &SpanDraft) -> Option<Self> {
        let attrs = &span.attributes;
        if span.kind != SpanKind::Policy || attrs.get("decision")?.as_str()? != "require_approval" {
            return None;
        }
        let text = |key: &str| attrs.get(key).and_then(Value::as_str);
        Some(Request {
            approval_id: text("approval_id")?.parse().ok()?,
            span_id: span.id,
            parent_id: span.parent_id,
            tool: text("subject").unwrap_or(&span.name).to_owned(),
            rule: text("rule").unwrap_or("approvals.tools").to_owned(),
            reason: text("reason").unwrap_or_default().to_owned(),
            args_sha256: text("args_sha256").map(str::to_owned),
        })
    }
}

/// Records the request, pauses the node, tells the approvers and starts
/// watching for the decision. Returns the watcher, or `None` when the
/// request could not be taken (the runtime's call then runs out of time).
pub async fn open(
    ctx: &ExecContext,
    request: Request,
    stop: CancellationToken,
    granted: Granted,
    delivery: Delivery,
) -> Option<JoinHandle<()>> {
    let Some(workspace_id) = ctx.workspace_id else {
        ctx.log(
            LogLevel::Warn,
            "a tool call asked for an approval, but the graph is in no workspace: nobody can decide",
        );
        return None;
    };
    let rules = &ctx.rules.require_approval;
    let approvers = if rules.approvers.is_empty() {
        repo::workspaces::admins(&ctx.state.db, workspace_id)
            .await
            .unwrap_or_default()
    } else {
        rules.approvers.clone()
    };
    let expires_at = Utc::now() + chrono::Duration::seconds(i64::from(rules.timeout_s));
    let principal = ctx.principal();
    let record = ApprovalRequest {
        id: request.approval_id,
        workspace_id,
        run_id: ctx.run_id,
        node_id: ctx.node.id,
        graph_id: ctx.graph_id,
        agent_id: ctx.agent.as_ref().map(|a| a.id),
        principal: &principal,
        span_id: request.span_id,
        tool: &request.tool,
        args_sha256: request.args_sha256.as_deref(),
        rule: &request.rule,
        reason: &request.reason,
        approver_ids: &approvers,
        expires_at,
        policy_versions: &ctx.rules.versions,
    };
    let approval = match repo::approvals::insert(&ctx.state.db, &record).await {
        Ok(approval) => approval,
        Err(err) => {
            // A reused id is the runtime's doing; anything else is ours. Either way the call
            // is not held on the record and the runtime's wait will run out.
            ctx.policy_mismatch(
                request.span_id,
                "approvals",
                format!(
                    "the approval {} could not be recorded: {err}",
                    request.approval_id
                ),
            )
            .await;
            return None;
        }
    };
    if let Err(err) =
        repo::approvals::notify(&ctx.state.db, approval.id, workspace_id, &approvers).await
    {
        tracing::error!(approval = %approval.id, error = %err, "approvers were not told");
    }
    pause(ctx, true).await;
    ctx.log(
        LogLevel::Info,
        format!(
            "{} waits for an approval of {} ({} approver(s) asked; {}s to decide)",
            request.tool,
            principal,
            approvers.len(),
            rules.timeout_s
        ),
    );
    let watch = Watch {
        ctx: ctx.clone_light(),
        approval_id: approval.id,
        request_span_id: request.span_id,
        parent_id: request.parent_id,
        tool: request.tool,
        rule: request.rule,
        requested_at: approval.requested_at,
        stop,
        granted,
        delivery,
    };
    Some(tokio::spawn(watch.run()))
}

/// Flips the node between running and waiting, on the record and on the wire.
async fn pause(ctx: &ExecContext, waiting: bool) {
    let flipped = async {
        let mut conn = ctx.state.db.acquire().await?;
        repo::runs::set_waiting(&mut conn, ctx.run_id, ctx.node.id, waiting).await
    }
    .await;
    match flipped {
        Ok(true) => ctx.publish_status(if waiting {
            NodeStatus::WaitingApproval
        } else {
            NodeStatus::Running
        }),
        Ok(false) => {}
        Err(err) => {
            tracing::error!(run_id = %ctx.run_id, error = %err, "node status not changed for the approval");
        }
    }
}

struct Watch {
    ctx: ExecContext,
    approval_id: Uuid,
    request_span_id: Uuid,
    parent_id: Option<Uuid>,
    tool: String,
    rule: String,
    requested_at: DateTime<Utc>,
    stop: CancellationToken,
    granted: Granted,
    delivery: Delivery,
}

impl Watch {
    async fn run(self) {
        loop {
            tokio::select! {
                _ = self.stop.cancelled() => {
                    // The attempt ended (its result, an error, a cancellation) before anyone decided.
                    if repo::approvals::expire(&self.ctx.state.db, self.approval_id).await.unwrap_or(false) {
                        self.settle(ApprovalStatus::Expired, None).await;
                    }
                    return;
                }
                _ = tokio::time::sleep(POLL) => {}
            }
            let approval = match repo::approvals::find(&self.ctx.state.db, self.approval_id).await {
                Ok(Some(approval)) => approval,
                Ok(None) => return,
                Err(err) => {
                    tracing::warn!(approval = %self.approval_id, error = %err, "approval not read; trying again");
                    continue;
                }
            };
            match approval.status {
                ApprovalStatus::Pending if Utc::now() > approval.expires_at => {
                    if repo::approvals::expire(&self.ctx.state.db, self.approval_id)
                        .await
                        .unwrap_or(false)
                    {
                        self.settle(ApprovalStatus::Expired, None).await;
                        return;
                    }
                }
                ApprovalStatus::Pending => {}
                status => {
                    if status == ApprovalStatus::Approved {
                        self.granted
                            .lock()
                            .expect("granted set")
                            .insert(self.approval_id);
                    }
                    self.deliver(&approval).await;
                    self.settle(status, Some(&approval)).await;
                    return;
                }
            }
        }
    }

    /// Hands the decision to the runtime holding the call.
    async fn deliver(&self, approval: &Approval) {
        let decision = match approval.status {
            ApprovalStatus::Approved => "approved",
            _ => "denied",
        };
        let decided_by = approval.decided_by.as_ref().map_or("", |d| d.name.as_str());
        if let Delivery::Stream(tx) = &self.delivery {
            let command = BackendCommand {
                command: Some(backend_command::Command::ApprovalDecision(
                    crate::rpc::runtime::ApprovalDecision {
                        approval_id: approval.id.to_string(),
                        decision: decision.into(),
                        reason: approval.decision_reason.clone(),
                        decided_by: decided_by.into(),
                    },
                )),
            };
            if tx.send(command).is_err() {
                self.ctx.log(
                    LogLevel::Warn,
                    format!(
                        "the decision on {} found the runtime's stream closed",
                        self.tool
                    ),
                );
            }
            return;
        }
        let settings = &self.ctx.state.settings;
        let url = format!(
            "{}/v1/approvals/{}",
            settings.runtime_url.trim_end_matches('/'),
            approval.id
        );
        let sent = self
            .ctx
            .state
            .http
            .post(&url)
            .bearer_auth(settings.runtime_token.expose())
            .json(&json!({
                "decision": decision,
                "reason": approval.decision_reason,
                "decided_by": decided_by,
            }))
            .send()
            .await;
        match sent {
            Ok(resp) if resp.status().is_success() => {}
            Ok(resp) => self.ctx.log(
                LogLevel::Warn,
                format!(
                    "the runtime did not take the decision on {} (HTTP {}); the call waits until it gives up",
                    self.tool,
                    resp.status()
                ),
            ),
            Err(err) => self.ctx.log(
                LogLevel::Warn,
                format!(
                    "the decision on {} did not reach the runtime: {}",
                    self.tool,
                    err.without_url()
                ),
            ),
        }
    }

    /// Puts the decision on the record (an `approval` span) and resumes the node.
    async fn settle(&self, status: ApprovalStatus, approval: Option<&Approval>) {
        let ctx = &self.ctx;
        let now = Utc::now();
        let mut span = ctx.draft(SpanKind::Approval, self.tool.clone(), self.requested_at);
        span.parent_id = self.parent_id;
        span.ended_at = Some(now);
        span.status = if status == ApprovalStatus::Approved {
            SpanStatus::Ok
        } else {
            SpanStatus::Error
        };
        let decider = approval.and_then(|a| a.decided_by.as_ref());
        span.attributes = json!({
            "approval_id": self.approval_id,
            "decision": status.as_str(),
            "tool": self.tool,
            "rule": self.rule,
            "request_span_id": self.request_span_id,
            "decided_by": decider.and_then(|d| d.id),
            "decided_by_name": decider.map_or("", |d| d.name.as_str()),
            "reason": approval.map_or("", |a| a.decision_reason.as_str()),
            "waited_ms": (now - self.requested_at).num_milliseconds(),
            "policy_version": ctx.policy_version,
            "policy_versions": ctx.rules.versions,
        });
        let span_id = span.id;
        ctx.record(span);
        if let Err(err) =
            repo::approvals::set_decision_span(&ctx.state.db, self.approval_id, span_id).await
        {
            tracing::error!(approval = %self.approval_id, error = %err, "decision span not noted");
        }
        let who = decider.map_or_else(|| "nobody".to_owned(), |d| d.name.clone());
        ctx.log(
            match status {
                ApprovalStatus::Approved => LogLevel::Info,
                _ => LogLevel::Warn,
            },
            format!("{}: {} by {who}", self.tool, status),
        );
        if status != ApprovalStatus::Expired {
            pause(ctx, false).await;
        }
    }
}
