//! The Agent Firewall API (AgentSecOps phase 4, roadmap §6): a gRPC service
//! on `NEXC_GRPC_ADDR` through which an agent that runs somewhere else uses
//! nexc as its policy and trace service. The agent is a workspace agent of
//! runtime `external` holding an API token; its decisions are taken in
//! process from its resolved policy, signed with the backend's Ed25519 key
//! and recorded as `policy` spans; the runs it reports are chained and
//! sealed like native ones; its card is the registry's. Reflection and
//! `grpc.health.v1` are served beside it.
//!
//! Every run of an external agent (a reported run, or the session its
//! decisions go to) is scored like a native one when it closes, against the
//! same kind of baseline, with the same alerts and quarantine; while it is
//! open it is scored live, span by span ([`live`]), and stopped mid-run
//! when it crosses the quarantine threshold. A `require_approval` decision
//! is an approval of the approvals engine the agent awaits ([`approvals`]).
#![forbid(unsafe_code)]

mod approvals;
pub mod live;

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant};

use chrono::{SecondsFormat, Utc};
use dashmap::DashMap;
use futures::StreamExt;
use serde_json::{Value, json};
use tokio::sync::{Mutex, RwLock, Semaphore, oneshot};
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
use crate::domain::policy::{self, Action, Decision, Effect, PolicyVersions};
use crate::domain::risk::Scope;
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
/// A decisions session is sealed after this long even when busy: each is a
/// window of about the same length, which its agent's session baseline
/// compares with the earlier ones.
pub const SESSION_WINDOW: Duration = Duration::from_secs(15 * 60);
/// How often a reported run looks whether its agent was quarantined on
/// another instance (here, the quarantine stops it at once), and takes up
/// the policy in force.
const STATUS_EVERY: Duration = Duration::from_secs(2);
/// Spans accepted on one reported run.
const SPANS_PER_RUN_MAX: usize = 20_000;
/// Reported runs one agent may have open on one instance.
const REPORTED_RUNS_MAX: usize = 16;
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
/// decisions go to (scope `sessions`), or a run being reported (`runs`).
pub struct OpenRun {
    pub run_id: Uuid,
    pub node_id: Uuid,
    pub graph_id: Uuid,
    /// Who answers for the run: the owner of the agent's graph.
    pub owner: Uuid,
    agent_id: Uuid,
    scope: Scope,
    handle: TraceHandle,
    sink: Mutex<Option<TraceSink>>,
    last_used: Mutex<Instant>,
    opened: Instant,
    /// Cancelled when the agent is quarantined (here at once, through the
    /// engine; elsewhere at a reported run's next look) or the run's live
    /// score crossed the threshold: nothing more is allowed on the run.
    quarantine: CancellationToken,
    /// Cancelled when the run is closed (finished, sealed, shut down).
    closed: CancellationToken,
    /// Decisions hold it shared from the moment they pick the run until
    /// their span is emitted; closing takes it alone and marks it `true`, so
    /// no decision lands in a sealed run.
    gate: Arc<RwLock<bool>>,
    live: StdMutex<live::Live>,
    /// Held while the run is contained, so it is not sealed before the record is.
    containment: Arc<Mutex<Option<live::Contained>>>,
    /// Approvals whose decision the run's trace records.
    settled: StdMutex<HashSet<Uuid>>,
}

/// What folding a span into a run's live score says.
struct Observed {
    /// This span took the score over the quarantine threshold.
    crossing: Option<live::Crossing>,
    /// The score crossed it, now or before: nothing more is allowed.
    tripped: bool,
}

impl OpenRun {
    async fn touch(&self) {
        *self.last_used.lock().await = Instant::now();
    }

    /// Folds a span into the run's live score.
    fn observe(&self, id: Uuid, kind: SpanKind, name: &str, attrs: &Value) -> Observed {
        let mut live = self.live.lock().expect("live risk");
        let crossing = live.observe(id, kind, name, attrs);
        Observed {
            crossing,
            tripped: live.tripped(),
        }
    }

    /// A session that has run its window and may be sealed for a new one.
    fn window_over(&self) -> bool {
        self.scope == Scope::Sessions && self.opened.elapsed() >= SESSION_WINDOW
    }

    /// The agent may no longer act on this run.
    fn stopped(&self) -> bool {
        self.quarantine.is_cancelled() || self.live.lock().expect("live risk").tripped()
    }
}

/// The runs of external agents open on this instance.
#[derive(Default)]
pub struct Sessions {
    /// Decisions sessions, by agent.
    open: DashMap<Uuid, Arc<OpenRun>>,
    /// Every open run (sessions and reported runs), by run.
    runs: DashMap<Uuid, Arc<OpenRun>>,
    /// `AwaitApproval` calls under way, by agent.
    waits: DashMap<Uuid, Arc<Semaphore>>,
}

impl Sessions {
    /// Seals the decisions sessions idle for `idle` or more; with `idle`
    /// zero, every open run, unscored (shutdown: a run cut short is not what
    /// its agent usually does, and its stream is told to try again).
    /// Returns how many.
    pub async fn seal_idle(&self, state: &AppState, idle: Duration) -> usize {
        let all = idle.is_zero();
        let stale: Vec<Arc<OpenRun>> = self
            .runs
            .iter()
            .filter(|e| {
                let run = e.value();
                all || (run.scope == Scope::Sessions
                    && run
                        .last_used
                        .try_lock()
                        .is_ok_and(|at| at.elapsed() >= idle))
            })
            .map(|e| e.value().clone())
            .collect();
        let mut sealed = 0;
        for run in stale {
            self.forget(&run);
            close_run(state, &run, RunStatus::Succeeded, None, !all).await;
            sealed += 1;
        }
        sealed
    }

    /// Seals every open run (shutdown).
    pub async fn seal_all(&self, state: &AppState) {
        self.seal_idle(state, Duration::ZERO).await;
    }

    /// The open run `run_id`, if it is open here.
    fn by_run(&self, run_id: Uuid) -> Option<Arc<OpenRun>> {
        self.runs.get(&run_id).map(|e| e.value().clone())
    }

    /// The decisions session of `agent` open here.
    fn session_of(&self, agent: Uuid) -> Option<Arc<OpenRun>> {
        self.open.get(&agent).map(|e| e.value().clone())
    }

    /// Forgets `run` (closed).
    fn forget(&self, run: &Arc<OpenRun>) {
        self.runs.remove(&run.run_id);
        self.open
            .remove_if(&run.agent_id, |_, open| Arc::ptr_eq(open, run));
    }

    /// How many runs `agent` is reporting here.
    fn reported_by(&self, agent: Uuid) -> usize {
        self.runs
            .iter()
            .filter(|e| e.value().agent_id == agent && e.value().scope == Scope::Runs)
            .count()
    }
}

/// Opens a run of an external agent in its managed graph, its live score
/// against the agent's baseline of `scope`. Until it closes, a quarantine
/// of the agent here stops it.
async fn open_run(
    state: &AppState,
    caller: &Caller,
    title: &str,
    external_run_id: &str,
    scope: Scope,
) -> Result<Arc<OpenRun>, Status> {
    // Read before the run exists: a run is never left open without its trace.
    let baseline = repo::baselines::find(&state.db, caller.agent.id, scope)
        .await
        .map_err(internal)?
        .unwrap_or_default();
    let opened = async {
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
        Ok::<_, sqlx::Error>((graph_id, owner, run_id, node_id))
    }
    .await;
    let (graph_id, owner, run_id, node_id) = opened.map_err(internal)?;
    // An external agent's session is no claimed run: nothing takes it over, nothing is fenced.
    let sink = trace::start(
        state.clone(),
        run_id,
        graph_id,
        crate::engine::lease::Lease::unfenced(run_id),
        trace::Tail::GENESIS,
    );
    let handle = sink.handle();
    let quarantine = CancellationToken::new();
    state
        .engine
        .agent_started(caller.agent.id, node_id, quarantine.clone());
    let run = Arc::new(OpenRun {
        run_id,
        node_id,
        graph_id,
        owner,
        agent_id: caller.agent.id,
        scope,
        handle,
        sink: Mutex::new(Some(sink)),
        last_used: Mutex::new(Instant::now()),
        opened: Instant::now(),
        quarantine,
        closed: CancellationToken::new(),
        gate: Arc::new(RwLock::new(false)),
        live: StdMutex::new(live::Live::new(scope, baseline, caller.resolved.clone())),
        containment: Arc::new(Mutex::new(None)),
        settled: StdMutex::new(HashSet::new()),
    });
    // Quarantined between the caller's check and the registration above: the
    // quarantine's cancel did not reach this run, so it stops here.
    if quarantined(state, caller).await {
        close_run(
            state,
            &run,
            RunStatus::Cancelled,
            Some("the agent was quarantined"),
            false,
        )
        .await;
        return Err(Status::permission_denied("the agent is quarantined"));
    }
    if let Ok(loaded) = scheduler::load_run(state, run_id).await {
        state
            .hub
            .publish(graph_id, SseEvent::RunStarted { run: loaded });
    }
    Ok(run)
}

/// Whether the caller's agent is quarantined now (or gone).
async fn quarantined(state: &AppState, caller: &Caller) -> bool {
    match repo::agents::find(&state.db, caller.workspace_id, caller.agent.id).await {
        Ok(Some(agent)) => agent.status == AgentStatus::Quarantined,
        Ok(None) => true,
        Err(_) => false,
    }
}

/// Seals a run's trace and closes it; returns the seal, once. Decisions
/// under way finish first, and a containment under way is recorded first.
/// Then, when `scored`, the run is scored and learned into its agent's
/// baseline like a native run, unless its live score crossed the threshold
/// (scored live already, and a run that went rogue is not what the agent
/// usually does).
async fn close_run(
    state: &AppState,
    run: &OpenRun,
    status: RunStatus,
    output: Option<&str>,
    scored: bool,
) -> Option<trace::Sealed> {
    let mut gate = run.gate.write().await;
    if *gate {
        return None;
    }
    *gate = true;
    drop(gate);
    run.closed.cancel();
    let contained = run.containment.lock().await.clone();
    let tripped = run.live.lock().expect("live risk").tripped();
    let sink = run.sink.lock().await.take()?;
    state.engine.agent_finished(run.agent_id, run.node_id);
    let sealed = sink.seal().await;
    let hash = sealed.hash;
    let seal = hash.map(|h| {
        crate::domain::seal::SignedSeal::sign(
            state.signing_keys.current(),
            run.run_id,
            sealed.spans,
            &h,
        )
    });
    // The run is closed and its seal on record in one transaction.
    let closed = async {
        let mut tx = state.db.begin().await?;
        repo::external_runs::close_run(
            &mut tx,
            run.run_id,
            run.node_id,
            status,
            output,
            hash.as_ref().map(|h| h.as_slice()),
            seal.as_ref(),
        )
        .await?;
        if let (Some(hash), Some(row)) = (hash, repo::runs::find_row(&mut *tx, run.run_id).await?) {
            scheduler::seal_audit(&mut tx, &row, hash, sealed.spans).await?;
        }
        tx.commit().await
    }
    .await;
    if let Err(err) = closed {
        tracing::error!(run_id = %run.run_id, error = %err, "external run not closed");
    }
    if scored
        && contained.is_none()
        && !tripped
        && let Ok(Some(row)) = repo::runs::find_row(&state.db, run.run_id).await
    {
        let assessed = crate::engine::risk::assess_in(state, &row, run.scope).await;
        if assessed.alerts > 0 || assessed.quarantined > 0 {
            tracing::info!(run_id = %run.run_id, alerts = assessed.alerts,
                quarantined = assessed.quarantined, "external run assessed");
        }
    }
    if let Ok(loaded) = scheduler::load_run(state, run.run_id).await {
        state
            .hub
            .publish(run.graph_id, SseEvent::RunFinished { run: loaded });
    }
    Some(sealed)
}

/// Expires the approvals external agents asked for that are past their time
/// (their watching instance may be gone), and tells the destinations. Run
/// by the maintenance worker.
pub async fn sweep(state: &AppState) {
    match repo::approvals::expire_overdue_external(&state.db).await {
        Ok(expired) => {
            for approval in &expired {
                crate::alerting::enqueue_approval(
                    state,
                    approval,
                    crate::domain::alert_routing::DeliveryEvent::ApprovalDecided,
                )
                .await;
            }
            if !expired.is_empty() {
                tracing::info!(
                    expired = expired.len(),
                    "overdue firewall approvals expired"
                );
            }
        }
        Err(err) => tracing::warn!(error = %err, "overdue firewall approvals not swept"),
    }
}

/// A reported run whose stream went away without a word (the client hung
/// up, its deadline passed): closed all the same, unscored.
struct Abandoned {
    state: AppState,
    run: Option<Arc<OpenRun>>,
}

impl Abandoned {
    fn disarm(&mut self) {
        self.run = None;
    }
}

impl Drop for Abandoned {
    fn drop(&mut self) {
        if let Some(run) = self.run.take() {
            let state = self.state.clone();
            tokio::spawn(async move {
                state.firewall_sessions.forget(&run);
                close_run(
                    &state,
                    &run,
                    RunStatus::Failed,
                    Some("the stream went away before the run finished"),
                    false,
                )
                .await;
            });
        }
    }
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

    /// The agent's decisions session on this instance, opened on demand;
    /// one that ran its window is sealed (and scored) and a new one opened.
    async fn session(&self, caller: &Caller) -> Result<Arc<OpenRun>, Status> {
        if let Some(open) = self.sessions.session_of(caller.agent.id) {
            if open.stopped() {
                // Stopped by a quarantine: still in force, or lifted since (the caller passed
                // the check), in which case the agent starts a new window.
                if quarantined(&self.state, caller).await {
                    return Err(Status::permission_denied("the agent is quarantined"));
                }
            } else if !open.window_over() && !open.closed.is_cancelled() {
                open.touch().await;
                return Ok(open);
            }
            self.sessions.forget(&open);
            let state = self.state.clone();
            tokio::spawn(async move {
                let scored = !open.stopped();
                close_run(&state, &open, RunStatus::Succeeded, None, scored).await;
            });
        }
        let run = open_run(
            &self.state,
            caller,
            "Firewall decisions",
            &format!("session-{}", Uuid::now_v7()),
            Scope::Sessions,
        )
        .await?;
        // Two first calls at once: one session stays, the other is sealed empty.
        let kept = self
            .sessions
            .open
            .entry(caller.agent.id)
            .or_insert_with(|| run.clone())
            .clone();
        if !Arc::ptr_eq(&kept, &run) {
            close_run(&self.state, &run, RunStatus::Succeeded, None, false).await;
            kept.touch().await;
            return Ok(kept);
        }
        self.sessions.runs.insert(run.run_id, run.clone());
        // A quarantine of the agent here stops the session at once.
        let (state, watched) = (self.state.clone(), run.clone());
        tokio::spawn(async move {
            tokio::select! {
                () = watched.quarantine.cancelled() => {}
                () = watched.closed.cancelled() => return,
            }
            // A containment closes the session itself, with its reason.
            if !watched.live.lock().expect("live risk").tripped() {
                state.firewall_sessions.forget(&watched);
                close_run(
                    &state,
                    &watched,
                    RunStatus::Cancelled,
                    Some("the agent was quarantined"),
                    false,
                )
                .await;
            }
        });
        Ok(run)
    }

    /// Contains `run` (its live score crossed the threshold): the agent is
    /// quarantined and the run recorded; a session is closed as cancelled
    /// (a reported run is closed by its stream). The work runs on a task of
    /// its own, so a caller that goes away does not stop it half way.
    async fn contain(
        &self,
        caller: &Caller,
        run: &Arc<OpenRun>,
        crossing: live::Crossing,
    ) -> live::Contained {
        // The containment is held before anything can wake a close of the run.
        let slot = match run.containment.clone().try_lock_owned() {
            Ok(slot) => slot,
            Err(_) => run.containment.clone().lock_owned().await,
        };
        run.quarantine.cancel();
        let (state, caller, run) = (self.state.clone(), caller.clone(), run.clone());
        let task = tokio::spawn(async move {
            let contained = live::contain(&state, &caller, &run, crossing, slot).await;
            if run.scope == Scope::Sessions {
                state.firewall_sessions.forget(&run);
                close_run(
                    &state,
                    &run,
                    RunStatus::Cancelled,
                    Some(&format!("quarantined mid-run: {}", contained.reason)),
                    false,
                )
                .await;
            }
            contained
        });
        task.await.expect("the containment task does not panic")
    }
}

impl FirewallService {
    /// A reported run looks again at its agent: quarantined elsewhere, the
    /// run stops; else it takes up the policy in force.
    async fn look_again(&self, caller: &Caller, run: &OpenRun) {
        match repo::agents::find(&self.state.db, caller.workspace_id, caller.agent.id).await {
            Ok(Some(agent)) if agent.status == AgentStatus::Quarantined => run.quarantine.cancel(),
            Ok(Some(agent)) => {
                if let Ok(effective) = self
                    .state
                    .caches
                    .resolved(&self.state.db, caller.workspace_id, &agent)
                    .await
                {
                    run.live
                        .lock()
                        .expect("live risk")
                        .refresh(effective.resolved.clone());
                }
            }
            Ok(None) => run.quarantine.cancel(),
            Err(err) => tracing::debug!(run_id = %run.run_id, error = %err, "agent not looked at"),
        }
    }

    /// A reported call of a held tool that names the approval it ran under
    /// uses it (once, by this agent, for this tool and these arguments): the
    /// re-check then takes it as granted. Named again, it grants nothing.
    async fn grant_reported(&self, caller: &Caller, progress: &Progress, draft: &SpanDraft) {
        if draft.kind != SpanKind::ToolCall {
            return;
        }
        let attrs = &draft.attributes;
        let Some(id) = attrs
            .get("approval_id")
            .and_then(Value::as_str)
            .and_then(|id| id.parse::<Uuid>().ok())
        else {
            return;
        };
        let tool = attrs
            .get("tool")
            .and_then(Value::as_str)
            .unwrap_or(&draft.name);
        if !matches!(
            caller.decide(&[], &Action::Tool(tool)).effect,
            Effect::RequireApproval { .. }
        ) {
            return;
        }
        let digest = attrs.get("args_sha256").and_then(Value::as_str);
        match repo::approvals::consume(&self.state.db, id, caller.agent.id, tool, digest, draft.id)
            .await
        {
            Ok(true) => progress.grant(id),
            Ok(false) => progress.revoke(id),
            Err(err) => {
                progress.revoke(id);
                tracing::warn!(approval = %id, error = %err, "approval not used");
            }
        }
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
    /// The approval the decision asks for (`require_approval`) or settles
    /// (the final decision `AwaitApproval` hands out).
    pub approval_id: Option<Uuid>,
    /// The digest of the tool call's arguments the decision is about, when named.
    pub args_sha256: Option<&'a str>,
    /// The id of the key that signs (`/.well-known/jwks.json`).
    pub kid: &'a str,
}

impl Signed<'_> {
    /// The bytes the signature covers: canonical JSON (keys sorted) of what
    /// was asked, what was decided, under which policy versions, where it is
    /// recorded, when, and by which key, and the approval and arguments it is
    /// about, if any.
    pub fn bytes(&self) -> Vec<u8> {
        let mut map = serde_json::Map::new();
        map.insert("action".into(), json!(self.action));
        if let Some(approval) = self.approval_id {
            map.insert("approval_id".into(), json!(approval));
        }
        if let Some(digest) = self.args_sha256 {
            map.insert("args_sha256".into(), json!(digest));
        }
        for (key, value) in [
            ("decided_at", json!(self.decided_at)),
            ("decision", json!(self.decision.verdict())),
            ("kid", json!(self.kid)),
            ("labels", json!(self.labels)),
            ("policy_versions", json!(self.versions)),
            ("principal", json!(self.principal)),
            ("reason", json!(self.decision.reason)),
            ("rule", json!(self.decision.rule)),
            ("run_id", json!(self.run_id)),
            ("span_id", json!(self.span_id)),
            ("subject", json!(self.subject)),
        ] {
            map.insert(key.into(), value);
        }
        serde_json::to_vec(&Value::Object(map)).expect("a decision serialises")
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
        let args_sha256 = match context.get("args_sha256").and_then(Value::as_str) {
            Some(digest)
                if digest.len() == 64
                    && digest
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) =>
            {
                Some(digest.to_owned())
            }
            Some(_) => {
                return Err(Status::invalid_argument(
                    "args_sha256 is the lower-case hex SHA-256 of the arguments",
                ));
            }
            None => None,
        };
        let mut decision = caller.decide(&labels, &action);
        // The run the decision is recorded in: one the agent reported, else its session. It
        // stays open until the decision's span is emitted.
        let mut reported = context
            .get("run_id")
            .and_then(Value::as_str)
            .and_then(|r| r.parse::<Uuid>().ok())
            .and_then(|run_id| self.sessions.by_run(run_id))
            .filter(|run| run.agent_id == caller.agent.id);
        let mut tries = 0;
        let (run, gate) = loop {
            let run = match reported.take() {
                Some(run) => run,
                None => self.session(&caller).await?,
            };
            let gate = run.gate.clone().read_owned().await;
            if !*gate {
                break (run, gate);
            }
            // Closed meanwhile (the session ran its window): the next one takes it.
            tries += 1;
            if tries == 3 {
                return Err(Status::unavailable(
                    "no run to record the decision in; try again",
                ));
            }
        };
        let span_id = Uuid::now_v7();
        // The live score takes the decision in before it is handed out, under the policy in
        // force now: the one that takes the agent over the quarantine threshold is refused,
        // and so is every one after it.
        run.live
            .lock()
            .expect("live risk")
            .refresh(caller.resolved.clone());
        let observed = run.observe(
            span_id,
            SpanKind::Policy,
            &subject,
            &json!({"decision": decision.verdict(), "action": req.action,
                "subject": subject, "firewall": true}),
        );
        if let Some(crossing) = &observed.crossing {
            decision = Decision::deny(
                live::RULE,
                format!(
                    "live risk score {:.1} over {:.1} ({}); the agent is quarantined",
                    crossing.risk.score,
                    crossing.threshold,
                    crossing.reasons()
                ),
            );
        } else if observed.tripped || run.quarantine.is_cancelled() {
            // Quarantined while this call was under way.
            decision = Decision::deny("quarantine", "the agent is quarantined");
        }
        // Held for a human: an approval of the approvals engine, to await.
        let approval = match (&decision.effect, &action) {
            (Effect::RequireApproval { .. }, Action::Tool(tool)) => {
                match approvals::open(
                    &self.state,
                    approvals::Asked {
                        caller: &caller,
                        run: &run,
                        principal: &principal,
                        span_id,
                        tool,
                        args_sha256: args_sha256.as_deref(),
                        decision: &decision,
                    },
                )
                .await
                {
                    approvals::Opened::Asked(approval) => Some(*approval),
                    approvals::Opened::Refused(refusal) => {
                        decision = refusal;
                        None
                    }
                    approvals::Opened::Failed => None,
                }
            }
            _ => None,
        };
        let now = Utc::now();
        let decided_at = now.to_rfc3339_opts(SecondsFormat::Micros, true);
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
            approval_id: approval.as_ref().map(|a| a.id),
            args_sha256: args_sha256.as_deref(),
            kid: self.state.firewall_signer.kid(),
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
                "approval_id": approval.as_ref().map(|a| a.id),
                "args_sha256": args_sha256,
                "kid": self.state.firewall_signer.kid(),
            }),
        });
        drop(gate);
        self.state
            .metrics
            .firewall_decisions
            .add(&[("decision", decision.verdict())], 1);
        if let Some(crossing) = observed.crossing {
            self.contain(&caller, &run, crossing).await;
        }
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
            approval_id: approval
                .as_ref()
                .map(|a| a.id.to_string())
                .unwrap_or_default(),
            approval_expires_at: approval
                .as_ref()
                .map(|a| a.expires_at.to_rfc3339_opts(SecondsFormat::Micros, true))
                .unwrap_or_default(),
            kid: self.state.firewall_signer.kid().to_owned(),
        }))
    }

    async fn await_approval(
        &self,
        request: Request<pb::AwaitApprovalRequest>,
    ) -> Result<Response<pb::ApprovalReply>, Status> {
        let caller = self.authenticate(request.metadata()).await?;
        let req = request.into_inner();
        let id: Uuid = req
            .approval_id
            .parse()
            .map_err(|_| Status::invalid_argument("approval_id is not a UUID"))?;
        let wait = Duration::from_millis(u64::from(req.wait_ms));
        let reply = approvals::await_reply(&self.state, &self.sessions, &caller, id, wait).await?;
        Ok(Response::new(reply))
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
        if self.sessions.reported_by(caller.agent.id) >= REPORTED_RUNS_MAX {
            return Err(Status::resource_exhausted(format!(
                "the agent already reports {REPORTED_RUNS_MAX} runs here"
            )));
        }
        let run = open_run(
            &self.state,
            &caller,
            &title,
            &bounded(&start.external_run_id),
            Scope::Runs,
        )
        .await?;
        self.sessions.runs.insert(run.run_id, run.clone());
        // Closed even if this handler is dropped before the run ends.
        let mut abandoned = Abandoned {
            state: self.state.clone(),
            run: Some(run.clone()),
        };
        let mut progress = Progress::new(Delivery::Http);
        let mut spans = 0usize;
        let mut mismatches = 0u32;
        let mut status = RunStatus::Succeeded;
        let mut output: Option<String> = None;
        let own = caller.agent.principal_id.clone();
        let decide = |action: &Action<'_>| caller.decide(&[], action);
        let mut look =
            tokio::time::interval_at(tokio::time::Instant::now() + STATUS_EVERY, STATUS_EVERY);
        look.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let next = tokio::select! {
                biased;
                () = run.quarantine.cancelled() => break,
                () = run.closed.cancelled() => break,
                _ = look.tick() => {
                    self.look_again(&caller, &run).await;
                    continue;
                }
                next = stream.next() => next,
            };
            let Some(next) = next else { break };
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
                            self.grant_reported(&caller, &progress, &draft).await;
                            let mismatch = recheck_with(&decide, &own, &mut progress, &draft)
                                .or_else(|| recheck_egress(&caller, &draft));
                            let span_id = draft.id;
                            let mut crossing = run
                                .observe(span_id, draft.kind, &draft.name, &draft.attributes)
                                .crossing;
                            run.handle.emit(draft);
                            if let Some((rule, detail)) = mismatch {
                                mismatches += 1;
                                let tripped = record_mismatch(
                                    &self.state,
                                    &caller,
                                    &run,
                                    span_id,
                                    rule,
                                    detail,
                                )
                                .await;
                                crossing = crossing.or(tripped);
                            }
                            if let Some(crossing) = crossing {
                                self.contain(&caller, &run, crossing).await;
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
        abandoned.disarm();
        self.sessions.forget(&run);
        // Closed under the stream (the server is shutting down): the agent tries again.
        if run.closed.is_cancelled() && !run.quarantine.is_cancelled() {
            return Err(Status::unavailable(
                "the server is shutting down; the run was closed where it stood",
            ));
        }
        // Stopped before its end: its agent was quarantined (by this run's live score, by a
        // decision recorded in it, or anywhere else) while it was open.
        let quarantined = run.quarantine.is_cancelled();
        let contained = run.containment.lock().await.clone();
        if quarantined {
            status = RunStatus::Cancelled;
            output = Some(match &contained {
                Some(contained) => format!("quarantined mid-run: {}", contained.reason),
                None => "the agent was quarantined while the run was open".to_owned(),
            });
        }
        let risk_score = run.live.lock().expect("live risk").score();
        let sealed = close_run(&self.state, &run, status, output.as_deref(), !quarantined)
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
            status: status.as_str().into(),
            quarantined,
            risk_score: contained.as_ref().map_or(risk_score, |c| c.score),
            time_to_contain_ms: contained.as_ref().map_or(0, |c| c.time_to_contain_ms),
            reason: contained.map(|c| c.reason).unwrap_or_default(),
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
        // The signed seal is checked as GET /runs/{rid}/trace/verify checks it.
        let seal = row.seal.as_ref().map(|seal| {
            let key = self.state.signing_keys.public_key(&seal.kid);
            seal.check(row.id, verified.chain(), key.as_ref())
        });
        let verified = verified.with_seal(seal);
        let seal_reason = verified.seal.as_ref().and_then(|s| s.reason.clone());
        let broken = verified.broken.as_ref();
        Ok(Response::new(pb::VerifyReply {
            ok: verified.ok,
            spans: verified.spans,
            hash: verified.hash.unwrap_or_default(),
            sealed_hash: verified.sealed_hash.unwrap_or_default(),
            broken_span_id: broken.map(|b| b.span_id.to_string()).unwrap_or_default(),
            broken_seq: broken.map_or(0, |b| b.seq),
            broken_reason: broken
                .map(|b| b.reason.clone())
                .or(seal_reason)
                .unwrap_or_default(),
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

/// A reported call out (a `tool_call` naming its `egress_host`) to a host the
/// policy does not allow: the rule and the detail, as a re-check gives them.
fn recheck_egress(caller: &Caller, draft: &SpanDraft) -> Option<(&'static str, String)> {
    if draft.kind != SpanKind::ToolCall {
        return None;
    }
    let host = draft
        .attributes
        .get("egress_host")
        .and_then(Value::as_str)?;
    let decision = caller.decide(&[], &Action::Egress(host));
    (!decision.allowed).then(|| {
        (
            decision.rule,
            format!("the agent called host {host}; {}", decision.reason),
        )
    })
}

/// An external agent reported what its policy does not allow: a `policy`
/// span of the run, a count on it, an audit entry. The live score counts it
/// as a denial: the crossing, if that took it over the threshold.
async fn record_mismatch(
    state: &AppState,
    caller: &Caller,
    run: &OpenRun,
    span_id: Uuid,
    rule: &'static str,
    detail: String,
) -> Option<live::Crossing> {
    let now = Utc::now();
    let id = Uuid::now_v7();
    let attributes = json!({
        "decision": "mismatch",
        "rule": rule,
        "reason": detail,
        "action": "recheck",
        "span_id": span_id,
        "principal": caller.agent.principal_id,
        "policy_version": caller.agent.policy_version,
        "policy_versions": caller.versions,
        "firewall": true,
    });
    let crossing = run
        .observe(id, SpanKind::Policy, "policy_mismatch", &attributes)
        .crossing;
    run.handle.emit(SpanDraft {
        id,
        node_id: run.node_id,
        agent_id: Some(caller.agent.id),
        principal: caller.agent.principal_id.clone(),
        parent_id: None,
        kind: SpanKind::Policy,
        name: "policy_mismatch".into(),
        started_at: now,
        ended_at: Some(now),
        status: SpanStatus::Error,
        attributes,
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
    crossing
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
/// is noticed; over TLS when `NEXC_GRPC_TLS_*` name a certificate (and, with
/// a client CA, only for callers with a certificate of it).
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
    let mut builder = tonic::transport::Server::builder();
    if let Some(tls) = &state.settings.grpc_server_tls {
        builder = builder
            .tls_config(tls.config())
            .map_err(std::io::Error::other)?;
    }
    let service = FirewallService::new(state);
    let task = tokio::spawn(async move {
        let result = builder
            .tcp_nodelay(true)
            .http2_keepalive_interval(Some(keepalive.keepalive))
            .http2_keepalive_timeout(Some(keepalive.keepalive_timeout))
            .http2_adaptive_window(Some(true))
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
    /// Its id, as published at `/.well-known/jwks.json`.
    pub kid: String,
    /// Decisions sessions open on this instance.
    pub open_sessions: usize,
}

pub fn status(state: &AppState) -> FirewallStatus {
    FirewallStatus {
        enabled: state.settings.grpc_addr.is_some(),
        addr: state.settings.grpc_addr.map(|a| a.to_string()),
        public_key: hex::encode(state.firewall_signer.public_key()),
        kid: state.firewall_signer.kid().to_owned(),
        open_sessions: state.firewall_sessions.runs.len(),
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
            approval_id: None,
            args_sha256: None,
            kid: "kid-1",
        }
        .bytes();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("{\"action\":\"egress\""));
        assert!(
            text.contains("\"decision\":\"deny\",\"kid\":\"kid-1\""),
            "{text}"
        );
        assert!(text.contains("\"decision\":\"deny\""));
        assert!(text.contains(&span.to_string()) && text.contains(&run.to_string()));
        assert!(text.contains("\"workspace\":2"));
    }
}
