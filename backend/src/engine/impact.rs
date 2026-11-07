//! Assesses every change to an issue or a project with the change-impact
//! engine (`vendor/assessment-core`) and versions it.
//!
//! The engine runs in-process and statelessly: PostgreSQL holds the versions
//! and the pins, and each assessment is given the changed item with its
//! neighbourhood read from there — its parent chain, sub-issues, siblings and
//! projects — in the transaction that writes the change. So the version moves
//! with the change or not at all, and two edits of one item take turns (its
//! row is locked from the read of its "before" to the commit).
//!
//! For each change it writes the item's next version (MAJOR, MINOR or PATCH),
//! keeps the assessment when it moved a version or asks anything of anyone,
//! and, when the engine recommends a review or a re-plan, puts an `impact`
//! entry in the inbox of the owners of every other affected item. An engine that
//! refuses a change (it never should with nexc's limits) is logged and leaves
//! the change unassessed rather than failing the edit.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use assessment_core as ac;
use assessment_core::model::{
    ActivityEvent, ActivityKind, AssessOptions, AssessmentRequest, ChangeSet, Entity, HistoryPoint,
};
use chrono::Utc;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::domain::impact::{
    AssessmentCause, HISTORY_MAX, IMPACTS_MAX, ImpactAction, ImpactEntityKind, IssueNode,
    NewAssessment, ProjectNode, VersionBump,
};
use crate::repo;

/// The engine with its default policy; immutable, shared by every request.
static ENGINE: LazyLock<ac::Engine> = LazyLock::new(ac::Engine::default);

/// What was changed, for the assessment's row.
struct Subject {
    kind: ImpactEntityKind,
    id: Uuid,
    workspace_id: Uuid,
    team_id: Option<Uuid>,
    cause: AssessmentCause,
}

/// Runs the engine on a blocking thread: its text measures are CPU work.
async fn run(request: AssessmentRequest) -> Option<ac::Assessment> {
    match tokio::task::spawn_blocking(move || ENGINE.assess(&request)).await {
        Ok(Ok(assessment)) => Some(assessment),
        Ok(Err(err)) => {
            tracing::warn!(error = %err, kind = %err.kind(), "the change-impact engine refused a change");
            None
        }
        Err(err) => {
            tracing::error!(error = %err, "the change-impact engine stopped");
            None
        }
    }
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
    let Some(assessment) = run(request).await else {
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
    if a.worth_keeping() {
        repo::impact::insert_assessment(&mut *db, &a).await?;
    }
    if a.asks_for_review() {
        // Only the owners of the other items: the changed item's own hear of it from its timeline
        // and the entries an edit already sends (assigned, state, comment), and a deleted one's
        // entries would go with it.
        let acted_on = a
            .impacts
            .iter()
            .filter(|i| i.action != ImpactAction::None && !i.changed);
        let (issues, projects): (Vec<_>, Vec<_>) =
            acted_on.partition(|i| i.entity_kind == ImpactEntityKind::Issue);
        let issues: Vec<Uuid> = issues.iter().map(|i| i.entity_id).collect();
        let projects: Vec<Uuid> = projects.iter().map(|i| i.entity_id).collect();
        // A project's lead hears about the changed issue; a deleted one has nothing to point at.
        let about = (subject.kind == ImpactEntityKind::Issue
            && subject.cause != AssessmentCause::Deleted)
            .then_some(subject.id);
        repo::impact::notify_impacted(&mut *db, actor, &issues, &projects, about).await?;
    }
    Ok(Some(a))
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
    db: &mut PgConnection,
    actor: Uuid,
    before: &IssueNode,
) -> Result<Option<NewAssessment>, sqlx::Error> {
    issue_changed(db, Some(actor), before).await
}

/// Assesses what a finished run did to the issues its graph executes (a
/// successful one moves them to review), given them as they were before
/// ([`repo::impact::issues_of_graph_for_update`]): a change nobody made.
pub async fn issues_reported_to(
    db: &mut PgConnection,
    before: &[IssueNode],
) -> Result<(), sqlx::Error> {
    for node in before {
        issue_changed(db, None, node).await?;
    }
    Ok(())
}

async fn issue_changed(
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
    assess(db, actor, subject, ChangeSet::update(was, now), context).await
}

/// Assesses a new issue (version 1.0.0, pinned by `repo::issues::create` to
/// what it is part of): what adding it does to its parent and siblings.
pub async fn issue_created(
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
    assess(db, Some(actor), subject, change, context).await
}

/// Assesses the edit that turned `before` (read with
/// [`repo::impact::project_for_update`] before the write) into the project as
/// it is now, and versions it.
pub async fn project_updated(
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
