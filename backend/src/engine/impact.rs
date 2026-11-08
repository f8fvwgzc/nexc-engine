//! Assesses every change to an issue or a project with the change-impact
//! engine and versions it.
//!
//! The engine is a sibling service (`assessment-engine`,
//! <https://github.com/f8fvwgzc/assessment-engine>) reached over gRPC with its
//! typed client ([`assessment_grpc::client`]), through one lazily connected
//! channel the whole process shares ([`Assessor`]). It is called statelessly
//! (`Assess`): PostgreSQL holds the versions and the pins, and each assessment
//! is given the changed item with its neighbourhood read from there — its
//! parent chain, sub-issues, siblings and projects — and its history, in the
//! transaction that writes the change. So the version moves with the change or
//! not at all, and two edits of one item take turns (its row is locked from the
//! read of its "before" to the commit). The engine's own registry
//! (`PutEntity`, …) is never used: there is no second store to keep in step.
//!
//! For each change it writes the item's next version (MAJOR, MINOR or PATCH),
//! keeps the assessment when it moved a version or asks anything of anyone,
//! and, when the engine recommends a review or a re-plan, puts an `impact`
//! entry in the inbox of the owners of every other affected item (and of the
//! item to re-plan). A recommended re-plan is taken up as a proposal
//! (`engine::replan`), drafted by the planner once the change commits.
//!
//! An engine that is down, refuses the call or misses the deadline
//! (`NEXC_ASSESSMENT_TIMEOUT_MS`, short: rows are locked meanwhile) never
//! fails the edit: it is logged, the change saves unassessed and its version
//! stays. After the engine was unreachable, calls rest for a few seconds, so
//! that a burst of edits does not each wait out the deadline.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context as _;
use assessment_core as ac;
use assessment_core::model::{
    ActivityEvent, ActivityKind, AssessOptions, AssessmentRequest, ChangeSet, Entity, HistoryPoint,
};
use assessment_grpc::client::{AssessmentClient, ClientErrorKind};
use chrono::Utc;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::config::Settings;
use crate::domain::error_chain;
use crate::domain::impact::{
    AssessmentCause, HISTORY_MAX, IMPACTS_MAX, ImpactAction, ImpactEntityKind, IssueNode,
    NewAssessment, ProjectNode, VersionBump,
};
use crate::observability::metrics::Metrics;
use crate::repo;
use crate::repo::replan::{NewProposal, Proposed};

/// The service and method the gRPC latency metric names.
const SERVICE: &str = "assessment.v1.AssessmentEngine";
/// How long calls rest after the engine was unreachable (refused, reset or silent).
const REST: Duration = Duration::from_secs(5);

/// The change-impact engine as this backend reaches it: one channel shared by
/// every request (cheap to clone), the bearer token it requires, the call
/// deadline, and how re-plans are proposed. Disabled, nothing is assessed.
#[derive(Clone)]
pub struct Assessor {
    inner: Option<Arc<Remote>>,
    metrics: Arc<Metrics>,
    /// Whether a recommended re-plan is proposed (`NEXC_REPLAN_PROPOSALS`).
    replans: bool,
    /// How long a proposal waits for further changes (`NEXC_REPLAN_DEBOUNCE_SECS`).
    debounce: Duration,
}

struct Remote {
    client: AssessmentClient,
    url: String,
    timeout: Duration,
    /// Until when (Unix milliseconds) no call is made: the engine was unreachable.
    resting_until: AtomicU64,
}

impl std::fmt::Debug for Assessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Assessor")
            .field("url", &self.inner.as_ref().map(|r| r.url.as_str()))
            .field("timeout", &self.inner.as_ref().map(|r| r.timeout))
            .field("replans", &self.replans)
            .finish()
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// How a failed call is counted and whether it makes calls rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    /// The engine answered with an error (a request it refused, a token it does not know).
    Refused,
    /// The engine could not be reached.
    Unavailable,
    /// No answer within the deadline.
    Timeout,
}

impl Failure {
    fn as_str(self) -> &'static str {
        match self {
            Failure::Refused => "refused",
            Failure::Unavailable => "unavailable",
            Failure::Timeout => "timeout",
        }
    }
}

impl Assessor {
    /// The engine at `NEXC_ASSESSMENT_URL` (TLS for `https://`, as every
    /// gRPC channel: `NEXC_GRPC_CA_FILE`, …), connected on first use; none
    /// when `NEXC_ASSESSMENT_ENABLED=false`.
    pub fn new(settings: &Settings, metrics: Arc<Metrics>) -> anyhow::Result<Self> {
        let mut assessor = Assessor {
            inner: None,
            metrics,
            replans: settings.replan_proposals,
            debounce: settings.replan_debounce,
        };
        if !settings.assessment_enabled {
            return Ok(assessor);
        }
        // One connection: a unary call a few milliseconds long needs no pool, and a single
        // endpoint fails at once when the engine is down instead of waiting for a ready one.
        let channel = crate::rpc::ChannelSettings {
            pool: 1,
            ..settings
                .grpc_channel()
                .connect_timeout(settings.assessment_timeout)
        };
        let channel = crate::rpc::channel(&settings.assessment_url, &channel)
            .context("NEXC_ASSESSMENT_URL is not a usable gRPC endpoint")?;
        let mut client = AssessmentClient::from_channel(channel);
        if let Some(token) = &settings.assessment_token {
            client = client
                .with_bearer_token(token.expose())
                .context("NEXC_ASSESSMENT_TOKEN is not a valid header value")?;
        }
        assessor.inner = Some(Arc::new(Remote {
            client,
            url: settings.assessment_url.clone(),
            timeout: settings.assessment_timeout,
            resting_until: AtomicU64::new(0),
        }));
        Ok(assessor)
    }

    /// Whether changes are assessed at all.
    pub fn is_enabled(&self) -> bool {
        self.inner.is_some()
    }

    /// The engine's endpoint, when enabled.
    pub fn url(&self) -> Option<&str> {
        self.inner.as_ref().map(|r| r.url.as_str())
    }

    /// Whether recommended re-plans are proposed.
    pub fn proposes_replans(&self) -> bool {
        self.replans
    }

    /// How long a re-plan proposal waits for further changes to its root.
    pub fn debounce(&self) -> Duration {
        self.debounce
    }

    /// Assesses `request` within the deadline; `None`, logged, when the
    /// engine is off, resting, unreachable, refused it or did not answer.
    async fn run(&self, request: AssessmentRequest) -> Option<ac::Assessment> {
        let remote = self.inner.as_ref()?;
        if now_millis() < remote.resting_until.load(Ordering::Relaxed) {
            self.metrics.assessments.add(&[("outcome", "resting")], 1);
            tracing::debug!(url = %remote.url, "the change-impact engine was unreachable moments ago; the change is not assessed");
            return None;
        }
        let mut client = remote.client.clone();
        let started = Instant::now();
        let answer = tokio::time::timeout(remote.timeout, client.assess(request)).await;
        self.metrics
            .grpc_client(SERVICE, "Assess", started.elapsed());
        let failure = match answer {
            Ok(Ok(assessment)) => {
                self.metrics.assessments.add(&[("outcome", "assessed")], 1);
                return Some(assessment);
            }
            Ok(Err(err)) => {
                let failure = match err.kind() {
                    ClientErrorKind::Transport => Failure::Unavailable,
                    ClientErrorKind::Status(tonic::Code::Unavailable) => Failure::Unavailable,
                    ClientErrorKind::Status(tonic::Code::DeadlineExceeded) => Failure::Timeout,
                    _ => Failure::Refused,
                };
                tracing::warn!(url = %remote.url, error = %error_chain(&err),
                    outcome = failure.as_str(),
                    "the change-impact engine did not assess a change; it saves unassessed and its version stays");
                failure
            }
            Err(_) => {
                tracing::warn!(url = %remote.url, timeout_ms = remote.timeout.as_millis() as u64,
                    "the change-impact engine did not answer in time; the change saves unassessed and its version stays");
                Failure::Timeout
            }
        };
        self.metrics
            .assessments
            .add(&[("outcome", failure.as_str())], 1);
        if failure != Failure::Refused {
            let until = now_millis().saturating_add(REST.as_millis() as u64);
            remote.resting_until.store(until, Ordering::Relaxed);
        }
        None
    }
}

/// What was changed, for the assessment's row.
struct Subject {
    kind: ImpactEntityKind,
    id: Uuid,
    workspace_id: Uuid,
    team_id: Option<Uuid>,
    cause: AssessmentCause,
}

/// The issues around `node` and every project they or `node` are part of,
/// as the engine's entities, plus `node` itself when `with_self`.
async fn issue_context(
    db: &mut PgConnection,
    node: &IssueNode,
    also_issues: &[Uuid],
    also_projects: &[Uuid],
    with_self: bool,
) -> Result<Vec<Entity>, sqlx::Error> {
    let near = repo::impact::issue_neighbourhood(&mut *db, node.id, also_issues).await?;
    let project_ids: BTreeSet<Uuid> = near
        .iter()
        .chain(std::iter::once(node))
        .filter_map(|n| n.project_id)
        .chain(also_projects.iter().copied())
        .collect();
    let project_ids: Vec<Uuid> = project_ids.into_iter().collect();
    let projects = repo::impact::projects(&mut *db, &project_ids).await?;
    let mut context: Vec<Entity> = near.iter().map(IssueNode::entity).collect();
    context.extend(projects.iter().map(ProjectNode::entity));
    if with_self {
        context.push(node.entity());
    }
    Ok(context)
}

/// Assesses `change` of `subject` against `context`, then writes the next
/// version, keeps the assessment and tells the owners of the affected items
/// as the module says. `actor` is `None` for a change nobody made (a run
/// moving its issue to review).
async fn assess(
    engine: &Assessor,
    db: &mut PgConnection,
    actor: Option<Uuid>,
    subject: Subject,
    change: ChangeSet,
    context: Vec<Entity>,
) -> Result<Option<NewAssessment>, sqlx::Error> {
    let history = repo::impact::history(&mut *db, subject.id, HISTORY_MAX)
        .await?
        .into_iter()
        .map(|(at, magnitude)| HistoryPoint {
            entity_id: subject.id.to_string(),
            at,
            magnitude,
        })
        .collect();
    let request = AssessmentRequest {
        changes: vec![change],
        context,
        history,
        now: Some(Utc::now()),
        options: AssessOptions {
            top_k: IMPACTS_MAX,
            explain: false,
            max_contributions: 0,
        },
    };
    let Some(assessment) = engine.run(request).await else {
        return Ok(None);
    };
    let mut a = NewAssessment::from_engine(&assessment, subject.kind, subject.id, subject.cause);
    a.workspace_id = subject.workspace_id;
    a.team_id = subject.team_id;
    a.actor_id = actor;
    if matches!(
        a.bump,
        VersionBump::Patch | VersionBump::Minor | VersionBump::Major
    ) && let Some(next) = a.next_version
    {
        match subject.kind {
            ImpactEntityKind::Issue => {
                repo::impact::set_issue_version(&mut *db, subject.id, next).await?
            }
            ImpactEntityKind::Project => {
                repo::impact::set_project_version(&mut *db, subject.id, next).await?
            }
        }
    }
    if !a.worth_keeping() {
        return Ok(Some(a));
    }
    let id = repo::impact::insert_assessment(&mut *db, &a).await?;
    // The item to re-plan, unless it is the one being deleted.
    let root = a
        .replan_target()
        .filter(|(_, root)| !(subject.cause == AssessmentCause::Deleted && *root == subject.id));
    if let Some((_, root)) = root
        && engine.proposes_replans()
    {
        a.replan_id = propose(engine, db, &subject, id, actor, root).await?;
    }
    if a.asks_for_review() {
        // Only the owners of the other items: the changed item's own hear of it from its timeline
        // and the entries an edit already sends (assigned, state, comment), and a deleted one's
        // entries would go with it. The owners of the item to re-plan hear of it too: the entry
        // leads to the proposal.
        let acted_on = a
            .impacts
            .iter()
            .filter(|i| i.action != ImpactAction::None && !i.changed);
        let (issues, projects): (Vec<_>, Vec<_>) =
            acted_on.partition(|i| i.entity_kind == ImpactEntityKind::Issue);
        let mut issues: Vec<Uuid> = issues.iter().map(|i| i.entity_id).collect();
        if let Some((ImpactEntityKind::Issue, root)) = root
            && a.replan_id.is_some()
            && !issues.contains(&root)
        {
            issues.push(root);
        }
        let projects: Vec<Uuid> = projects.iter().map(|i| i.entity_id).collect();
        // A project's lead hears about the changed issue; a deleted one has nothing to point at.
        let about = (subject.kind == ImpactEntityKind::Issue
            && subject.cause != AssessmentCause::Deleted)
            .then_some(subject.id);
        repo::impact::notify_impacted(&mut *db, actor, id, &issues, &projects, about).await?;
    }
    Ok(Some(a))
}

/// Takes up the re-plan of `root` that assessment `assessment_id` asks for
/// (`repo::replan::propose`) and links the two; the planner starts once the
/// transaction commits and the debounce is over. `None` when nothing was taken
/// up: the root is gone, or a change nobody made found no proposal to extend.
async fn propose(
    engine: &Assessor,
    db: &mut PgConnection,
    subject: &Subject,
    assessment_id: Uuid,
    actor: Option<Uuid>,
    root: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    let Some((root_kind, team_id)) = repo::replan::root(&mut *db, root).await? else {
        return Ok(None);
    };
    let proposal = NewProposal {
        workspace_id: subject.workspace_id,
        root_kind,
        root_id: root,
        team_id,
        assessment_id,
        owner_id: actor,
    };
    let Some((id, proposed)) = repo::replan::propose(&mut *db, &proposal, engine.debounce).await?
    else {
        return Ok(None);
    };
    let replans = &engine.metrics.replan_proposals;
    match proposed {
        Proposed::Extended => replans.add(&[("event", "extended")], 1),
        Proposed::New { superseded } => {
            replans.add(&[("event", "proposed")], 1);
            if superseded > 0 {
                replans.add(&[("event", "superseded")], superseded);
            }
        }
    }
    repo::replan::link_assessment(&mut *db, assessment_id, id).await?;
    Ok(Some(id))
}

fn issue_subject(node: &IssueNode, cause: AssessmentCause) -> Subject {
    Subject {
        kind: ImpactEntityKind::Issue,
        id: node.id,
        workspace_id: node.workspace_id,
        team_id: Some(node.team_id),
        cause,
    }
}

fn project_subject(node: &ProjectNode, cause: AssessmentCause) -> Subject {
    Subject {
        kind: ImpactEntityKind::Project,
        id: node.id,
        workspace_id: node.workspace_id,
        team_id: node.team_id,
        cause,
    }
}

/// Assesses the edit that turned `before` (read with
/// [`repo::impact::issue_for_update`] before the write) into the issue as it
/// is now, in the same transaction, and versions it. An issue that moved to
/// another parent or project is pinned to that one's current version.
pub async fn issue_updated(
    engine: &Assessor,
    db: &mut PgConnection,
    actor: Uuid,
    before: &IssueNode,
) -> Result<Option<NewAssessment>, sqlx::Error> {
    issue_changed(engine, db, Some(actor), before).await
}

/// Assesses what a finished run did to the issues its graph executes (a
/// successful one moves them to review), given them as they were before
/// ([`repo::impact::issues_of_graph_for_update`]): a change nobody made.
pub async fn issues_reported_to(
    engine: &Assessor,
    db: &mut PgConnection,
    before: &[IssueNode],
) -> Result<(), sqlx::Error> {
    for node in before {
        issue_changed(engine, db, None, node).await?;
    }
    Ok(())
}

async fn issue_changed(
    engine: &Assessor,
    db: &mut PgConnection,
    actor: Option<Uuid>,
    before: &IssueNode,
) -> Result<Option<NewAssessment>, sqlx::Error> {
    let Some(after) = repo::impact::issue(&mut *db, before.id).await? else {
        return Ok(None);
    };
    if before.basis_target() != after.basis_target() {
        repo::impact::repin(&mut *db, after.id).await?;
    }
    let (was, now) = (before.entity(), after.entity());
    if was.fields == now.fields && was.parent_id == now.parent_id {
        return Ok(None);
    }
    // Where it was before a move is affected too.
    let also_issues: Vec<Uuid> = before
        .parent_id
        .filter(|p| Some(*p) != after.parent_id)
        .into_iter()
        .collect();
    let also_projects: Vec<Uuid> = before.project_id.into_iter().collect();
    let context = issue_context(db, &after, &also_issues, &also_projects, false).await?;
    let subject = issue_subject(&after, AssessmentCause::Updated);
    assess(
        engine,
        db,
        actor,
        subject,
        ChangeSet::update(was, now),
        context,
    )
    .await
}

/// Assesses a new issue (version 1.0.0, pinned by `repo::issues::create` to
/// what it is part of): what adding it does to its parent and siblings.
pub async fn issue_created(
    engine: &Assessor,
    db: &mut PgConnection,
    actor: Uuid,
    id: Uuid,
) -> Result<Option<NewAssessment>, sqlx::Error> {
    let Some(node) = repo::impact::issue(&mut *db, id).await? else {
        return Ok(None);
    };
    let context = issue_context(db, &node, &[], &[], false).await?;
    let subject = issue_subject(&node, AssessmentCause::Created);
    assess(
        engine,
        db,
        Some(actor),
        subject,
        ChangeSet::create(node.entity()),
        context,
    )
    .await
}

/// Assesses the deletion of issue `id`, before it is deleted in the same
/// transaction: its sub-issues lose their parent, its parent part of its scope.
pub async fn issue_deleted(
    engine: &Assessor,
    db: &mut PgConnection,
    actor: Uuid,
    id: Uuid,
) -> Result<Option<NewAssessment>, sqlx::Error> {
    let Some(node) = repo::impact::issue_for_update(&mut *db, id).await? else {
        return Ok(None);
    };
    let context = issue_context(db, &node, &[], &[], false).await?;
    let subject = issue_subject(&node, AssessmentCause::Deleted);
    assess(
        engine,
        db,
        Some(actor),
        subject,
        ChangeSet::delete(node.entity()),
        context,
    )
    .await
}

/// Assesses a comment on issue `id`: the engine reads it for blockers, scope
/// changes, risks and slips. A comment moves no version.
pub async fn issue_commented(
    engine: &Assessor,
    db: &mut PgConnection,
    actor: Uuid,
    id: Uuid,
    body: &str,
) -> Result<Option<NewAssessment>, sqlx::Error> {
    let Some(node) = repo::impact::issue(&mut *db, id).await? else {
        return Ok(None);
    };
    let context = issue_context(db, &node, &[], &[], true).await?;
    let event = ActivityEvent {
        entity_id: node.id.to_string(),
        kind: ActivityKind::Comment,
        author: Some(actor.to_string()),
        body: body.to_owned(),
        at: Some(Utc::now()),
    };
    let subject = issue_subject(&node, AssessmentCause::Comment);
    let change = ChangeSet::activity(node.id.to_string(), vec![event]);
    assess(engine, db, Some(actor), subject, change, context).await
}

/// Assesses the edit that turned `before` (read with
/// [`repo::impact::project_for_update`] before the write) into the project as
/// it is now, and versions it.
pub async fn project_updated(
    engine: &Assessor,
    db: &mut PgConnection,
    actor: Uuid,
    before: &ProjectNode,
) -> Result<Option<NewAssessment>, sqlx::Error> {
    let Some(after) = repo::impact::project(&mut *db, before.id).await? else {
        return Ok(None);
    };
    let (was, now) = (before.entity(), after.entity());
    if was.fields == now.fields {
        return Ok(None);
    }
    let context = project_context(db, after.id).await?;
    let subject = project_subject(&after, AssessmentCause::Updated);
    assess(
        engine,
        db,
        Some(actor),
        subject,
        ChangeSet::update(was, now),
        context,
    )
    .await
}

/// Assesses the deletion of project `id`, before it is deleted in the same
/// transaction: its issues stay, without a project.
pub async fn project_deleted(
    engine: &Assessor,
    db: &mut PgConnection,
    actor: Uuid,
    id: Uuid,
) -> Result<Option<NewAssessment>, sqlx::Error> {
    let Some(node) = repo::impact::project_for_update(&mut *db, id).await? else {
        return Ok(None);
    };
    let context = project_context(db, node.id).await?;
    let subject = project_subject(&node, AssessmentCause::Deleted);
    assess(
        engine,
        db,
        Some(actor),
        subject,
        ChangeSet::delete(node.entity()),
        context,
    )
    .await
}

async fn project_context(
    db: &mut PgConnection,
    project_id: Uuid,
) -> Result<Vec<Entity>, sqlx::Error> {
    let issues = repo::impact::project_neighbourhood(&mut *db, project_id).await?;
    Ok(issues.iter().map(IssueNode::entity).collect())
}
