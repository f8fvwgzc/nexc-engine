//! The platform console's API: every workspace and account of the
//! installation, what a platform administrator may do to them, and the log
//! of what was done.
//!
//! This is the other side of the platform boundary. Every handler takes
//! [`PlatformAdmin`]; the workspace API takes `AuthUser`, which refuses the
//! same accounts. Nothing here returns a workspace's content.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::workspaces::{audit, ensure_personal};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::audit::AuditAction;
use crate::domain::platform::{
    PlatformAction, PlatformEvent, PlatformUser, PlatformWorkspace, PlatformWorkspaceDetail,
    REASON_MAX,
};
use crate::domain::user::{self, Role};
use crate::domain::validation::{FieldErrors, Validate};
use crate::domain::workspace::WorkspaceRole;
use crate::http::extract::{Path, PlatformAdmin, Query, ValidatedJson};
use crate::http::problem::Problem;
use crate::repo::audit::Subject;
use crate::repo::platform::Page;
use crate::repo::{self, OrNotFound};

/// Query of the platform lists.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct PlatformQuery {
    /// Part of a name or e-mail address.
    pub q: Option<String>,
    /// 1-100, default 25.
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl PlatformQuery {
    fn page(&self) -> Page {
        Page {
            q: self
                .q
                .as_deref()
                .map(str::trim)
                .filter(|q| !q.is_empty())
                .map(|q| q.chars().take(100).collect()),
            limit: self.limit.unwrap_or(25).clamp(1, 100),
            offset: self.offset.unwrap_or(0).clamp(0, 1_000_000),
        }
    }
}

/// Writes an entry of the platform's activity log. A failure is logged, not
/// returned: the action it describes has already happened.
async fn log(
    state: &AppState,
    admin: PlatformAdmin,
    action: PlatformAction,
    subject: &str,
    detail: &str,
) {
    if let Err(err) = repo::platform::record(&state.db, admin.id, action, subject, detail).await {
        tracing::error!(actor = %admin.id, %action, error = %err, "platform event was not written");
    }
}

fn named(user: &PlatformUser) -> String {
    format!("{} <{}>", user.name, user.email)
}

async fn detail(state: &AppState, wid: Uuid) -> Result<PlatformWorkspaceDetail, AppError> {
    let workspace = repo::platform::workspace(&state.db, wid)
        .await
        .or_not_found("workspace")?;
    Ok(PlatformWorkspaceDetail {
        workspace,
        members: repo::platform::members(&state.db, wid).await?,
        footprint: repo::platform::footprint(&state.db, wid).await?,
    })
}

/// Every workspace of the installation, newest first.
#[utoipa::path(get, path = "/admin/workspaces", tag = "admin", security(("bearer" = [])), params(PlatformQuery),
    responses((status = 200, body = [PlatformWorkspace]), (status = 403, body = Problem)))]
pub async fn workspaces(
    State(state): State<AppState>,
    _admin: PlatformAdmin,
    Query(query): Query<PlatformQuery>,
) -> Result<Json<Vec<PlatformWorkspace>>, AppError> {
    Ok(Json(
        repo::platform::workspaces(&state.db, &query.page()).await?,
    ))
}

/// One workspace: its members and how much it holds, never what it holds.
#[utoipa::path(get, path = "/admin/workspaces/{wid}", tag = "admin", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, body = PlatformWorkspaceDetail), (status = 403, body = Problem),
        (status = 404, body = Problem)))]
pub async fn workspace(
    State(state): State<AppState>,
    _admin: PlatformAdmin,
    Path(wid): Path<Uuid>,
) -> Result<Json<PlatformWorkspaceDetail>, AppError> {
    Ok(Json(detail(&state, wid).await?))
}

/// `POST /admin/workspaces/{wid}/owners` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AssignOwner {
    /// E-mail address of a registered account.
    pub email: String,
}

impl Validate for AssignOwner {
    fn validate(&self, errors: &mut FieldErrors) {
        user::check_email(errors, &user::normalize_email(&self.email));
    }
}

/// Makes a registered account an owner of a workspace: for a workspace
/// whose owner left, was suspended or now administers the platform. The
/// workspace's own audit log records it, so its members see who did it.
#[utoipa::path(post, path = "/admin/workspaces/{wid}/owners", tag = "admin", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")), request_body = AssignOwner,
    responses((status = 200, body = PlatformWorkspaceDetail), (status = 403, body = Problem),
        (status = 404, body = Problem),
        (status = 409, description = "Already an owner, suspended, or a platform administrator", body = Problem),
        (status = 422, description = "No account has this address", body = Problem)))]
pub async fn assign_owner(
    State(state): State<AppState>,
    admin: PlatformAdmin,
    Path(wid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<AssignOwner>,
) -> Result<Json<PlatformWorkspaceDetail>, AppError> {
    let workspace = repo::platform::workspace(&state.db, wid)
        .await
        .or_not_found("workspace")?;
    let email = user::normalize_email(&req.email);
    let Some(account) = repo::platform::user_by_email(&state.db, &email).await? else {
        return Err(AppError::field(
            "email",
            "no account has this e-mail address",
        ));
    };
    if account.role == Role::Admin {
        return Err(AppError::Conflict(
            "a platform administrator does not work inside workspaces; choose another account"
                .into(),
        ));
    }
    if account.suspended {
        return Err(AppError::Conflict(
            "this account is suspended; reactivate it first".into(),
        ));
    }
    let was = repo::workspaces::role_of(&state.db, wid, account.id).await?;
    let change = match was {
        Some(WorkspaceRole::Owner) => {
            return Err(AppError::Conflict(
                "this account already owns the workspace".into(),
            ));
        }
        Some(role) => {
            repo::workspaces::set_role(&state.db, wid, account.id, WorkspaceRole::Owner).await?;
            format!("{} is an owner now (was {role})", named(&account))
        }
        None => {
            repo::workspaces::add_member(&state.db, wid, account.id, WorkspaceRole::Owner).await?;
            format!("{} joined as an owner", named(&account))
        }
    };
    log(
        &state,
        admin,
        PlatformAction::OwnerAssigned,
        &workspace.name,
        &change,
    )
    .await;
    audit(
        &state,
        wid,
        admin.id,
        AuditAction::PlatformOwnerAssigned,
        Subject::User(account.id),
        "from the platform console",
    )
    .await;
    Ok(Json(detail(&state, wid).await?))
}

/// `POST /admin/workspaces/{wid}/delete` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteWorkspace {
    /// The workspace's name, exactly: deleting cannot be undone.
    pub confirm: String,
}

impl Validate for DeleteWorkspace {
    fn validate(&self, _errors: &mut FieldErrors) {}
}

/// Deletes a workspace with everything in it, files included. Members it
/// leaves without any workspace get an empty personal one, so that they can
/// still sign in and work. The name travels in the body, not in the
/// address, so that it does not end up in access logs.
#[utoipa::path(post, path = "/admin/workspaces/{wid}/delete", tag = "admin", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")), request_body = DeleteWorkspace,
    responses((status = 204, description = "Deleted"), (status = 403, body = Problem),
        (status = 404, body = Problem),
        (status = 422, description = "The name does not match", body = Problem)))]
pub async fn delete_workspace(
    State(state): State<AppState>,
    admin: PlatformAdmin,
    Path(wid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<DeleteWorkspace>,
) -> Result<StatusCode, AppError> {
    let workspace = repo::platform::workspace(&state.db, wid)
        .await
        .or_not_found("workspace")?;
    if req.confirm != workspace.name {
        return Err(AppError::field(
            "confirm",
            "type the workspace's name exactly to delete it",
        ));
    }
    crate::engine::workspaces::erase(&state, wid).await?;
    let owner = workspace.owner_email.as_deref().unwrap_or("nobody");
    let count = |n: i64, one: &str| format!("{n} {one}{}", if n == 1 { "" } else { "s" });
    let summary = format!(
        "owned by {owner}; {}, {}, {}",
        count(workspace.member_count, "member"),
        count(workspace.issue_count, "issue"),
        count(workspace.graph_count, "graph"),
    );
    log(
        &state,
        admin,
        PlatformAction::WorkspaceDeleted,
        &workspace.name,
        &summary,
    )
    .await;
    ensure_personal(&state).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Every account of the installation, newest first.
#[utoipa::path(get, path = "/admin/users", tag = "admin", security(("bearer" = [])), params(PlatformQuery),
    responses((status = 200, body = [PlatformUser]), (status = 403, body = Problem)))]
pub async fn users(
    State(state): State<AppState>,
    _admin: PlatformAdmin,
    Query(query): Query<PlatformQuery>,
) -> Result<Json<Vec<PlatformUser>>, AppError> {
    Ok(Json(repo::platform::users(&state.db, &query.page()).await?))
}

/// `PATCH /admin/users/{uid}` body: at least one of `role` and `suspended`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdatePlatformUser {
    /// `admin` lets the account administer the platform; `user` takes that away.
    pub role: Option<Role>,
    /// `true` stops the account from signing in and ends its sessions; `false` lifts that.
    pub suspended: Option<bool>,
    /// Why the account is suspended (with `suspended: true`), for the activity log.
    pub reason: Option<String>,
}

impl Validate for UpdatePlatformUser {
    fn validate(&self, errors: &mut FieldErrors) {
        if self.role.is_none() && self.suspended.is_none() {
            errors.add("role", "give a role, a suspension, or both");
        }
        match &self.reason {
            Some(_) if self.suspended != Some(true) => {
                errors.add("reason", "a reason goes with a suspension");
            }
            Some(reason) if reason.chars().count() > REASON_MAX => {
                errors.add("reason", format!("at most {REASON_MAX} characters"));
            }
            _ => {}
        }
    }
}

/// Changes an account's platform role, suspends it, or lets it back in.
///
/// Both a role change and a suspension end the account's access tokens at
/// once. Nobody changes their own account here, so the platform always
/// keeps the administrator who is acting. An account that is the only
/// owner of a workspace other people work in cannot be made a platform
/// administrator before that workspace has another owner.
#[utoipa::path(patch, path = "/admin/users/{uid}", tag = "admin", security(("bearer" = [])),
    params(("uid" = Uuid, Path, description = "User id")), request_body = UpdatePlatformUser,
    responses((status = 200, body = PlatformUser), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "Your own account, or the only owner of a shared workspace", body = Problem)))]
pub async fn update_user(
    State(state): State<AppState>,
    admin: PlatformAdmin,
    Path(uid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<UpdatePlatformUser>,
) -> Result<Json<PlatformUser>, AppError> {
    let account = repo::platform::user(&state.db, uid)
        .await
        .or_not_found("user")?;
    if uid == admin.id {
        return Err(AppError::Conflict(
            "you cannot change your own account here; ask another administrator".into(),
        ));
    }
    let subject = named(&account);
    if let Some(role) = req.role.filter(|role| *role != account.role) {
        if role == Role::Admin {
            let orphaned = repo::platform::sole_owner_of(&state.db, uid).await?;
            if !orphaned.is_empty() {
                return Err(AppError::Conflict(format!(
                    "{} is the only owner of {}; assign another owner there first",
                    account.name,
                    orphaned.join(", ")
                )));
            }
        }
        if let Some(epoch) = repo::platform::set_role(&state.db, uid, role).await? {
            state.sessions.raise(uid, epoch);
            let change = format!("{} → {role}", account.role);
            log(
                &state,
                admin,
                PlatformAction::RoleChanged,
                &subject,
                &change,
            )
            .await;
        }
    }
    match req.suspended {
        Some(true) => {
            let reason = req.reason.as_deref().unwrap_or("").trim();
            if let Some(epoch) = repo::platform::suspend(&state.db, uid, reason).await? {
                repo::tokens::revoke_user(&state.db, uid).await?;
                state.sessions.raise(uid, epoch);
                log(
                    &state,
                    admin,
                    PlatformAction::AccountSuspended,
                    &subject,
                    reason,
                )
                .await;
            }
        }
        Some(false) if repo::platform::reactivate(&state.db, uid).await? => {
            log(
                &state,
                admin,
                PlatformAction::AccountReactivated,
                &subject,
                "",
            )
            .await;
        }
        _ => {}
    }
    Ok(Json(
        repo::platform::user(&state.db, uid)
            .await
            .or_not_found("user")?,
    ))
}

/// A password reset link's token, shown once.
#[derive(Debug, Serialize, ToSchema)]
pub struct IssuedReset {
    /// What the account's holder sets a new password with
    /// (`POST /auth/password/reset`). It is not stored and cannot be shown again.
    pub token: String,
    /// Seconds until it stops working.
    pub expires_in: u64,
}

/// Issues a one-time password reset link for an account whose holder cannot
/// sign in. It works once and for an hour, and replaces any earlier link of
/// the account that still worked. The administrator hands it over; the
/// installation sends no e-mail.
#[utoipa::path(post, path = "/admin/users/{uid}/password-reset", tag = "admin", security(("bearer" = [])),
    params(("uid" = Uuid, Path, description = "User id")),
    responses((status = 200, body = IssuedReset), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn issue_password_reset(
    State(state): State<AppState>,
    admin: PlatformAdmin,
    Path(uid): Path<Uuid>,
) -> Result<Json<IssuedReset>, AppError> {
    let account = repo::platform::user(&state.db, uid)
        .await
        .or_not_found("user")?;
    let token = crate::security::random::random_token();
    let expires_at = chrono::Utc::now()
        + chrono::Duration::seconds(i64::try_from(user::RESET_TTL_SECS).unwrap_or(3600));
    let mut tx = state.db.begin().await?;
    repo::resets::issue(
        &mut tx,
        uid,
        admin.id,
        &crate::security::random::token_digest(&token),
        expires_at,
    )
    .await?;
    tx.commit().await?;
    log(
        &state,
        admin,
        PlatformAction::PasswordResetIssued,
        &named(&account),
        "",
    )
    .await;
    Ok(Json(IssuedReset {
        token,
        expires_in: user::RESET_TTL_SECS,
    }))
}

/// `POST /admin/users/{uid}/erase` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EraseAccount {
    /// The account's e-mail address, exactly: erasing cannot be undone.
    pub confirm: String,
}

impl Validate for EraseAccount {
    fn validate(&self, _errors: &mut FieldErrors) {}
}

/// Deletes an account on its holder's request, exactly as the holder could
/// from their profile: workspaces it is alone in are removed, it leaves the
/// others, and what identified the person is gone. The activity log keeps
/// the account's id, not its name or address.
#[utoipa::path(post, path = "/admin/users/{uid}/erase", tag = "admin", security(("bearer" = [])),
    params(("uid" = Uuid, Path, description = "User id")), request_body = EraseAccount,
    responses((status = 204, description = "Erased"), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "Your own account, a platform administrator, or the only owner of a shared workspace", body = Problem),
        (status = 422, description = "The address does not match", body = Problem)))]
pub async fn erase_user(
    State(state): State<AppState>,
    admin: PlatformAdmin,
    Path(uid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<EraseAccount>,
) -> Result<StatusCode, AppError> {
    let account = repo::platform::user(&state.db, uid)
        .await
        .or_not_found("user")?;
    if uid == admin.id {
        return Err(AppError::Conflict(
            "you cannot erase your own account here".into(),
        ));
    }
    if account.role == Role::Admin {
        return Err(AppError::Conflict(
            "take the platform role away from this account before erasing it".into(),
        ));
    }
    if user::normalize_email(&req.confirm) != user::normalize_email(&account.email) {
        return Err(AppError::field(
            "confirm",
            "type the account's e-mail address exactly to erase it",
        ));
    }
    crate::engine::account::erase(&state, uid).await?;
    log(
        &state,
        admin,
        PlatformAction::AccountErased,
        &format!("account {uid}"),
        "erased on request",
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// What platform administrators did, newest first.
#[utoipa::path(get, path = "/admin/events", tag = "admin", security(("bearer" = [])), params(PlatformQuery),
    responses((status = 200, body = [PlatformEvent]), (status = 403, body = Problem)))]
pub async fn events(
    State(state): State<AppState>,
    _admin: PlatformAdmin,
    Query(query): Query<PlatformQuery>,
) -> Result<Json<Vec<PlatformEvent>>, AppError> {
    Ok(Json(
        repo::platform::events(&state.db, &query.page()).await?,
    ))
}
