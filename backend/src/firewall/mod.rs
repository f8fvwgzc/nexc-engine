//! The Agent Firewall API (AgentSecOps phase 4, roadmap §6): a gRPC service
//! on `NEXC_GRPC_ADDR` through which an agent that runs somewhere else uses
//! nexc as its policy and trace service. The agent is a workspace agent of
//! runtime `external` holding an API token; its decisions are taken in
//! process from its resolved policy, signed with the backend's Ed25519 key
//! and recorded as `policy` spans; the runs it reports are chained and
//! sealed like native ones; its card is the registry's. Reflection and
//! `grpc.health.v1` are served beside it.
#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{SecondsFormat, Utc};
use dashmap::DashMap;
use futures::StreamExt;
use serde_json::{Value, json};
use tokio::sync::{Mutex, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tonic::metadata::MetadataMap;
use tonic::{Request, Response, Status, Streaming};
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::agent::{Agent, AgentStatus};
use crate::domain::audit::AuditAction;
use crate::domain::capabilities::DataLabel;
use crate::domain::memory::MemoryScope;
use crate::domain::policy::{self, Action, Decision, PolicyVersions};
use crate::domain::run::RunStatus;
use crate::domain::trace::{self as trace_domain, SpanDraft, SpanKind, SpanStatus};
use crate::engine::executor::agent::{Progress, recheck_with, span_from_wire};
use crate::engine::policy::{Resolved, levels_for, versions_of};
use crate::engine::trace::{self, TraceHandle, TraceSink};
use crate::engine::{approvals::Delivery, scheduler};
use crate::ontology::json;
use crate::realtime::events::SseEvent;
use crate::repo;
use crate::repo::audit::Subject;
use crate::rpc::firewall::agent_firewall_server::{AgentFirewall, AgentFirewallServer};
use crate::rpc::firewall::{self as pb};
use crate::security::api_token;

/// A decisions session is sealed after this long without a call.
pub const SESSION_IDLE: Duration = Duration::from_secs(600);
/// Spans accepted on one reported run.
const SPANS_PER_RUN_MAX: usize = 20_000;
/// Longest string taken from a request.
const TEXT_MAX: usize = 1024;

/// Who is calling: the external agent its token names, and its policy.
#[derive(Clone)]
pub struct Caller {
    pub agent: Agent,
    pub workspace_id: Uuid,
    pub resolved: Arc<Resolved>,
    pub versions: PolicyVersions,
}

impl Caller {
    fn decide(&self, labels: &[DataLabel], action: &Action<'_>) -> Decision {
        policy::decide_with(
            &self.resolved.capabilities,
            &self.resolved.rules,
            labels,
            action,
        )
    }
}

/// An open run of an external agent on this instance: the session its
/// decisions go to, or a run being reported.
pub struct OpenRun {
    pub run_id: Uuid,
    pub node_id: Uuid,
    pub graph_id: Uuid,
    handle: TraceHandle,
    sink: Mutex<Option<TraceSink>>,
    last_used: Mutex<Instant>,
}

/// The decisions sessions open on this instance, by agent.
#[derive(Default)]
pub struct Sessions {
    open: DashMap<Uuid, Arc<OpenRun>>,
}

impl Sessions {
    /// Seals the sessions idle for `idle` or more; returns how many.
    pub async fn seal_idle(&self, state: &AppState, idle: Duration) -> usize {
        let stale: Vec<(Uuid, Arc<OpenRun>)> = self
            .open
            .iter()
            .filter(|e| {
                e.value()
                    .last_used
                    .try_lock()
                    .is_ok_and(|at| at.elapsed() >= idle)
            })
            .map(|e| (*e.key(), e.value().clone()))
            .collect();
        let mut sealed = 0;
        for (agent, run) in stale {
            self.open.remove(&agent);
            close_run(state, &run, RunStatus::Succeeded, None).await;
            sealed += 1;
        }
        sealed
    }

    /// Seals every open session (shutdown).
    pub async fn seal_all(&self, state: &AppState) {
        self.seal_idle(state, Duration::ZERO).await;
    }
}

/// Opens a run of an external agent in its managed graph.
async fn open_run(
    state: &AppState,
    caller: &Caller,
    title: &str,
    external_run_id: &str,
) -> anyhow::Result<Arc<OpenRun>> {
    let mut tx = state.db.begin().await?;
    let (graph_id, owner) = repo::external_runs::ensure_graph(
        &mut tx,
        caller.agent.id,
        caller.workspace_id,
        &caller.agent.name,
    )
    .await?;
    let (run_id, node_id) = repo::external_runs::open_run(
        &mut tx,
        graph_id,
        owner,
        caller.agent.id,
        &caller.agent.role,
        title,
        external_run_id,
    )
    .await?;
    repo::runs::set_policy_versions(&mut *tx, run_id, &caller.versions).await?;
    tx.commit().await?;
    let sink = trace::start(state.clone(), run_id, graph_id);
    let handle = sink.handle();
    if let Ok(run) = scheduler::load_run(state, run_id).await {
        state.hub.publish(graph_id, SseEvent::RunStarted { run });
    }
    Ok(Arc::new(OpenRun {
        run_id,
        node_id,
        graph_id,
        handle,
        sink: Mutex::new(Some(sink)),
        last_used: Mutex::new(Instant::now()),
    }))
}

/// Seals a run's trace and closes it; returns the seal, once.
async fn close_run(
    state: &AppState,
    run: &OpenRun,
    status: RunStatus,
    output: Option<&str>,
) -> Option<trace::Sealed> {
    let sink = run.sink.lock().await.take()?;
    let sealed = sink.seal().await;
    let hash = sealed.hash;
    if let Err(err) = repo::external_runs::close_run(
        &state.db,
        run.run_id,
        run.node_id,
        status,
        output,
        hash.as_ref().map(|h| h.as_slice()),
    )
    .await
    {
        tracing::error!(run_id = %run.run_id, error = %err, "external run not closed");
    }
    if let (Some(hash), Ok(Some(row))) = (hash, repo::runs::find_row(&state.db, run.run_id).await) {
        scheduler::seal_audit(state, &row, hash, sealed.spans).await;
    }
    if let Ok(loaded) = scheduler::load_run(state, run.run_id).await {
        state
            .hub
            .publish(run.graph_id, SseEvent::RunFinished { run: loaded });
    }
    Some(sealed)
}

/// The service.
#[derive(Clone)]
pub struct FirewallService {
    state: AppState,
    sessions: Arc<Sessions>,
}

impl FirewallService {
    pub fn new(state: AppState) -> Self {
        FirewallService {
            sessions: state.firewall_sessions.clone(),
            state,
        }
    }

    /// The caller a request's bearer token names. The token and the agent's
    /// status are looked up on every call (a revoked token or a quarantine
    /// takes effect at once); the resolved policy comes from the hot-read
    /// cache, dropped on every replica the moment a policy or the agent is
    /// written.
    async fn authenticate(&self, metadata: &MetadataMap) -> Result<Caller, Status> {
        let presented = metadata
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .ok_or_else(|| Status::unauthenticated("an agent API token is required"))?;
        if !api_token::looks_like_token(presented) {
            return Err(Status::unauthenticated("invalid agent API token"));
        }
        let hash = api_token::hash(presented);
        let agent = repo::agent_tokens::authenticate(&self.state.db, &hash)
            .await
            .map_err(internal)?
            .ok_or_else(|| Status::unauthenticated("invalid agent API token"))?;
        if agent.status == AgentStatus::Quarantined {
            return Err(Status::permission_denied("the agent is quarantined"));
        }
        if agent.status == AgentStatus::Paused {
            return Err(Status::permission_denied("the agent is paused"));
        }
        // `authenticate` only returns agents of a workspace; the principal names it.
        let Some(workspace_id) = repo_workspace(&agent) else {
            return Err(Status::permission_denied("the agent is in no workspace"));
        };
        let effective = self
            .state
            .caches
            .resolved(&self.state.db, workspace_id, &agent)
            .await
            .map_err(internal)?;
        Ok(Caller {
            agent,
            workspace_id,
            resolved: effective.resolved.clone(),
            versions: effective.versions.clone(),
        })
    }

    /// The agent's decisions session on this instance, opened on demand.
    async fn session(&self, caller: &Caller) -> Result<Arc<OpenRun>, Status> {
        if let Some(open) = self.sessions.open.get(&caller.agent.id) {
            *open.last_used.lock().await = Instant::now();
            return Ok(open.clone());
        }
        let run = open_run(
            &self.state,
            caller,
            "Firewall decisions",
            &format!("session-{}", Uuid::now_v7()),
        )
        .await
        .map_err(|e| internal(anyhow::anyhow!("{e}")))?;
        self.sessions.open.insert(caller.agent.id, run.clone());
        Ok(run)
    }
}

fn repo_workspace(agent: &Agent) -> Option<Uuid> {
    agent
        .principal_id
        .strip_prefix("agent://")
        .and_then(|rest| {
            rest.split_once('/')
                .and_then(|(workspace, _)| workspace.parse().ok())
        })
}

fn internal(err: impl std::fmt::Display) -> Status {
    tracing::error!(error = %err, "firewall request failed");
    Status::internal("the request could not be served")
}

fn bounded(text: &str) -> String {
    text.chars().take(TEXT_MAX).collect()
}

fn versions_to(versions: &PolicyVersions) -> pb::PolicyVersions {
    pb::PolicyVersions {
        workspace: versions.workspace.unwrap_or(0),
        team: versions.team.unwrap_or(0),
        agent: versions.agent.unwrap_or(0),
    }
}

/// Whether `reported` is the caller's principal or a sub-agent under it.
fn acting_principal(caller: &Caller, reported: &str) -> Option<String> {
    let own = caller.agent.principal_id.as_str();
    let reported = if reported.is_empty() { own } else { reported };
    (reported == own
        || reported
            .strip_prefix(own)
            .is_some_and(|rest| rest.starts_with('/') && rest.len() > 1))
    .then(|| bounded(reported))
}

/// What a decision is signed over.
pub struct Signed<'a> {
    pub principal: &'a str,
    pub action: &'a str,
    pub subject: &'a str,
    pub labels: &'a [String],
    pub decision: &'a Decision,
    pub versions: &'a PolicyVersions,
    pub span_id: Uuid,
    pub run_id: Uuid,
    /// RFC 3339, UTC, microseconds.
    pub decided_at: &'a str,
}

impl Signed<'_> {
    /// The bytes the signature covers: canonical JSON of what was asked,
    /// what was decided, under which policy versions, where it is recorded
    /// and when.
    pub fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "action": self.action,
            "decided_at": self.decided_at,
            "decision": self.decision.verdict(),
            "labels": self.labels,
            "policy_versions": self.versions,
            "principal": self.principal,
            "reason": self.decision.reason,
            "rule": self.decision.rule,
            "run_id": self.run_id,
            "span_id": self.span_id,
            "subject": self.subject,
        }))
        .expect("a decision serialises")
    }
}

type SpanReportStream = Streaming<pb::SpanReport>;

#[tonic::async_trait]
impl AgentFirewall for FirewallService {
    async fn authorize_action(
        &self,
        request: Request<pb::AuthorizeRequest>,
    ) -> Result<Response<pb::AuthorizeReply>, Status> {
        let caller = self.authenticate(request.metadata()).await?;
        let req = request.into_inner();
        let principal = acting_principal(&caller, &req.principal).ok_or_else(|| {
            Status::permission_denied(format!(
                "{} is not {} or a sub-agent of it",
                bounded(&req.principal),
                caller.agent.principal_id
            ))
        })?;
        let subject = bounded(&req.subject);
        let labels: Vec<DataLabel> = req.labels.iter().filter_map(|l| l.parse().ok()).collect();
        let label_names: Vec<String> = labels.iter().map(|l| l.as_str().to_owned()).collect();
        let context = req
            .context
            .as_ref()
            .map(json::from_struct)
            .unwrap_or(Value::Null);
        let small = |key: &str| {
            context
                .get(key)
                .and_then(Value::as_u64)
                .map_or(0, |n| n.min(255) as u8)
        };
        let action = match req.action.as_str() {
            "tool_call" => Action::Tool(&subject),
            "egress" => Action::Egress(&subject),
            "model" => Action::Model(&subject),
            "spawn" => Action::Spawn {
                role: &subject,
                depth: small("depth").max(1),
                children: small("children"),
            },
            "data_read" => Action::Read {
                label: subject.parse().map_err(|_| {
                    Status::invalid_argument("data_read takes a classification as its subject")
                })?,
                topic: context
                    .get("topic")
                    .and_then(Value::as_str)
                    .and_then(|t| t.parse().ok()),
                namespace: context
                    .get("namespace")
                    .and_then(Value::as_str)
                    .and_then(|n| n.parse::<MemoryScope>().ok()),
            },
            other => {
                return Err(Status::invalid_argument(format!(
                    "unknown action `{}`; one of tool_call, egress, model, spawn, data_read",
                    bounded(other)
                )));
            }
        };
        if subject.is_empty() {
            return Err(Status::invalid_argument("the subject is empty"));
        }
        let decision = caller.decide(&labels, &action);
        // The run the decision is recorded in: one the agent reported, else its session.
        let run = match context
            .get("run_id")
            .and_then(Value::as_str)
            .and_then(|r| r.parse::<Uuid>().ok())
        {
            Some(run_id) => self
                .sessions
                .open
                .iter()
                .find(|e| e.value().run_id == run_id)
                .map(|e| e.value().clone()),
            None => None,
        };
        let run = match run {
            Some(run) => run,
            None => self.session(&caller).await?,
        };
        let now = Utc::now();
        let decided_at = now.to_rfc3339_opts(SecondsFormat::Micros, true);
        let span_id = Uuid::now_v7();
        let signed = Signed {
            principal: &principal,
            action: &req.action,
            subject: &subject,
            labels: &label_names,
            decision: &decision,
            versions: &caller.versions,
            span_id,
            run_id: run.run_id,
            decided_at: &decided_at,
        }
        .bytes();
        let signature = self.state.firewall_signer.sign(&signed);
        run.handle.emit(SpanDraft {
            id: span_id,
            node_id: run.node_id,
            agent_id: Some(caller.agent.id),
            principal: principal.clone(),
            parent_id: None,
            kind: SpanKind::Policy,
            name: subject.clone(),
            started_at: now,
            ended_at: Some(now),
            status: if decision.allowed {
                SpanStatus::Ok
            } else {
                SpanStatus::Error
            },
            attributes: json!({
                "decision": decision.verdict(),
                "rule": decision.rule,
                "reason": decision.reason,
                "action": req.action,
                "subject": subject,
                "labels": label_names,
                "policy_version": caller.agent.policy_version,
                "policy_versions": caller.versions,
                "firewall": true,
                "signature": hex::encode(&signature),
            }),
        });
        self.state
            .metrics
            .firewall_decisions
            .add(&[("decision", decision.verdict())], 1);
        Ok(Response::new(pb::AuthorizeReply {
            decision: decision.verdict().into(),
            rule: decision.rule.into(),
            reason: decision.reason,
            policy_versions: Some(versions_to(&caller.versions)),
            span_id: span_id.to_string(),
            run_id: run.run_id.to_string(),
            decided_at,
            signed,
            signature,
            public_key: self.state.firewall_signer.public_key().to_vec(),
        }))
    }

    async fn report_span(
        &self,
        request: Request<SpanReportStream>,
    ) -> Result<Response<pb::ReportSummary>, Status> {
        let caller = self.authenticate(request.metadata()).await?;
        let mut stream = request.into_inner();
        let first = stream
            .next()
            .await
            .ok_or_else(|| Status::invalid_argument("the stream opened with no message"))?
            .map_err(|s| Status::invalid_argument(format!("the stream failed: {}", s.message())))?;
        let Some(pb::span_report::Report::Start(start)) = first.report else {
            return Err(Status::invalid_argument(
                "the first message of the stream must be `start`",
            ));
        };
        let title = if start.title.trim().is_empty() {
            "External run".to_owned()
        } else {
            bounded(start.title.trim())
        };
        let run = open_run(
            &self.state,
            &caller,
            &title,
            &bounded(&start.external_run_id),
        )
        .await
        .map_err(internal)?;
        self.sessions.open.insert(Uuid::now_v7(), run.clone());
        let mut progress = Progress::new(Delivery::Http);
        let mut spans = 0usize;
        let mut mismatches = 0u32;
        let mut status = RunStatus::Succeeded;
        let mut output: Option<String> = None;
        let own = caller.agent.principal_id.clone();
        let decide = |action: &Action<'_>| caller.decide(&[], action);
        while let Some(next) = stream.next().await {
            let report = match next {
                Ok(report) => report,
                Err(err) => {
                    status = RunStatus::Failed;
                    output = Some(format!("the stream failed: {}", err.message()));
                    break;
                }
            };
            match report.report {
                Some(pb::span_report::Report::Span(span)) => {
                    if spans >= SPANS_PER_RUN_MAX {
                        continue;
                    }
                    let wire = span_to_wire(span);
                    let bytes = Value::Object(wire.clone()).to_string().len();
                    match span_from_wire(
                        &wire,
                        run.node_id,
                        Some(caller.agent.id),
                        &own,
                        bytes,
                        &mut progress,
                    ) {
                        Ok(draft) => {
                            spans += 1;
                            let mismatch = recheck_with(&decide, &own, &mut progress, &draft);
                            let span_id = draft.id;
                            run.handle.emit(draft);
                            if let Some((rule, detail)) = mismatch {
                                mismatches += 1;
                                record_mismatch(&self.state, &caller, &run, span_id, rule, detail)
                                    .await;
                            }
                        }
                        Err(warning) => tracing::debug!(%warning, "external span ignored"),
                    }
                }
                Some(pb::span_report::Report::Finish(finish)) => {
                    status = match finish.status.as_str() {
                        "failed" => RunStatus::Failed,
                        "cancelled" => RunStatus::Cancelled,
                        _ => RunStatus::Succeeded,
                    };
                    if !finish.output.is_empty() {
                        output = Some(finish.output.chars().take(64 * 1024).collect());
                    }
                    break;
                }
                Some(pb::span_report::Report::Start(_)) | None => {}
            }
        }
        self.sessions.open.retain(|_, r| !Arc::ptr_eq(r, &run));
        let sealed = close_run(&self.state, &run, status, output.as_deref())
            .await
            .unwrap_or(trace::Sealed {
                hash: None,
                spans: 0,
            });
        Ok(Response::new(pb::ReportSummary {
            run_id: run.run_id.to_string(),
            spans: sealed.spans.max(0) as u64,
            trace_hash: sealed.hash.map(hex::encode).unwrap_or_default(),
            policy_mismatches: mismatches,
        }))
    }

    async fn verify_trace(
        &self,
        request: Request<pb::VerifyRequest>,
    ) -> Result<Response<pb::VerifyReply>, Status> {
        let caller = self.authenticate(request.metadata()).await?;
        let run_id: Uuid = request
            .into_inner()
            .run_id
            .parse()
            .map_err(|_| Status::invalid_argument("run_id is not a UUID"))?;
        let row = repo::runs::find_row(&self.state.db, run_id)
            .await
            .map_err(internal)?
            .ok_or_else(|| Status::not_found("run"))?;
        let workspace = repo::graphs::workspace_of(&self.state.db, row.graph_id)
            .await
            .map_err(internal)?;
        if workspace != Some(caller.workspace_id) {
            return Err(Status::not_found("run"));
        }
        let sealed: Option<[u8; 32]> = row
            .trace_hash
            .as_deref()
            .and_then(|bytes| bytes.try_into().ok());
        let spans = repo::spans::chain(&self.state.db, run_id)
            .await
            .map_err(internal)?;
        let verified = tokio::task::spawn_blocking(move || trace_domain::verify(&spans, sealed))
            .await
            .map_err(internal)?;
        let broken = verified.broken.as_ref();
        Ok(Response::new(pb::VerifyReply {
            ok: verified.ok,
            spans: verified.spans,
            hash: verified.hash.unwrap_or_default(),
            sealed_hash: verified.sealed_hash.unwrap_or_default(),
            broken_span_id: broken.map(|b| b.span_id.to_string()).unwrap_or_default(),
            broken_seq: broken.map_or(0, |b| b.seq),
            broken_reason: broken.map(|b| b.reason.clone()).unwrap_or_default(),
        }))
    }

    async fn get_agent_card(
        &self,
        request: Request<pb::AgentCardRequest>,
    ) -> Result<Response<pb::AgentCardReply>, Status> {
        let caller = self.authenticate(request.metadata()).await?;
        let wanted = request.into_inner().principal;
        let (agent, versions) = if wanted.is_empty() || wanted == caller.agent.principal_id {
            (caller.agent.clone(), caller.versions.clone())
        } else {
            let agent =
                repo::agents::find_by_principal(&self.state.db, caller.workspace_id, &wanted)
                    .await
                    .map_err(internal)?
                    .ok_or_else(|| Status::not_found("agent"))?;
            let policies = self
                .state
                .caches
                .policies(&self.state.db, caller.workspace_id)
                .await
                .map_err(internal)?;
            let versions = versions_of(&levels_for(&policies, agent.team_id, Some(agent.id)));
            (agent, versions)
        };
        let owner = repo::agents::owner(&self.state.db, agent.owner_user_id)
            .await
            .map_err(internal)?;
        let team = repo::agents::team(&self.state.db, agent.team_id)
            .await
            .map_err(internal)?;
        Ok(Response::new(pb::AgentCardReply {
            id: agent.id.to_string(),
            principal: agent.principal_id.clone(),
            name: agent.name.clone(),
            role: agent.role.clone(),
            purpose: agent.purpose.clone(),
            status: agent.status.as_str().into(),
            runtime: agent.runtime.as_str().into(),
            workspace_id: caller.workspace_id.to_string(),
            owner: owner.map(|o| o.name).unwrap_or_default(),
            team: team.map(|t| t.name).unwrap_or_default(),
            budget_tokens: agent.budget_tokens,
            budget_left: agent.budget_left().unwrap_or(-1),
            policy_version: agent.policy_version,
            policy_versions: Some(versions_to(&versions)),
            risk_score: agent.risk_score,
            risk_reasons: agent.risk_reasons.clone(),
            capabilities: Some(json::to_struct(
                &serde_json::to_value(&agent.capabilities).unwrap_or_default(),
            )),
        }))
    }
}

/// A reported span as the map the runtime's reader works on.
fn span_to_wire(span: pb::Span) -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    let text = |value: String| {
        if value.is_empty() {
            Value::Null
        } else {
            Value::String(value)
        }
    };
    map.insert("id".into(), text(span.id));
    map.insert("parent_id".into(), text(span.parent_id));
    map.insert("kind".into(), Value::String(span.kind));
    map.insert("name".into(), Value::String(span.name));
    map.insert("agent".into(), text(span.agent));
    map.insert("started_at".into(), text(span.started_at));
    map.insert("ended_at".into(), text(span.ended_at));
    map.insert("status".into(), Value::String(span.status));
    map.insert(
        "attributes".into(),
        span.attributes
            .as_ref()
            .map(json::from_struct)
            .unwrap_or(Value::Null),
    );
    map
}

/// An external agent reported what its policy does not allow: a `policy`
/// span of the run, a count on it, an audit entry.
async fn record_mismatch(
    state: &AppState,
    caller: &Caller,
    run: &OpenRun,
    span_id: Uuid,
    rule: &'static str,
    detail: String,
) {
    let now = Utc::now();
    run.handle.emit(SpanDraft {
        id: Uuid::now_v7(),
        node_id: run.node_id,
        agent_id: Some(caller.agent.id),
        principal: caller.agent.principal_id.clone(),
        parent_id: None,
        kind: SpanKind::Policy,
        name: "policy_mismatch".into(),
        started_at: now,
        ended_at: Some(now),
        status: SpanStatus::Error,
        attributes: json!({
            "decision": "mismatch",
            "rule": rule,
            "reason": detail,
            "action": "recheck",
            "span_id": span_id,
            "principal": caller.agent.principal_id,
            "policy_version": caller.agent.policy_version,
            "policy_versions": caller.versions,
            "firewall": true,
        }),
    });
    state.metrics.policy_mismatches.add(1);
    if let Err(err) = repo::runs::add_policy_mismatch(&state.db, run.run_id).await {
        tracing::error!(run_id = %run.run_id, error = %err, "policy mismatch not counted on the run");
    }
    let actor = caller.agent.owner_user_id.unwrap_or(Uuid::nil());
    if let Err(err) = repo::audit::record(
        &state.db,
        caller.workspace_id,
        actor,
        AuditAction::PolicyMismatch,
        Subject::Text(&caller.agent.principal_id),
        &format!("run {} span {span_id}: {detail} ({rule})", run.run_id),
    )
    .await
    {
        tracing::error!(run_id = %run.run_id, error = %err, "policy mismatch not audited");
    }
}

/// A running firewall server.
pub struct Running {
    pub addr: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl Running {
    /// `http://127.0.0.1:{port}`, for a client.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Stops the server.
    pub async fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let _ = (&mut self.task).await;
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

/// Serves the firewall, reflection and `grpc.health.v1` on `addr` (port 0
/// for a free one) until stopped, with HTTP/2 keepalive as `NEXC_GRPC_*`
/// say, so an agent platform's idle connection stays open and a dead one
/// is noticed.
pub async fn serve(state: AppState, addr: SocketAddr) -> std::io::Result<Running> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let addr = listener.local_addr()?;
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
    let (stop_tx, stop_rx) = oneshot::channel();
    let reflection = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(crate::rpc::FILE_DESCRIPTOR_SET)
        .build_v1()
        .map_err(std::io::Error::other)?;
    let (health_reporter, health) = tonic_health::server::health_reporter();
    health_reporter
        .set_serving::<AgentFirewallServer<FirewallService>>()
        .await;
    let keepalive = state.settings.grpc_channel();
    let service = FirewallService::new(state);
    let task = tokio::spawn(async move {
        let result = tonic::transport::Server::builder()
            .tcp_nodelay(true)
            .http2_keepalive_interval(Some(keepalive.keepalive))
            .http2_keepalive_timeout(Some(keepalive.keepalive_timeout))
            .add_service(health)
            .add_service(reflection)
            .add_service(AgentFirewallServer::new(service))
            .serve_with_incoming_shutdown(incoming, async {
                let _ = stop_rx.await;
            })
            .await;
        if let Err(err) = result {
            tracing::error!(error = %err, "the Agent Firewall API stopped");
        }
    });
    Ok(Running {
        addr,
        stop: Some(stop_tx),
        task,
    })
}

/// Serves on `NEXC_GRPC_ADDR` until `shutdown`, then seals open sessions.
pub async fn run(state: AppState, shutdown: CancellationToken) {
    let Some(addr) = state.settings.grpc_addr else {
        return;
    };
    let running = match serve(state.clone(), addr).await {
        Ok(running) => running,
        Err(err) => {
            tracing::error!(%addr, error = %err, "the Agent Firewall API could not listen");
            return;
        }
    };
    tracing::info!(addr = %running.addr, "the Agent Firewall API is listening (gRPC, reflection and health on)");
    shutdown.cancelled().await;
    state.firewall_sessions.seal_all(&state).await;
    running.stop().await;
}

/// The status of the firewall, for the operator.
#[derive(Debug, Clone, Default, serde::Serialize, utoipa::ToSchema)]
pub struct FirewallStatus {
    pub enabled: bool,
    #[schema(required = true)]
    pub addr: Option<String>,
    /// Hex of the Ed25519 public key decisions are signed with.
    pub public_key: String,
    /// Decisions sessions open on this instance.
    pub open_sessions: usize,
}

pub fn status(state: &AppState) -> FirewallStatus {
    FirewallStatus {
        enabled: state.settings.grpc_addr.is_some(),
        addr: state.settings.grpc_addr.map(|a| a.to_string()),
        public_key: hex::encode(state.firewall_signer.public_key()),
        open_sessions: state.firewall_sessions.open.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_bytes_are_canonical_and_cover_the_decision() {
        let decision = Decision::deny("egress.deny", "host evil.example is denied by policy");
        let versions = PolicyVersions {
            workspace: Some(2),
            team: None,
            agent: Some(1),
        };
        let (span, run) = (Uuid::now_v7(), Uuid::now_v7());
        let bytes = Signed {
            principal: "agent://w/a",
            action: "egress",
            subject: "evil.example",
            labels: &["internal".into()],
            decision: &decision,
            versions: &versions,
            span_id: span,
            run_id: run,
            decided_at: "2026-10-18T12:00:00.000000Z",
        }
        .bytes();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("{\"action\":\"egress\""));
        assert!(text.contains("\"decision\":\"deny\""));
        assert!(text.contains(&span.to_string()) && text.contains(&run.to_string()));
        assert!(text.contains("\"workspace\":2"));
    }
}
