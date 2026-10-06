//! Issues, the workflow states of a team, and projects.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::NaiveDate;
use serde::{Deserialize, Deserializer};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::teams::visible_team;
use super::workspaces::{audit, member_of};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::audit::AuditAction;
use crate::domain::issue::{
    COMMENT_MAX_BYTES, DESCRIPTION_MAX_BYTES, ISSUE_LABELS_MAX, Issue, IssueEvent, IssueEventKind,
    IssuePerson, IssueState, LABEL_NAME_MAX, LABELS_MAX, Label, Notification, NotificationKind,
    PRIORITY_MAX, Project, ProjectStatus, STATE_NAME_MAX, STATES_MAX, StateCategory, TITLE_MAX,
    changes, is_hex_color,
};
use crate::domain::validation::{FieldErrors, Validate, check_text};
use crate::domain::workspace::TeamAccess;
use crate::engine::editor;
use crate::http::extract::{AuthUser, Path, Query, ValidatedJson};
use crate::http::problem::Problem;
use crate::repo::audit::Subject;
use crate::repo::issues::{IssueFilter, NewIssue};
use crate::repo::{self, OrNotFound};

fn double_option<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

fn check_description(errors: &mut FieldErrors, description: &str) {
    if description.len() > DESCRIPTION_MAX_BYTES {
        errors.add("description", "must be at most 64 KiB");
    }
}

fn check_priority(errors: &mut FieldErrors, priority: i16) {
    if !(0..=PRIORITY_MAX).contains(&priority) {
        errors.add(
            "priority",
            "must be 0 (none), 1 (urgent), 2 (high), 3 (medium) or 4 (low)",
        );
    }
}

fn check_label_count(errors: &mut FieldErrors, labels: &[Uuid]) {
    if labels.len() > ISSUE_LABELS_MAX {
        errors.add("label_ids", "an issue carries at most 20 labels");
    }
}

/// The distinct `ids`, checked to be labels of the workspace.
async fn known_labels(
    state: &AppState,
    workspace_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, AppError> {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let known = repo::issues::count_known_labels(&state.db, workspace_id, &ids).await?;
    if usize::try_from(known).ok() != Some(ids.len()) {
        return Err(AppError::field(
            "label_ids",
            "unknown label in this workspace",
        ));
    }
    Ok(ids)
}

fn require_issue_rights(access: TeamAccess) -> Result<(), AppError> {
    if access.can_file_issues() {
        Ok(())
    } else {
        Err(AppError::Forbidden(
            "guests can only work on issues of the teams they belong to".into(),
        ))
    }
}

/// Checks that what an issue points at exists where the issue lives: the
/// assignee in the workspace, the agent and the project in the workspace.
async fn check_references(
    state: &AppState,
    workspace_id: Uuid,
    assignee_id: Option<Uuid>,
    agent_id: Option<Uuid>,
    project_id: Option<Uuid>,
) -> Result<(), AppError> {
    let db = &state.db;
    if let Some(user) = assignee_id
        && repo::workspaces::role_of(db, workspace_id, user)
            .await?
            .is_none()
    {
        return Err(AppError::field(
            "assignee_id",
            "the assignee must be a member of the workspace",
        ));
    }
    if let Some(agent) = agent_id
        && repo::agents::find(db, workspace_id, agent).await?.is_none()
    {
        return Err(AppError::field(
            "agent_id",
            "unknown agent in this workspace",
        ));
    }
    if let Some(project) = project_id
        && !repo::issues::project_in_workspace(db, workspace_id, project).await?
    {
        return Err(AppError::field(
            "project_id",
            "unknown project in this workspace",
        ));
    }
    Ok(())
}

// ---------- issues ----------

/// Query of `GET /workspaces/{wid}/issues`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct IssueQuery {
    pub team_id: Option<Uuid>,
    pub assignee_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    /// Only issues that carry this label.
    pub label_id: Option<Uuid>,
    /// `true` leaves out completed and canceled issues.
    pub open: Option<bool>,
    /// Matches the title, or the start of the identifier (`ENG-1`).
    pub q: Option<String>,
    /// 1-500, default 200.
    pub limit: Option<i64>,
}

/// `POST /workspaces/{wid}/teams/{tid}/issues` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateIssue {
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// A state of the team's workflow (default: its first "unstarted" state).
    pub state_id: Option<Uuid>,
    /// `0` none (default), `1` urgent, `2` high, `3` medium, `4` low.
    pub priority: Option<i16>,
    pub assignee_id: Option<Uuid>,
    pub agent_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    /// Labels of the workspace to put on the issue (at most 20).
    #[serde(default)]
    pub label_ids: Vec<Uuid>,
}

impl Validate for CreateIssue {
    fn validate(&self, errors: &mut FieldErrors) {
        check_text(errors, "title", &self.title, TITLE_MAX);
        check_description(errors, &self.description);
        check_priority(errors, self.priority.unwrap_or_default());
        check_label_count(errors, &self.label_ids);
    }
}

/// `PATCH /issues/{iid}` body: any subset. `assignee_id`, `agent_id` and
/// `project_id` accept `null` to clear them.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateIssue {
    pub title: Option<String>,
    pub description: Option<String>,
    pub state_id: Option<Uuid>,
    pub priority: Option<i16>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<Uuid>, nullable)]
    pub assignee_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<Uuid>, nullable)]
    pub agent_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<Uuid>, nullable)]
    pub project_id: Option<Option<Uuid>>,
    /// Replaces the issue's labels.
    pub label_ids: Option<Vec<Uuid>>,
}

impl Validate for UpdateIssue {
    fn validate(&self, errors: &mut FieldErrors) {
        if let Some(title) = &self.title {
            check_text(errors, "title", title, TITLE_MAX);
        }
        if let Some(description) = &self.description {
            check_description(errors, description);
        }
        if let Some(priority) = self.priority {
            check_priority(errors, priority);
        }
        if let Some(labels) = &self.label_ids {
            check_label_count(errors, labels);
        }
    }
}

/// Issues of a workspace in the teams the caller can see, most recently
/// updated first.
#[utoipa::path(get, path = "/workspaces/{wid}/issues", tag = "issues", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), IssueQuery),
    responses((status = 200, body = [Issue]), (status = 404, body = Problem)))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    Query(query): Query<IssueQuery>,
) -> Result<Json<Vec<Issue>>, AppError> {
    member_of(&state, auth, wid).await?;
    let filter = IssueFilter {
        team_id: query.team_id,
        assignee_id: query.assignee_id,
        project_id: query.project_id,
        label_id: query.label_id,
        open_only: query.open.unwrap_or(false),
        q: query
            .q
            .map(|q| q.trim().chars().take(200).collect::<String>())
            .filter(|q| !q.is_empty()),
        limit: query.limit.unwrap_or(200).clamp(1, 500),
    };
    Ok(Json(
        repo::issues::list(&state.db, auth.id, wid, &filter).await?,
    ))
}

/// Files an issue with a team. It gets the team's next number.
#[utoipa::path(post, path = "/workspaces/{wid}/teams/{tid}/issues", tag = "issues", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("tid" = Uuid, Path, description = "Team id")),
    request_body = CreateIssue,
    responses((status = 201, body = Issue), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 422, body = Problem)))]
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, tid)): Path<(Uuid, Uuid)>,
    ValidatedJson(req): ValidatedJson<CreateIssue>,
) -> Result<(StatusCode, Json<Issue>), AppError> {
    require_issue_rights(visible_team(&state, auth, wid, tid).await?.1)?;
    let issue_state = match req.state_id {
        Some(id) => repo::issues::find_state(&state.db, tid, id)
            .await?
            .ok_or_else(|| AppError::field("state_id", "not a state of this team's workflow"))?,
        None => repo::issues::default_state(&state.db, tid)
            .await?
            .ok_or_else(|| AppError::Unprocessable("this team has no workflow states".into()))?,
    };
    check_references(&state, wid, req.assignee_id, req.agent_id, req.project_id).await?;
    let labels = known_labels(&state, wid, &req.label_ids).await?;
    let new = NewIssue {
        workspace_id: wid,
        team_id: tid,
        title: req.title.trim(),
        description: &req.description,
        state_id: issue_state.id,
        closed: issue_state.category.is_closed(),
        priority: req.priority.unwrap_or_default(),
        assignee_id: req.assignee_id,
        agent_id: req.agent_id,
        project_id: req.project_id,
        creator_id: auth.id,
    };
    let mut tx = state.db.begin().await?;
    let id = repo::issues::create(&mut tx, &new).await?;
    if !labels.is_empty() {
        repo::issues::set_labels(&mut tx, id, &labels).await?;
    }
    if let Some(assignee) = req.assignee_id.filter(|a| *a != auth.id) {
        repo::issues::notify(&mut *tx, id, assignee, auth.id, NotificationKind::Assigned).await?;
    }
    tx.commit().await?;
    let issue = repo::issues::find(&state.db, auth.id, id)
        .await
        .or_not_found("issue")?;
    Ok((StatusCode::CREATED, Json(issue)))
}

/// One issue.
#[utoipa::path(get, path = "/issues/{iid}", tag = "issues", security(("bearer" = [])),
    params(("iid" = Uuid, Path, description = "Issue id")),
    responses((status = 200, body = Issue), (status = 404, body = Problem)))]
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(iid): Path<Uuid>,
) -> Result<Json<Issue>, AppError> {
    Ok(Json(
        repo::issues::find(&state.db, auth.id, iid)
            .await
            .or_not_found("issue")?,
    ))
}

/// Loads an issue the caller may change.
async fn editable(state: &AppState, auth: AuthUser, iid: Uuid) -> Result<Issue, AppError> {
    let issue = repo::issues::find(&state.db, auth.id, iid)
        .await
        .or_not_found("issue")?;
    let (_, access) = visible_team(state, auth, issue.workspace_id, issue.team_id).await?;
    require_issue_rights(access)?;
    Ok(issue)
}

/// Updates an issue. Moving it to a completed or canceled state stamps
/// `completed_at`; moving it back clears it.
#[utoipa::path(patch, path = "/issues/{iid}", tag = "issues", security(("bearer" = [])),
    params(("iid" = Uuid, Path, description = "Issue id")), request_body = UpdateIssue,
    responses((status = 200, body = Issue), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 422, body = Problem)))]
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(iid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<UpdateIssue>,
) -> Result<Json<Issue>, AppError> {
    let mut issue = editable(&state, auth, iid).await?;
    let before = issue.clone();
    if let Some(id) = req.state_id {
        issue.state = repo::issues::find_state(&state.db, issue.team_id, id)
            .await?
            .ok_or_else(|| AppError::field("state_id", "not a state of this team's workflow"))?;
    }
    check_references(
        &state,
        issue.workspace_id,
        req.assignee_id.flatten(),
        req.agent_id.flatten(),
        req.project_id.flatten(),
    )
    .await?;
    let labels = match &req.label_ids {
        Some(ids) => Some(known_labels(&state, issue.workspace_id, ids).await?),
        None => None,
    };
    if let Some(title) = req.title {
        issue.title = title.trim().to_owned();
    }
    if let Some(description) = req.description {
        issue.description = description;
    }
    if let Some(priority) = req.priority {
        issue.priority = priority;
    }
    if let Some(assignee) = req.assignee_id {
        // The name is read back below; only the id is written.
        issue.assignee = assignee.map(|user_id| IssuePerson {
            user_id,
            name: String::new(),
        });
    }
    if let Some(agent) = req.agent_id {
        issue.agent_id = agent;
    }
    if let Some(project) = req.project_id {
        issue.project_id = project;
    }
    let mut tx = state.db.begin().await?;
    repo::issues::save(&mut *tx, &issue).await?;
    if let Some(labels) = &labels {
        repo::issues::set_labels(&mut tx, iid, labels).await?;
    }
    // Read back for the assignee's name, which the timeline shows.
    let after = repo::issues::find(&mut *tx, auth.id, iid)
        .await
        .or_not_found("issue")?;
    let changed = changes(&before, &after);
    repo::issues::record_changes(&mut tx, iid, auth.id, &changed).await?;
    for change in &changed {
        match change.kind {
            IssueEventKind::Assignee => {
                let assignee = after.assignee.as_ref().map(|a| a.user_id);
                if let Some(user) = assignee.filter(|a| *a != auth.id) {
                    let kind = NotificationKind::Assigned;
                    repo::issues::notify(&mut *tx, iid, user, auth.id, kind).await?;
                }
            }
            IssueEventKind::State => {
                let kind = NotificationKind::State;
                repo::issues::notify_watchers(&mut *tx, iid, auth.id, kind).await?;
            }
            _ => {}
        }
    }
    tx.commit().await?;
    Ok(Json(after))
}

/// Deletes an issue (a graph created for it stays).
#[utoipa::path(delete, path = "/issues/{iid}", tag = "issues", security(("bearer" = [])),
    params(("iid" = Uuid, Path, description = "Issue id")),
    responses((status = 204, description = "Deleted"), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(iid): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let issue = editable(&state, auth, iid).await?;
    repo::issues::delete(&state.db, issue.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- inbox ----------

/// `POST /workspaces/{wid}/inbox/read` body.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkRead {
    /// The notifications to mark read; all of them when left out.
    pub ids: Option<Vec<Uuid>>,
}

impl Validate for MarkRead {
    fn validate(&self, errors: &mut FieldErrors) {
        if self.ids.as_ref().is_some_and(|ids| ids.len() > 500) {
            errors.add("ids", "at most 500 at a time");
        }
    }
}

/// The caller's inbox in a workspace, newest first (the last 100): issues
/// assigned to them, and comments on and moves of issues they created or
/// are assigned.
#[utoipa::path(get, path = "/workspaces/{wid}/inbox", tag = "issues", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, body = [Notification]), (status = 404, body = Problem)))]
pub async fn inbox(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Json<Vec<Notification>>, AppError> {
    member_of(&state, auth, wid).await?;
    Ok(Json(
        repo::issues::inbox(&state.db, auth.id, wid, 100).await?,
    ))
}

/// Marks the caller's notifications read and returns the inbox.
#[utoipa::path(post, path = "/workspaces/{wid}/inbox/read", tag = "issues", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")), request_body = MarkRead,
    responses((status = 200, body = [Notification]), (status = 404, body = Problem), (status = 422, body = Problem)))]
pub async fn mark_read(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<MarkRead>,
) -> Result<Json<Vec<Notification>>, AppError> {
    member_of(&state, auth, wid).await?;
    repo::issues::mark_read(&state.db, auth.id, wid, req.ids.as_deref()).await?;
    Ok(Json(
        repo::issues::inbox(&state.db, auth.id, wid, 100).await?,
    ))
}

// ---------- labels ----------

/// `POST /workspaces/{wid}/labels` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateLabel {
    pub name: String,
    /// `#rrggbb`.
    pub color: String,
}

/// `PATCH /workspaces/{wid}/labels/{lid}` body: any subset.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateLabel {
    pub name: Option<String>,
    pub color: Option<String>,
}

fn check_label(errors: &mut FieldErrors, name: Option<&str>, color: Option<&str>) {
    if let Some(name) = name {
        check_text(errors, "name", name, LABEL_NAME_MAX);
    }
    if let Some(color) = color
        && !is_hex_color(color)
    {
        errors.add("color", "must be a colour like #10b981");
    }
}

impl Validate for CreateLabel {
    fn validate(&self, errors: &mut FieldErrors) {
        check_label(errors, Some(&self.name), Some(&self.color));
    }
}

impl Validate for UpdateLabel {
    fn validate(&self, errors: &mut FieldErrors) {
        check_label(errors, self.name.as_deref(), self.color.as_deref());
    }
}

fn label_taken(err: sqlx::Error) -> AppError {
    match &err {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            AppError::Conflict("this workspace already has a label with that name".into())
        }
        _ => err.into(),
    }
}

/// The labels of a workspace, by name.
#[utoipa::path(get, path = "/workspaces/{wid}/labels", tag = "issues", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, body = [Label]), (status = 404, body = Problem)))]
pub async fn labels(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Json<Vec<Label>>, AppError> {
    member_of(&state, auth, wid).await?;
    Ok(Json(repo::issues::labels(&state.db, wid).await?))
}

/// Adds a label to the workspace. Everyone but guests can.
#[utoipa::path(post, path = "/workspaces/{wid}/labels", tag = "issues", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")), request_body = CreateLabel,
    responses((status = 201, body = Label), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "Name taken", body = Problem), (status = 422, body = Problem)))]
pub async fn create_label(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<CreateLabel>,
) -> Result<(StatusCode, Json<Label>), AppError> {
    let workspace = member_of(&state, auth, wid).await?;
    if !workspace.role.is_member() {
        return Err(AppError::Forbidden("guests cannot add labels".into()));
    }
    if repo::issues::count_labels(&state.db, wid).await? >= LABELS_MAX {
        return Err(AppError::Unprocessable(format!(
            "a workspace has at most {LABELS_MAX} labels"
        )));
    }
    let label = repo::issues::create_label(&state.db, wid, req.name.trim(), &req.color)
        .await
        .map_err(label_taken)?;
    Ok((StatusCode::CREATED, Json(label)))
}

/// Renames or recolours a label. Everyone but guests can.
#[utoipa::path(patch, path = "/workspaces/{wid}/labels/{lid}", tag = "issues", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("lid" = Uuid, Path, description = "Label id")),
    request_body = UpdateLabel,
    responses((status = 200, body = Label), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "Name taken", body = Problem), (status = 422, body = Problem)))]
pub async fn update_label(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, lid)): Path<(Uuid, Uuid)>,
    ValidatedJson(req): ValidatedJson<UpdateLabel>,
) -> Result<Json<Label>, AppError> {
    let workspace = member_of(&state, auth, wid).await?;
    if !workspace.role.is_member() {
        return Err(AppError::Forbidden("guests cannot change labels".into()));
    }
    let mut label = repo::issues::find_label(&state.db, wid, lid)
        .await
        .or_not_found("label")?;
    if let Some(name) = req.name {
        label.name = name.trim().to_owned();
    }
    if let Some(color) = req.color {
        label.color = color;
    }
    repo::issues::save_label(&state.db, &label)
        .await
        .map_err(label_taken)?;
    Ok(Json(label))
}

/// Deletes a label and takes it off every issue. Workspace admins only.
#[utoipa::path(delete, path = "/workspaces/{wid}/labels/{lid}", tag = "issues", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("lid" = Uuid, Path, description = "Label id")),
    responses((status = 204, description = "Deleted"), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn delete_label(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, lid)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    let workspace = member_of(&state, auth, wid).await?;
    if !workspace.role.is_admin() {
        return Err(AppError::Forbidden(
            "only workspace admins delete labels".into(),
        ));
    }
    let label = repo::issues::find_label(&state.db, wid, lid)
        .await
        .or_not_found("label")?;
    repo::issues::delete_label(&state.db, label.id).await?;
    let subject = Subject::Text(&label.name);
    audit(&state, wid, auth.id, AuditAction::LabelDeleted, subject, "").await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- timeline ----------

/// `POST /issues/{iid}/comments` and `PATCH /issues/{iid}/comments/{cid}` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CommentBody {
    /// Markdown, 1 byte to 16 KiB.
    pub body: String,
}

impl Validate for CommentBody {
    fn validate(&self, errors: &mut FieldErrors) {
        if self.body.trim().is_empty() {
            errors.add("body", "must not be empty");
        } else if self.body.len() > COMMENT_MAX_BYTES {
            errors.add("body", "must be at most 16 KiB");
        }
    }
}

/// The timeline of an issue, oldest first: its comments and the changes to
/// its state, priority, assignee and title.
#[utoipa::path(get, path = "/issues/{iid}/events", tag = "issues", security(("bearer" = [])),
    params(("iid" = Uuid, Path, description = "Issue id")),
    responses((status = 200, body = [IssueEvent]), (status = 404, body = Problem)))]
pub async fn events(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(iid): Path<Uuid>,
) -> Result<Json<Vec<IssueEvent>>, AppError> {
    let issue = repo::issues::find(&state.db, auth.id, iid)
        .await
        .or_not_found("issue")?;
    Ok(Json(repo::issues::events(&state.db, issue.id).await?))
}

/// Comments on an issue. Whoever may edit the issue may comment on it.
#[utoipa::path(post, path = "/issues/{iid}/comments", tag = "issues", security(("bearer" = [])),
    params(("iid" = Uuid, Path, description = "Issue id")), request_body = CommentBody,
    responses((status = 201, body = IssueEvent), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 422, body = Problem)))]
pub async fn create_comment(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(iid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<CommentBody>,
) -> Result<(StatusCode, Json<IssueEvent>), AppError> {
    let issue = editable(&state, auth, iid).await?;
    let mut tx = state.db.begin().await?;
    let id = repo::issues::add_comment(&mut *tx, issue.id, auth.id, req.body.trim()).await?;
    repo::issues::notify_watchers(&mut *tx, issue.id, auth.id, NotificationKind::Comment).await?;
    tx.commit().await?;
    let comment = repo::issues::find_comment(&state.db, issue.id, id)
        .await
        .or_not_found("comment")?;
    Ok((StatusCode::CREATED, Json(comment)))
}

/// Edits a comment. Only its author can.
#[utoipa::path(patch, path = "/issues/{iid}/comments/{cid}", tag = "issues", security(("bearer" = [])),
    params(("iid" = Uuid, Path, description = "Issue id"), ("cid" = Uuid, Path, description = "Comment id")),
    request_body = CommentBody,
    responses((status = 200, body = IssueEvent), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 422, body = Problem)))]
pub async fn update_comment(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((iid, cid)): Path<(Uuid, Uuid)>,
    ValidatedJson(req): ValidatedJson<CommentBody>,
) -> Result<Json<IssueEvent>, AppError> {
    let issue = editable(&state, auth, iid).await?;
    let comment = repo::issues::find_comment(&state.db, issue.id, cid)
        .await
        .or_not_found("comment")?;
    if comment.actor.as_ref().map(|a| a.user_id) != Some(auth.id) {
        return Err(AppError::Forbidden(
            "only its author can edit a comment".into(),
        ));
    }
    repo::issues::edit_comment(&state.db, cid, req.body.trim()).await?;
    Ok(Json(
        repo::issues::find_comment(&state.db, issue.id, cid)
            .await
            .or_not_found("comment")?,
    ))
}

/// Deletes a comment: its author, or whoever manages the team.
#[utoipa::path(delete, path = "/issues/{iid}/comments/{cid}", tag = "issues", security(("bearer" = [])),
    params(("iid" = Uuid, Path, description = "Issue id"), ("cid" = Uuid, Path, description = "Comment id")),
    responses((status = 204, description = "Deleted"), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn delete_comment(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((iid, cid)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    let issue = repo::issues::find(&state.db, auth.id, iid)
        .await
        .or_not_found("issue")?;
    let (_, access) = visible_team(&state, auth, issue.workspace_id, issue.team_id).await?;
    let comment = repo::issues::find_comment(&state.db, issue.id, cid)
        .await
        .or_not_found("comment")?;
    let own = comment.actor.as_ref().map(|a| a.user_id) == Some(auth.id);
    if !(own && access.can_file_issues() || access.can_manage()) {
        return Err(AppError::Forbidden(
            "only its author or a team owner can delete a comment".into(),
        ));
    }
    repo::issues::delete_comment(&state.db, cid).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Gives the issue a graph that plans and executes it: a graph of the issue's
/// team whose goal is the issue's title and description. Calling it again
/// returns the issue with the graph it already has.
#[utoipa::path(post, path = "/issues/{iid}/graph", tag = "issues", security(("bearer" = [])),
    params(("iid" = Uuid, Path, description = "Issue id")),
    responses((status = 200, body = Issue), (status = 403, description = "Only team members run its issues", body = Problem),
        (status = 404, body = Problem)))]
pub async fn create_graph(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(iid): Path<Uuid>,
) -> Result<Json<Issue>, AppError> {
    let issue = editable(&state, auth, iid).await?;
    if issue.graph_id.is_some() {
        return Ok(Json(issue));
    }
    // Graphs of a team are open to its members only, so only a member can start one.
    let home = editor::graph_home(&state, auth.id, None, Some(issue.team_id))
        .await
        .map_err(|_| {
            AppError::Forbidden("join the team to plan and run its issues as graphs".into())
        })?;
    let name: String = format!("{} {}", issue.identifier, issue.title)
        .chars()
        .take(crate::domain::graph::GRAPH_NAME_MAX)
        .collect();
    let goal = if issue.description.trim().is_empty() {
        issue.title.clone()
    } else {
        format!("{}\n\n{}", issue.title, issue.description.trim())
    };
    let goal: String = goal
        .chars()
        .take(crate::domain::graph::GRAPH_TEXT_MAX)
        .collect();
    let mut tx = state.db.begin().await?;
    let graph = repo::graphs::create(
        &mut *tx,
        auth.id,
        home.workspace_id,
        home.team_id,
        &name,
        &format!("Plans and executes {}", issue.identifier),
        &goal,
    )
    .await?;
    repo::issues::link_graph(&mut *tx, issue.id, graph.id).await?;
    tx.commit().await?;
    Ok(Json(
        repo::issues::find(&state.db, auth.id, iid)
            .await
            .or_not_found("issue")?,
    ))
}

// ---------- workflow states ----------

/// `POST .../states` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateState {
    pub name: String,
    pub category: StateCategory,
    /// `#rrggbb`.
    pub color: String,
    /// Order within the workflow (default: after the existing states).
    pub position: Option<i32>,
}

/// `PATCH .../states/{sid}` body (any subset).
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateState {
    pub name: Option<String>,
    pub category: Option<StateCategory>,
    pub color: Option<String>,
    pub position: Option<i32>,
}

fn check_state(errors: &mut FieldErrors, name: Option<&str>, color: Option<&str>) {
    if let Some(name) = name {
        check_text(errors, "name", name, STATE_NAME_MAX);
    }
    if color.is_some_and(|c| !is_hex_color(c)) {
        errors.add("color", "must be a #rrggbb colour");
    }
}

impl Validate for CreateState {
    fn validate(&self, errors: &mut FieldErrors) {
        check_state(errors, Some(&self.name), Some(&self.color));
    }
}

impl Validate for UpdateState {
    fn validate(&self, errors: &mut FieldErrors) {
        check_state(errors, self.name.as_deref(), self.color.as_deref());
    }
}

fn require_manage(access: TeamAccess) -> Result<(), AppError> {
    if access.can_manage() {
        Ok(())
    } else {
        Err(AppError::Forbidden(
            "only team owners and workspace admins can change the workflow".into(),
        ))
    }
}

fn name_taken(err: sqlx::Error) -> AppError {
    match &err {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            AppError::Conflict("this team already has a state with that name".into())
        }
        _ => err.into(),
    }
}

/// The workflow of a team, in order.
#[utoipa::path(get, path = "/workspaces/{wid}/teams/{tid}/states", tag = "issues", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("tid" = Uuid, Path, description = "Team id")),
    responses((status = 200, body = [IssueState]), (status = 404, body = Problem)))]
pub async fn states(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, tid)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<IssueState>>, AppError> {
    visible_team(&state, auth, wid, tid).await?;
    Ok(Json(repo::issues::states(&state.db, tid).await?))
}

/// Adds a state to a team's workflow (team owners and workspace admins).
#[utoipa::path(post, path = "/workspaces/{wid}/teams/{tid}/states", tag = "issues", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("tid" = Uuid, Path, description = "Team id")),
    request_body = CreateState,
    responses((status = 201, body = IssueState), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "Name taken", body = Problem), (status = 422, body = Problem)))]
pub async fn create_state(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, tid)): Path<(Uuid, Uuid)>,
    ValidatedJson(req): ValidatedJson<CreateState>,
) -> Result<(StatusCode, Json<IssueState>), AppError> {
    require_manage(visible_team(&state, auth, wid, tid).await?.1)?;
    let existing = repo::issues::states(&state.db, tid).await?;
    if existing.len() >= STATES_MAX {
        return Err(AppError::Unprocessable(format!(
            "a workflow can have at most {STATES_MAX} states"
        )));
    }
    let position = req
        .position
        .unwrap_or_else(|| existing.iter().map(|s| s.position + 1).max().unwrap_or(0));
    let created = repo::issues::create_state(
        &state.db,
        tid,
        req.name.trim(),
        req.category,
        &req.color,
        position,
    )
    .await
    .map_err(name_taken)?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Renames, recolours, reorders or recategorises a state.
#[utoipa::path(patch, path = "/workspaces/{wid}/teams/{tid}/states/{sid}", tag = "issues", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("tid" = Uuid, Path, description = "Team id"),
        ("sid" = Uuid, Path, description = "State id")),
    request_body = UpdateState,
    responses((status = 200, body = IssueState), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "Name taken", body = Problem)))]
pub async fn update_state(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, tid, sid)): Path<(Uuid, Uuid, Uuid)>,
    ValidatedJson(req): ValidatedJson<UpdateState>,
) -> Result<Json<IssueState>, AppError> {
    require_manage(visible_team(&state, auth, wid, tid).await?.1)?;
    let mut current = repo::issues::find_state(&state.db, tid, sid)
        .await
        .or_not_found("state")?;
    if let Some(name) = req.name {
        current.name = name.trim().to_owned();
    }
    if let Some(category) = req.category {
        current.category = category;
    }
    if let Some(color) = req.color {
        current.color = color;
    }
    if let Some(position) = req.position {
        current.position = position;
    }
    Ok(Json(
        repo::issues::save_state(&state.db, &current)
            .await
            .map_err(name_taken)?,
    ))
}

/// Removes a state no issue is in. A workflow keeps at least one state.
#[utoipa::path(delete, path = "/workspaces/{wid}/teams/{tid}/states/{sid}", tag = "issues", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("tid" = Uuid, Path, description = "Team id"),
        ("sid" = Uuid, Path, description = "State id")),
    responses((status = 204, description = "Deleted"), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "Issues are in this state, or it is the last one", body = Problem)))]
pub async fn delete_state(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, tid, sid)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    require_manage(visible_team(&state, auth, wid, tid).await?.1)?;
    repo::issues::find_state(&state.db, tid, sid)
        .await
        .or_not_found("state")?;
    let in_use = repo::issues::count_in_state(&state.db, sid).await?;
    if in_use > 0 {
        return Err(AppError::Conflict(format!(
            "{in_use} issue(s) are in this state; move them first"
        )));
    }
    if repo::issues::states(&state.db, tid).await?.len() <= 1 {
        return Err(AppError::Conflict(
            "a workflow needs at least one state".into(),
        ));
    }
    repo::issues::delete_state(&state.db, tid, sid).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- projects ----------

/// `POST /workspaces/{wid}/projects` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateProject {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub target_date: Option<NaiveDate>,
}

/// `PATCH /workspaces/{wid}/projects/{pid}` body (any subset; `lead_id` and
/// `target_date` accept `null`).
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateProject {
    pub name: Option<String>,
    pub description: Option<String>,
    pub status: Option<ProjectStatus>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<Uuid>, nullable)]
    pub lead_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<NaiveDate>, nullable)]
    pub target_date: Option<Option<NaiveDate>>,
}

impl Validate for CreateProject {
    fn validate(&self, errors: &mut FieldErrors) {
        check_text(errors, "name", &self.name, TITLE_MAX);
        check_description(errors, &self.description);
    }
}

impl Validate for UpdateProject {
    fn validate(&self, errors: &mut FieldErrors) {
        if let Some(name) = &self.name {
            check_text(errors, "name", name, TITLE_MAX);
        }
        if let Some(description) = &self.description {
            check_description(errors, description);
        }
    }
}

/// 403 for guests: projects span teams, which guests do not.
async fn project_editor(state: &AppState, auth: AuthUser, wid: Uuid) -> Result<(), AppError> {
    if member_of(state, auth, wid).await?.role.is_member() {
        Ok(())
    } else {
        Err(AppError::Forbidden("guests cannot change projects".into()))
    }
}

/// Projects of a workspace, with how many of their issues the caller can see.
#[utoipa::path(get, path = "/workspaces/{wid}/projects", tag = "issues", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, body = [Project]), (status = 404, body = Problem)))]
pub async fn projects(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Json<Vec<Project>>, AppError> {
    member_of(&state, auth, wid).await?;
    Ok(Json(repo::issues::projects(&state.db, auth.id, wid).await?))
}

/// Creates a project led by the caller (members and above).
#[utoipa::path(post, path = "/workspaces/{wid}/projects", tag = "issues", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")), request_body = CreateProject,
    responses((status = 201, body = Project), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn create_project(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<CreateProject>,
) -> Result<(StatusCode, Json<Project>), AppError> {
    project_editor(&state, auth, wid).await?;
    let id = repo::issues::create_project(
        &state.db,
        wid,
        auth.id,
        req.name.trim(),
        &req.description,
        req.target_date,
    )
    .await?;
    let project = repo::issues::find_project(&state.db, auth.id, wid, id)
        .await
        .or_not_found("project")?;
    Ok((StatusCode::CREATED, Json(project)))
}

/// Updates a project (members and above).
#[utoipa::path(patch, path = "/workspaces/{wid}/projects/{pid}", tag = "issues", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("pid" = Uuid, Path, description = "Project id")),
    request_body = UpdateProject,
    responses((status = 200, body = Project), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 422, body = Problem)))]
pub async fn update_project(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, pid)): Path<(Uuid, Uuid)>,
    ValidatedJson(req): ValidatedJson<UpdateProject>,
) -> Result<Json<Project>, AppError> {
    project_editor(&state, auth, wid).await?;
    let mut project = repo::issues::find_project(&state.db, auth.id, wid, pid)
        .await
        .or_not_found("project")?;
    if let Some(Some(lead)) = req.lead_id
        && repo::workspaces::role_of(&state.db, wid, lead)
            .await?
            .is_none()
    {
        return Err(AppError::field(
            "lead_id",
            "the lead must be a member of the workspace",
        ));
    }
    if let Some(name) = req.name {
        project.name = name.trim().to_owned();
    }
    if let Some(description) = req.description {
        project.description = description;
    }
    if let Some(status) = req.status {
        project.status = status;
    }
    if let Some(lead) = req.lead_id {
        project.lead_id = lead;
    }
    if let Some(target) = req.target_date {
        project.target_date = target;
    }
    repo::issues::save_project(&state.db, &project).await?;
    Ok(Json(
        repo::issues::find_project(&state.db, auth.id, wid, pid)
            .await
            .or_not_found("project")?,
    ))
}

/// Deletes a project (workspace admins); its issues stay, without a project.
#[utoipa::path(delete, path = "/workspaces/{wid}/projects/{pid}", tag = "issues", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("pid" = Uuid, Path, description = "Project id")),
    responses((status = 204, description = "Deleted"), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn delete_project(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, pid)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    if !member_of(&state, auth, wid).await?.role.is_admin() {
        return Err(AppError::Forbidden(
            "only workspace admins can delete projects".into(),
        ));
    }
    repo::issues::find_project(&state.db, auth.id, wid, pid)
        .await
        .or_not_found("project")?;
    repo::issues::delete_project(&state.db, pid).await?;
    Ok(StatusCode::NO_CONTENT)
}
