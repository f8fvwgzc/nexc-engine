//! Registration, login, refresh-token rotation and logout.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::account::AccountEventKind;
use crate::domain::user::{self, AuthResponse, Role, User};
use crate::domain::validation::{FieldErrors, Validate};
use crate::engine::{account, two_factor};
use crate::http::extract::{Caller, ClientIp, ValidatedJson};
use crate::http::middleware::rate_limit::AuthRateLimit;
use crate::http::problem::Problem;
use crate::repo::{self, OrNotFound};
use crate::security::password;
use crate::security::session::{self, Session, SessionConfig};

/// Name of the refresh-token cookie.
pub const REFRESH_COOKIE: &str = "nexc_refresh";
const COOKIE_PATH: &str = "/api/v1/auth";
const CSRF_HEADER: &str = "x-requested-with";
const CSRF_VALUE: &str = "nexc";

/// `POST /auth/register` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RegisterRequest {
    pub email: String,
    /// 12–128 characters.
    pub password: String,
    pub name: String,
}

impl Validate for RegisterRequest {
    fn validate(&self, errors: &mut FieldErrors) {
        user::check_email(errors, &user::normalize_email(&self.email));
        user::check_password(errors, &self.password);
        user::check_name(errors, &self.name);
    }
}

/// `POST /auth/login` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    /// For an account with two-factor sign-in: the authenticator app's
    /// current code, or a recovery code.
    pub code: Option<String>,
}

impl Validate for LoginRequest {
    fn validate(&self, errors: &mut FieldErrors) {
        if self.email.trim().is_empty() || self.email.len() > user::EMAIL_MAX {
            errors.add("email", "invalid email");
        }
        if self.password.is_empty() || self.password.chars().count() > user::PASSWORD_MAX {
            errors.add("password", "invalid password");
        }
    }
}

pub(super) fn session_config(state: &AppState) -> SessionConfig<'_> {
    SessionConfig {
        jwt: &state.jwt,
        refresh_ttl: state.settings.refresh_ttl,
    }
}

pub(super) fn cookie(state: &AppState, value: &str, max_age: u64) -> HeaderValue {
    let secure = if state.settings.cookie_secure {
        "; Secure"
    } else {
        ""
    };
    let raw = format!(
        "{REFRESH_COOKIE}={value}; HttpOnly; SameSite=Strict; Path={COOKIE_PATH}; Max-Age={max_age}{secure}"
    );
    HeaderValue::from_str(&raw).expect("cookie is ASCII")
}

pub(super) fn with_session(state: &AppState, status: StatusCode, s: Session) -> Response {
    let set_cookie = cookie(
        state,
        &s.refresh_token,
        state.settings.refresh_ttl.as_secs(),
    );
    (status, [(header::SET_COOKIE, set_cookie)], Json(s.response)).into_response()
}

/// Reads the refresh token from the `Cookie` header.
fn refresh_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|pair| {
            pair.trim()
                .strip_prefix(REFRESH_COOKIE)?
                .strip_prefix('=')
                .map(str::to_owned)
        })
        .filter(|t| !t.is_empty() && t.len() <= 128)
}

fn require_csrf_header(headers: &HeaderMap) -> Result<(), AppError> {
    match headers.get(CSRF_HEADER).and_then(|v| v.to_str().ok()) {
        Some(CSRF_VALUE) => Ok(()),
        _ => Err(AppError::Forbidden(format!(
            "missing header {CSRF_HEADER}: {CSRF_VALUE}"
        ))),
    }
}

/// Creates an account (when signups are enabled) and starts a session.
#[utoipa::path(post, path = "/auth/register", tag = "auth", request_body = RegisterRequest,
    responses(
        (status = 201, description = "Registered; refresh cookie set", body = AuthResponse),
        (status = 403, description = "Signups disabled", body = Problem),
        (status = 409, description = "E-mail already registered", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
        (status = 429, description = "Rate limited", body = Problem),
    ))]
pub async fn register(
    _limit: AuthRateLimit,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ValidatedJson(req): ValidatedJson<RegisterRequest>,
) -> Result<Response, AppError> {
    if !state.settings.allow_signup {
        return Err(AppError::Forbidden("signups are disabled".into()));
    }
    let email = user::normalize_email(&req.email);
    if repo::users::email_exists(&state.db, &email).await? {
        return Err(AppError::Conflict(
            "this e-mail is already registered".into(),
        ));
    }
    let password = req.password.clone();
    let hash = tokio::task::spawn_blocking(move || password::hash_password(&password))
        .await
        .map_err(anyhow::Error::from)??;
    let user = create_user(&state, &email, req.name.trim(), Role::User, &hash).await?;
    account::note(&state, user.id, AccountEventKind::Registered, Some(ip), "").await;
    let session = session::start(&state.db, session_config(&state), user).await?;
    Ok(with_session(&state, StatusCode::CREATED, session))
}

/// Name of the workspace a new account starts in.
pub fn personal_workspace_name(user_name: &str) -> String {
    let first = user_name.split_whitespace().next().unwrap_or("My");
    let name: String = first.chars().take(60).collect();
    format!("{name}'s workspace")
}

/// Inserts a user and its first workspace membership in one transaction.
/// A platform administrator gets neither: such an account never works
/// inside a workspace.
pub async fn create_user(
    state: &AppState,
    email: &str,
    name: &str,
    role: Role,
    hash: &str,
) -> Result<User, AppError> {
    let mut tx = state.db.begin().await?;
    let user = repo::users::create(&mut *tx, email, name, role, hash).await?;
    // Join the workspaces this address was invited to; without any, start a personal one so
    // that every account works in at least one workspace.
    if role != Role::Admin && repo::workspaces::accept_invites(&mut tx, user.id, email).await? == 0
    {
        super::workspaces::create_owned(
            &mut tx,
            user.id,
            &personal_workspace_name(name),
            &state.settings.llm_model,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(user)
}

/// Verifies credentials and starts a session.
#[utoipa::path(post, path = "/auth/login", tag = "auth", request_body = LoginRequest,
    responses(
        (status = 200, description = "Logged in; refresh cookie set", body = AuthResponse),
        (status = 401, description = "Invalid credentials", body = Problem),
        (status = 422, description = "The account has two-factor sign-in: `errors.code` asks for the code, or says it is wrong", body = Problem),
        (status = 429, description = "Too many failed attempts", body = Problem),
    ))]
pub async fn login(
    _limit: AuthRateLimit,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ValidatedJson(req): ValidatedJson<LoginRequest>,
) -> Result<Response, AppError> {
    state.limiters.check_login_allowed(ip)?;
    let email = user::normalize_email(&req.email);
    // The account's holder sees where they signed in from, and when someone failed to.
    let failed = |detail: &'static str| {
        let (state, email) = (&state, &email);
        async move {
            if let Ok(Some(user)) = repo::workspaces::user_id_by_email(&state.db, email).await {
                account::note(
                    state,
                    user,
                    AccountEventKind::SignInFailed,
                    Some(ip),
                    detail,
                )
                .await;
            }
        }
    };
    let user = match session::authenticate(&state.db, &email, &req.password).await {
        Ok(user) => user,
        Err(err @ AppError::Unauthorized(_)) => {
            state.limiters.record_login_failure(ip);
            failed("").await;
            return Err(err);
        }
        Err(err @ AppError::Forbidden(_)) => {
            failed("the account is suspended").await;
            return Err(err);
        }
        Err(err) => return Err(err),
    };
    // The password was right; an account with a second factor is not in yet.
    if let Err(err) = two_factor::check(&state, user.id, req.code.as_deref()).await {
        if req.code.as_deref().is_some_and(|c| !c.trim().is_empty()) {
            state.limiters.record_login_failure(ip);
            failed("wrong two-factor code").await;
        }
        return Err(err);
    }
    let id = user.id;
    let session = session::start(&state.db, session_config(&state), user).await?;
    account::note(&state, id, AccountEventKind::SignedIn, Some(ip), "").await;
    Ok(with_session(&state, StatusCode::OK, session))
}

/// Rotates the refresh token (cookie + `X-Requested-With: nexc`).
#[utoipa::path(post, path = "/auth/refresh", tag = "auth",
    params(("X-Requested-With" = String, Header, description = "Must be `nexc` (CSRF guard)")),
    responses(
        (status = 200, description = "New access token; rotated cookie", body = AuthResponse),
        (status = 401, description = "Missing, expired, revoked or reused refresh token", body = Problem),
        (status = 403, description = "Missing CSRF header", body = Problem),
    ))]
pub async fn refresh(
    _limit: AuthRateLimit,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    require_csrf_header(&headers)?;
    let token = refresh_token(&headers).ok_or(AppError::Unauthorized("missing refresh token"))?;
    let session = session::rotate(&state.db, session_config(&state), &token).await?;
    Ok(with_session(&state, StatusCode::OK, session))
}

/// Ends the session: revokes the refresh-token family and clears the cookie.
#[utoipa::path(post, path = "/auth/logout", tag = "auth",
    params(("X-Requested-With" = String, Header, description = "Must be `nexc` (CSRF guard)")),
    responses(
        (status = 204, description = "Logged out"),
        (status = 403, description = "Missing CSRF header", body = Problem),
    ))]
pub async fn logout(
    _limit: AuthRateLimit,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    require_csrf_header(&headers)?;
    if let Some(token) = refresh_token(&headers) {
        session::revoke(&state.db, &token).await?;
    }
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, cookie(&state, "", 0))],
    )
        .into_response())
}

/// The current user, whichever side of the platform boundary they work on.
#[utoipa::path(get, path = "/auth/me", tag = "auth", security(("bearer" = [])),
    responses((status = 200, body = User), (status = 401, body = Problem)))]
pub async fn me(State(state): State<AppState>, auth: Caller) -> Result<Json<User>, AppError> {
    Ok(Json(
        repo::users::find(&state.db, auth.id)
            .await
            .or_not_found("user")?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_refresh_cookie() {
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            HeaderValue::from_static("a=1; nexc_refresh=tok-123; b=2"),
        );
        assert_eq!(refresh_token(&h).as_deref(), Some("tok-123"));
        h.insert(
            header::COOKIE,
            HeaderValue::from_static("nexc_refresh_other=x"),
        );
        assert_eq!(refresh_token(&h), None);
    }

    #[test]
    fn csrf_header_required() {
        let mut h = HeaderMap::new();
        assert!(require_csrf_header(&h).is_err());
        h.insert(CSRF_HEADER, HeaderValue::from_static("nexc"));
        assert!(require_csrf_header(&h).is_ok());
    }
}
