//! A person's own account: their name and password, their sessions, a copy
//! of what is held about them, and its deletion. Platform administrators
//! use the same endpoints, except that their account is not deleted while
//! it administers the platform.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::auth::{cookie, session_config, with_session};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::user::{self, AuthResponse, Role, User};
use crate::domain::validation::{FieldErrors, Validate};
use crate::engine::account::{self, AccountExport};
use crate::http::extract::{Caller, ClientIp, ValidatedJson};
use crate::http::middleware::rate_limit::AuthRateLimit;
use crate::http::problem::Problem;
use crate::repo::{self, OrNotFound};
use crate::security::{password, random, session};

/// `PATCH /auth/me` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateProfile {
    pub name: String,
}

impl Validate for UpdateProfile {
    fn validate(&self, errors: &mut FieldErrors) {
        user::check_name(errors, &self.name);
    }
}

/// Changes the caller's display name.
#[utoipa::path(patch, path = "/auth/me", tag = "auth", security(("bearer" = [])), request_body = UpdateProfile,
    responses((status = 200, body = User), (status = 401, body = Problem), (status = 422, body = Problem)))]
pub async fn update_me(
    State(state): State<AppState>,
    auth: Caller,
    ValidatedJson(req): ValidatedJson<UpdateProfile>,
) -> Result<Json<User>, AppError> {
    Ok(Json(
        repo::users::set_name(&state.db, auth.id, req.name.trim())
            .await
            .or_not_found("user")?,
    ))
}

/// Checks `given` against the caller's password, off the async workers.
/// A wrong one counts as a failed sign-in from this address.
async fn confirm_password(
    state: &AppState,
    auth: Caller,
    ip: std::net::IpAddr,
    given: &str,
    field: &'static str,
) -> Result<(), AppError> {
    state.limiters.check_login_allowed(ip)?;
    let hash = repo::users::password_hash(&state.db, auth.id)
        .await
        .or_not_found("user")?;
    let given = given.to_owned();
    let valid = tokio::task::spawn_blocking(move || password::verify_password(&given, &hash))
        .await
        .map_err(anyhow::Error::from)?;
    if valid {
        Ok(())
    } else {
        state.limiters.record_login_failure(ip);
        Err(AppError::field(field, "this is not your password"))
    }
}

/// `POST /auth/password` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChangePassword {
    pub current_password: String,
    /// 12–128 characters.
    pub new_password: String,
}

impl Validate for ChangePassword {
    fn validate(&self, errors: &mut FieldErrors) {
        if self.current_password.is_empty()
            || self.current_password.chars().count() > user::PASSWORD_MAX
        {
            errors.add("current_password", "invalid password");
        }
        let mut new = FieldErrors::default();
        user::check_password(&mut new, &self.new_password);
        for message in new.as_map().get("password").into_iter().flatten() {
            errors.add("new_password", message.clone());
        }
        if self.new_password == self.current_password {
            errors.add("new_password", "must differ from the current password");
        }
    }
}

/// Changes the caller's password. Every session ends, on every device, and
/// this one starts anew: the response carries a fresh access token and
/// refresh cookie.
#[utoipa::path(post, path = "/auth/password", tag = "auth", security(("bearer" = [])), request_body = ChangePassword,
    responses(
        (status = 200, description = "Changed; new session", body = AuthResponse),
        (status = 401, body = Problem),
        (status = 422, description = "Wrong current password, or a new one that does not meet the rules", body = Problem),
        (status = 429, description = "Too many wrong passwords", body = Problem),
    ))]
pub async fn change_password(
    _limit: AuthRateLimit,
    State(state): State<AppState>,
    auth: Caller,
    ClientIp(ip): ClientIp,
    ValidatedJson(req): ValidatedJson<ChangePassword>,
) -> Result<Response, AppError> {
    confirm_password(&state, auth, ip, &req.current_password, "current_password").await?;
    let new = req.new_password.clone();
    let hash = tokio::task::spawn_blocking(move || password::hash_password(&new))
        .await
        .map_err(anyhow::Error::from)??;
    repo::users::set_password(&state.db, auth.id, &hash).await?;
    end_sessions(&state, auth).await?;
    let account = repo::users::find(&state.db, auth.id)
        .await
        .or_not_found("user")?;
    let session = session::start(&state.db, session_config(&state), account).await?;
    Ok(with_session(&state, StatusCode::OK, session))
}

/// Ends every session of an account: access tokens stop at once, refresh
/// tokens are revoked.
async fn end_sessions_of(state: &AppState, user: Uuid) -> Result<(), AppError> {
    let epoch = repo::users::end_sessions(&state.db, user).await?;
    repo::tokens::revoke_user(&state.db, user).await?;
    state.sessions.raise(user, epoch);
    Ok(())
}

async fn end_sessions(state: &AppState, auth: Caller) -> Result<(), AppError> {
    end_sessions_of(state, auth.id).await
}

/// `POST /auth/password/reset` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ResetPassword {
    /// The token of a reset link, as a platform administrator issued it.
    pub token: String,
    /// 12–128 characters.
    pub new_password: String,
}

impl Validate for ResetPassword {
    fn validate(&self, errors: &mut FieldErrors) {
        if self.token.is_empty() || self.token.len() > 128 {
            errors.add("token", "invalid link");
        }
        let mut new = FieldErrors::default();
        user::check_password(&mut new, &self.new_password);
        for message in new.as_map().get("password").into_iter().flatten() {
            errors.add("new_password", message.clone());
        }
    }
}

/// Sets a new password with a reset link (no sign-in needed). The link
/// works once and for an hour. Every session of the account ends; the
/// person then signs in with the new password. A password that does not
/// meet the rules does not use the link up.
#[utoipa::path(post, path = "/auth/password/reset", tag = "auth", request_body = ResetPassword,
    responses(
        (status = 204, description = "Password set"),
        (status = 401, description = "The link is unknown, used or expired", body = Problem),
        (status = 422, description = "The new password does not meet the rules", body = Problem),
        (status = 429, description = "Rate limited", body = Problem),
    ))]
pub async fn reset_password(
    _limit: AuthRateLimit,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ValidatedJson(req): ValidatedJson<ResetPassword>,
) -> Result<StatusCode, AppError> {
    state.limiters.check_login_allowed(ip)?;
    let digest = random::token_digest(&req.token);
    let Some(account) = repo::resets::redeem(&state.db, &digest).await? else {
        state.limiters.record_login_failure(ip);
        return Err(AppError::Unauthorized(
            "this link does not work any more; ask for a new one",
        ));
    };
    let new = req.new_password.clone();
    let hash = tokio::task::spawn_blocking(move || password::hash_password(&new))
        .await
        .map_err(anyhow::Error::from)??;
    repo::users::set_password(&state.db, account, &hash).await?;
    end_sessions_of(&state, account).await?;
    tracing::info!(user_id = %account, "password set with a reset link");
    Ok(StatusCode::NO_CONTENT)
}

/// The caller's sessions.
#[derive(Debug, Serialize, ToSchema)]
pub struct Sessions {
    /// Sign-ins that are still alive, this one included.
    pub active: i64,
}

/// How many sessions the caller has.
#[utoipa::path(get, path = "/auth/sessions", tag = "auth", security(("bearer" = [])),
    responses((status = 200, body = Sessions), (status = 401, body = Problem)))]
pub async fn sessions(
    State(state): State<AppState>,
    auth: Caller,
) -> Result<Json<Sessions>, AppError> {
    Ok(Json(Sessions {
        active: repo::tokens::active_sessions(&state.db, auth.id).await?,
    }))
}

fn signed_out(state: &AppState) -> Response {
    (
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, cookie(state, "", 0))],
    )
        .into_response()
}

/// Signs the caller out everywhere, this device included.
#[utoipa::path(post, path = "/auth/sessions/end", tag = "auth", security(("bearer" = [])),
    responses((status = 204, description = "Every session ended"), (status = 401, body = Problem)))]
pub async fn end_all_sessions(
    State(state): State<AppState>,
    auth: Caller,
) -> Result<Response, AppError> {
    end_sessions(&state, auth).await?;
    Ok(signed_out(&state))
}

/// A copy of what the installation holds about the caller, as a JSON file:
/// the account, its workspaces and teams, the issues and comments it wrote,
/// its graphs, documents and personal memories, and what it spent. No
/// credential is in it.
#[utoipa::path(get, path = "/auth/me/export", tag = "auth", security(("bearer" = [])),
    responses((status = 200, body = AccountExport), (status = 401, body = Problem)))]
pub async fn export(State(state): State<AppState>, auth: Caller) -> Result<Response, AppError> {
    let export = account::export(&state, auth.id).await?;
    let disposition = HeaderValue::from_static("attachment; filename=\"nexc-account.json\"");
    Ok(([(header::CONTENT_DISPOSITION, disposition)], Json(export)).into_response())
}

/// `POST /auth/me/delete` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteAccount {
    /// The caller's password: deleting cannot be undone.
    pub password: String,
}

impl Validate for DeleteAccount {
    fn validate(&self, errors: &mut FieldErrors) {
        if self.password.is_empty() || self.password.chars().count() > user::PASSWORD_MAX {
            errors.add("password", "invalid password");
        }
    }
}

/// Deletes the caller's account. Workspaces it is alone in are removed; it
/// leaves the others, where what it made stays under "Deleted account".
/// Its name, address, password, sessions, AI account, notifications and
/// personal memories are gone, and the address can register again.
#[utoipa::path(post, path = "/auth/me/delete", tag = "auth", security(("bearer" = [])), request_body = DeleteAccount,
    responses(
        (status = 204, description = "Deleted"),
        (status = 401, body = Problem),
        (status = 409, description = "The only owner of a shared workspace, or a platform administrator", body = Problem),
        (status = 422, description = "Wrong password", body = Problem),
        (status = 429, description = "Too many wrong passwords", body = Problem),
    ))]
pub async fn delete_me(
    _limit: AuthRateLimit,
    State(state): State<AppState>,
    auth: Caller,
    ClientIp(ip): ClientIp,
    ValidatedJson(req): ValidatedJson<DeleteAccount>,
) -> Result<Response, AppError> {
    confirm_password(&state, auth, ip, &req.password, "password").await?;
    if auth.role == Role::Admin {
        return Err(AppError::Conflict(
            "this account administers the platform; another administrator has to take that \
             role away before it can be deleted"
                .into(),
        ));
    }
    account::erase(&state, auth.id).await?;
    Ok(signed_out(&state))
}
