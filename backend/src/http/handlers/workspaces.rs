//! Workspaces, their members and invitations.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::assistant::{AssistantReply, AssistantTurn, MESSAGE_MAX};
use crate::domain::audit::{AuditAction, AuditEntry};
use crate::domain::guardrails::Guardrails;
use crate::domain::usage::{UsageReport, UsageScope};
use crate::domain::user::{check_email, normalize_email};
use crate::domain::validation::{FieldErrors, Validate, check_text};
use crate::domain::workspace::{
    NAME_MAX, SLUG_MAX, Workspace, WorkspaceAction, WorkspaceInvite, WorkspaceMember,
    WorkspaceRole, can, can_assign_role, can_remove_member, slugify,
};
use crate::engine;
use crate::http::extract::{AuthUser, Path, Query, ValidatedJson};
use crate::http::problem::Problem;
use crate::orchestrator;
use crate::repo::audit::Subject;
use crate::repo::usage::Window;
use crate::repo::{self, OrNotFound};

/// Loads a workspace the caller belongs to. A workspace they are not a
/// member of is a 404, exactly like one that does not exist.
pub async fn member_of(state: &AppState, auth: AuthUser, wid: Uuid) -> Result<Workspace, AppError> {
    repo::workspaces::find_for(&state.db, auth.id, wid)
        .await
        .or_not_found("workspace")
}

/// Notes a change in the workspace's audit log. The change has already
/// happened, so a failure to note it is logged, not returned.
pub async fn audit(
    state: &AppState,
    workspace_id: Uuid,
    actor_id: Uuid,
    action: AuditAction,
    subject: Subject<'_>,
    detail: &str,
) {
    if let Err(err) =
        repo::audit::record(&state.db, workspace_id, actor_id, action, subject, detail).await
    {
        tracing::error!(%workspace_id, %action, error = %err, "audit entry was not written");
    }
}

/// 403 unless `workspace.role` may perform `action`.
pub fn require(workspace: &Workspace, action: WorkspaceAction) -> Result<(), AppError> {
    if can(workspace.role, action) {
        Ok(())
    } else {
        Err(AppError::Forbidden(format!(
            "your role ({}) does not allow this",
            workspace.role
        )))
    }
}

/// Creates a workspace owned by `owner`, with a slug derived from `name`
/// (suffixed when taken) and the default agent organisation running on
/// `model`. Runs inside the caller's transaction.
pub async fn create_owned(
    tx: &mut sqlx::PgConnection,
    owner: Uuid,
    name: &str,
    model: &str,
) -> Result<Uuid, AppError> {
    let base = Some(slugify(name))
        .filter(|s| s.len() >= 2)
        .unwrap_or_else(|| "workspace".to_owned());
    let mut slug = base.clone();
    while repo::workspaces::slug_exists(&mut *tx, &slug).await? {
        let suffix = Uuid::new_v4().simple().to_string();
        slug = format!("{base}-{}", &suffix[..8]);
    }
    debug_assert!(slug.len() <= SLUG_MAX);
    let id = repo::workspaces::create(&mut *tx, name, &slug, owner).await?;
    repo::workspaces::add_member(&mut *tx, id, owner, WorkspaceRole::Owner).await?;
    orchestrator::seed_default_org(&mut *tx, owner, id, model).await?;
    Ok(id)
}

/// `POST /workspaces` and `PATCH /workspaces/{wid}` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceInput {
    pub name: String,
}

impl Validate for WorkspaceInput {
    fn validate(&self, errors: &mut FieldErrors) {
        check_text(errors, "name", &self.name, NAME_MAX);
    }
}

/// `POST /workspaces/{wid}/members` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InviteMember {
    pub email: String,
    /// Defaults to `member`.
    pub role: Option<WorkspaceRole>,
}

impl Validate for InviteMember {
    fn validate(&self, errors: &mut FieldErrors) {
        check_email(errors, &normalize_email(&self.email));
    }
}

/// `PATCH /workspaces/{wid}/members/{uid}` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateMember {
    pub role: WorkspaceRole,
}

impl Validate for UpdateMember {
    fn validate(&self, _errors: &mut FieldErrors) {}
}

/// Outcome of inviting someone: they joined at once (they already have an
/// account) or an invitation waits for them to sign up.
#[derive(Debug, Serialize, ToSchema)]
pub struct InviteResult {
    #[schema(required = true)]
    pub member: Option<WorkspaceMember>,
    #[schema(required = true)]
    pub invite: Option<WorkspaceInvite>,
}

/// The workspaces the caller belongs to.
#[utoipa::path(get, path = "/workspaces", tag = "workspaces", security(("bearer" = [])),
    responses((status = 200, body = [Workspace]), (status = 401, body = Problem)))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<Workspace>>, AppError> {
    Ok(Json(repo::workspaces::list_for(&state.db, auth.id).await?))
}

/// Creates a workspace; the caller becomes its owner.
#[utoipa::path(post, path = "/workspaces", tag = "workspaces", security(("bearer" = [])),
    request_body = WorkspaceInput,
    responses((status = 201, body = Workspace), (status = 422, body = Problem)))]
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    ValidatedJson(req): ValidatedJson<WorkspaceInput>,
) -> Result<(StatusCode, Json<Workspace>), AppError> {
    let mut tx = state.db.begin().await?;
    let id = create_owned(&mut tx, auth.id, req.name.trim(), &state.settings.llm_model).await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(member_of(&state, auth, id).await?),
    ))
}

/// One workspace the caller belongs to.
#[utoipa::path(get, path = "/workspaces/{wid}", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, body = Workspace), (status = 404, body = Problem)))]
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Json<Workspace>, AppError> {
    Ok(Json(member_of(&state, auth, wid).await?))
}

/// Renames a workspace (admins and owners).
#[utoipa::path(patch, path = "/workspaces/{wid}", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")), request_body = WorkspaceInput,
    responses((status = 200, body = Workspace), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<WorkspaceInput>,
) -> Result<Json<Workspace>, AppError> {
    let workspace = member_of(&state, auth, wid).await?;
    require(&workspace, WorkspaceAction::UpdateSettings)?;
    repo::workspaces::rename(&state.db, wid, req.name.trim()).await?;
    if workspace.name != req.name.trim() {
        let detail = format!("{} -> {}", workspace.name, req.name.trim());
        let subject = Subject::Text(req.name.trim());
        audit(
            &state,
            wid,
            auth.id,
            AuditAction::WorkspaceRenamed,
            subject,
            &detail,
        )
        .await;
    }
    Ok(Json(member_of(&state, auth, wid).await?))
}

/// Deletes a workspace with everything in it (owners only). A user's last
/// workspace cannot be deleted: every account works in at least one.
#[utoipa::path(delete, path = "/workspaces/{wid}", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 204, description = "Deleted"), (status = 403, body = Problem),
        (status = 404, body = Problem), (status = 409, body = Problem)))]
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::Delete,
    )?;
    if repo::workspaces::count_for(&state.db, auth.id).await? <= 1 {
        return Err(AppError::Conflict(
            "this is your only workspace; create another one before deleting it".into(),
        ));
    }
    engine::workspaces::erase(&state, wid).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Gives every account that belongs to no workspace a personal one, so that
/// everyone who signs in has somewhere to work. Returns how many were made.
pub async fn ensure_personal(state: &AppState) -> Result<usize, AppError> {
    let stranded = repo::workspaces::users_without_workspace(&state.db).await?;
    for (user, name) in &stranded {
        let mut tx = state.db.begin().await?;
        create_owned(
            &mut tx,
            *user,
            &super::auth::personal_workspace_name(name),
            &state.settings.llm_model,
        )
        .await?;
        tx.commit().await?;
    }
    Ok(stranded.len())
}

/// Members of a workspace (not visible to guests).
#[utoipa::path(get, path = "/workspaces/{wid}/members", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, body = [WorkspaceMember]), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn members(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Json<Vec<WorkspaceMember>>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::ViewMembers,
    )?;
    Ok(Json(repo::workspaces::members(&state.db, wid).await?))
}

/// Invites someone by e-mail. A registered user joins immediately; anyone
/// else joins when they sign up with that address.
#[utoipa::path(post, path = "/workspaces/{wid}/members", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")), request_body = InviteMember,
    responses((status = 201, body = InviteResult), (status = 403, body = Problem),
        (status = 404, body = Problem), (status = 409, description = "Already a member", body = Problem)))]
pub async fn invite(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<InviteMember>,
) -> Result<(StatusCode, Json<InviteResult>), AppError> {
    let workspace = member_of(&state, auth, wid).await?;
    let role = req.role.unwrap_or(WorkspaceRole::Member);
    if !can_assign_role(workspace.role, None, role) {
        return Err(AppError::Forbidden(format!(
            "your role ({}) cannot invite a {role}",
            workspace.role
        )));
    }
    let email = normalize_email(&req.email);
    let result = match repo::workspaces::user_id_by_email(&state.db, &email).await? {
        Some(user_id) => {
            if !repo::workspaces::add_member(&state.db, wid, user_id, role).await? {
                return Err(AppError::Conflict(
                    "this person is already a member of the workspace".into(),
                ));
            }
            let member = repo::workspaces::members(&state.db, wid)
                .await?
                .into_iter()
                .find(|m| m.user_id == user_id);
            let detail = format!("as {role}");
            let subject = Subject::User(user_id);
            audit(
                &state,
                wid,
                auth.id,
                AuditAction::MemberAdded,
                subject,
                &detail,
            )
            .await;
            InviteResult {
                member,
                invite: None,
            }
        }
        None => {
            if role == WorkspaceRole::Owner {
                return Err(AppError::field(
                    "role",
                    "ownership can only be given to someone who already has an account",
                ));
            }
            let invite =
                repo::workspaces::upsert_invite(&state.db, wid, &email, role, auth.id).await?;
            let detail = format!("as {role}");
            let subject = Subject::Text(&email);
            audit(
                &state,
                wid,
                auth.id,
                AuditAction::MemberInvited,
                subject,
                &detail,
            )
            .await;
            InviteResult {
                member: None,
                invite: Some(invite),
            }
        }
    };
    Ok((StatusCode::CREATED, Json(result)))
}

/// Changes a member's role. Only owners change owners, and a workspace
/// always keeps at least one owner who can sign in: a suspended owner, or
/// one who administers the platform, does not count.
#[utoipa::path(patch, path = "/workspaces/{wid}/members/{uid}", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("uid" = Uuid, Path, description = "User id")),
    request_body = UpdateMember,
    responses((status = 200, body = [WorkspaceMember]), (status = 403, body = Problem),
        (status = 404, body = Problem), (status = 409, description = "Would leave no owner", body = Problem)))]
pub async fn update_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, uid)): Path<(Uuid, Uuid)>,
    ValidatedJson(req): ValidatedJson<UpdateMember>,
) -> Result<Json<Vec<WorkspaceMember>>, AppError> {
    let workspace = member_of(&state, auth, wid).await?;
    let mut tx = state.db.begin().await?;
    let owners = repo::workspaces::lock_owners(&mut *tx, wid).await?;
    let current = repo::workspaces::role_of(&mut *tx, wid, uid)
        .await
        .or_not_found("member")?;
    if !can_assign_role(workspace.role, Some(current), req.role) {
        return Err(AppError::Forbidden(format!(
            "your role ({}) cannot change a {current} into a {}",
            workspace.role, req.role
        )));
    }
    if current == WorkspaceRole::Owner
        && req.role != WorkspaceRole::Owner
        && !owners.iter().any(|owner| *owner != uid)
    {
        return Err(AppError::Conflict(
            "a workspace needs at least one owner who can sign in".into(),
        ));
    }
    repo::workspaces::set_role(&mut *tx, wid, uid, req.role).await?;
    tx.commit().await?;
    if current != req.role {
        let detail = format!("{current} -> {}", req.role);
        let subject = Subject::User(uid);
        audit(
            &state,
            wid,
            auth.id,
            AuditAction::MemberRoleChanged,
            subject,
            &detail,
        )
        .await;
    }
    Ok(Json(repo::workspaces::members(&state.db, wid).await?))
}

/// Removes a member, or lets the caller leave. The last owner cannot go.
#[utoipa::path(delete, path = "/workspaces/{wid}/members/{uid}", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("uid" = Uuid, Path, description = "User id")),
    responses((status = 204, description = "Removed"), (status = 403, body = Problem),
        (status = 404, body = Problem), (status = 409, description = "Would leave no owner", body = Problem)))]
pub async fn remove_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, uid)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    let workspace = member_of(&state, auth, wid).await?;
    let mut tx = state.db.begin().await?;
    let owners = repo::workspaces::lock_owners(&mut *tx, wid).await?;
    let target = repo::workspaces::role_of(&mut *tx, wid, uid)
        .await
        .or_not_found("member")?;
    if uid != auth.id && !can_remove_member(workspace.role, target) {
        return Err(AppError::Forbidden(format!(
            "your role ({}) cannot remove a {target}",
            workspace.role
        )));
    }
    if target == WorkspaceRole::Owner && !owners.iter().any(|owner| *owner != uid) {
        return Err(AppError::Conflict(
            "a workspace needs at least one owner who can sign in".into(),
        ));
    }
    repo::workspaces::remove_member(&mut tx, wid, uid).await?;
    tx.commit().await?;
    let detail = if uid == auth.id { "left" } else { "removed" };
    let subject = Subject::User(uid);
    audit(
        &state,
        wid,
        auth.id,
        AuditAction::MemberRemoved,
        subject,
        detail,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /workspaces/{wid}/assistant` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AssistantRequest {
    pub message: String,
    /// Earlier turns of this conversation, oldest first (the last 20 are used).
    #[serde(default)]
    pub history: Vec<AssistantTurn>,
}

impl Validate for AssistantRequest {
    fn validate(&self, errors: &mut FieldErrors) {
        check_text(errors, "message", &self.message, MESSAGE_MAX);
        if self
            .history
            .iter()
            .any(|t| t.content.len() > 4 * MESSAGE_MAX)
        {
            errors.add("history", "a turn is too long");
        }
    }
}

/// Asks the workspace assistant. It answers from the workspace's memory and
/// open issues, as far as the caller can see them, and files issues when asked
/// to, with the caller's rights.
#[utoipa::path(post, path = "/workspaces/{wid}/assistant", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")), request_body = AssistantRequest,
    responses((status = 200, body = AssistantReply), (status = 403, description = "Refused by the workspace's guardrails", body = Problem),
        (status = 404, body = Problem), (status = 422, body = Problem)))]
pub async fn assistant(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<AssistantRequest>,
) -> Result<Json<AssistantReply>, AppError> {
    let workspace = member_of(&state, auth, wid).await?;
    Ok(Json(
        engine::assistant::reply(&state, auth.id, &workspace, req.history, req.message.trim())
            .await?,
    ))
}

impl Validate for Guardrails {
    fn validate(&self, errors: &mut FieldErrors) {
        for (field, budget) in [
            ("monthly_token_budget", self.monthly_token_budget),
            (
                "member_monthly_token_budget",
                self.member_monthly_token_budget,
            ),
        ] {
            if budget.is_some_and(|b| b < 0) {
                errors.add(field, "must not be negative");
            }
        }
        // Low values would forget what was learned minutes ago.
        if self.memory_limit.is_some_and(|n| n < 100) {
            errors.add("memory_limit", "must be at least 100");
        }
        if self.memory_forget_after_days.is_some_and(|d| d < 7) {
            errors.add("memory_forget_after_days", "must be at least 7");
        }
    }
}

/// The guardrails of a workspace (the permissive defaults when it set none).
#[utoipa::path(get, path = "/workspaces/{wid}/guardrails", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, body = Guardrails), (status = 404, body = Problem)))]
pub async fn guardrails(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Json<Guardrails>, AppError> {
    member_of(&state, auth, wid).await?;
    Ok(Json(
        repo::workspaces::guardrails(&state.db, wid)
            .await?
            .unwrap_or_default(),
    ))
}

/// Sets the guardrails of a workspace (admins and owners): monthly token
/// budgets, allowed providers, code execution and secret redaction.
#[utoipa::path(put, path = "/workspaces/{wid}/guardrails", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")), request_body = Guardrails,
    responses((status = 200, body = Guardrails), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 422, body = Problem)))]
pub async fn put_guardrails(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<Guardrails>,
) -> Result<Json<Guardrails>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    repo::workspaces::set_guardrails(&state.db, wid, auth.id, &req).await?;
    let subject = Subject::Text("Guardrails");
    audit(
        &state,
        wid,
        auth.id,
        AuditAction::GuardrailsChanged,
        subject,
        "",
    )
    .await;
    Ok(Json(req))
}

/// Query of `GET /workspaces/{wid}/usage`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct UsageQuery {
    /// How many days back to report, 1-365 (default 30).
    pub days: Option<i64>,
}

/// Token usage of a workspace: totals and breakdowns by day, member, model,
/// purpose and paying account. Admins see everyone; other members their own.
#[utoipa::path(get, path = "/workspaces/{wid}/usage", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), UsageQuery),
    responses((status = 200, body = UsageReport), (status = 404, body = Problem)))]
pub async fn usage(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    Query(query): Query<UsageQuery>,
) -> Result<Json<UsageReport>, AppError> {
    let workspace = member_of(&state, auth, wid).await?;
    let everyone = workspace.role.is_admin();
    let window = Window {
        workspace_id: wid,
        days: query.days.unwrap_or(30).clamp(1, 365),
        only_user: (!everyone).then_some(auth.id),
    };
    let db = &state.db;
    let (totals, by_day, by_member, by_model, by_purpose, by_credential) = tokio::try_join!(
        repo::usage::total(db, window),
        repo::usage::by_day(db, window),
        repo::usage::by_member(db, window),
        repo::usage::by_model(db, window),
        repo::usage::by_purpose(db, window),
        repo::usage::by_credential(db, window),
    )?;
    Ok(Json(UsageReport {
        scope: if everyone {
            UsageScope::Workspace
        } else {
            UsageScope::Own
        },
        days: window.days,
        totals,
        by_day,
        by_member,
        by_model,
        by_purpose,
        by_credential,
    }))
}

/// Pending invitations (admins and owners).
#[utoipa::path(get, path = "/workspaces/{wid}/invites", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, body = [WorkspaceInvite]), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn invites(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Json<Vec<WorkspaceInvite>>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::ManageMembers,
    )?;
    Ok(Json(repo::workspaces::invites(&state.db, wid).await?))
}

/// Withdraws an invitation.
#[utoipa::path(delete, path = "/workspaces/{wid}/invites/{iid}", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("iid" = Uuid, Path, description = "Invite id")),
    responses((status = 204, description = "Withdrawn"), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn delete_invite(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, iid)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::ManageMembers,
    )?;
    if repo::workspaces::delete_invite(&state.db, wid, iid).await? {
        let subject = Subject::Text("Invitation");
        audit(
            &state,
            wid,
            auth.id,
            AuditAction::InviteWithdrawn,
            subject,
            "",
        )
        .await;
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound("invite"))
    }
}

/// Query of `GET /workspaces/{wid}/audit`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct AuditQuery {
    /// Only entries older than this (the `created_at` of the last one seen).
    pub before: Option<chrono::DateTime<chrono::Utc>>,
    /// 1-200, default 100.
    pub limit: Option<i64>,
}

/// The audit log of a workspace, newest first (admins and owners): changes
/// to members, invitations, teams, credentials, guardrails and labels.
#[utoipa::path(get, path = "/workspaces/{wid}/audit", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), AuditQuery),
    responses((status = 200, body = [AuditEntry]), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn audit_log(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<Vec<AuditEntry>>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    let limit = query.limit.unwrap_or(100).clamp(1, 200);
    Ok(Json(
        repo::audit::list(&state.db, wid, query.before, limit).await?,
    ))
}
