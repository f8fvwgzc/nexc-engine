//! Login sessions: access token + rotating refresh token.
//!
//! Every refresh consumes the presented token and issues a new one in the
//! same family. Presenting an already used token is treated as theft: the
//! whole family is revoked, logging out every holder.

use std::time::Duration;

use chrono::Utc;
use sqlx::PgPool;

use super::jwt::JwtKeys;
use super::password;
use super::random::{random_token, token_digest};
use crate::domain::AppError;
use crate::domain::user::{AuthResponse, User, lockout_duration};
use crate::repo::{tokens, users};

const INVALID_CREDENTIALS: &str = "invalid email or password";
const INVALID_SESSION: &str = "session expired, please log in again";
const SUSPENDED: &str = "this account is suspended; contact whoever administers this installation";

/// A freshly issued session; `refresh_token` goes into the HttpOnly cookie.
#[derive(Debug)]
pub struct Session {
    pub response: AuthResponse,
    pub refresh_token: String,
}

/// Token configuration shared by the session functions.
#[derive(Debug, Clone, Copy)]
pub struct SessionConfig<'a> {
    pub jwt: &'a JwtKeys,
    pub refresh_ttl: Duration,
}

async fn issue(
    db: &PgPool,
    cfg: SessionConfig<'_>,
    user: User,
    family_id: uuid::Uuid,
) -> Result<Session, AppError> {
    let epoch = users::session_epoch(db, user.id).await?;
    let access_token = cfg.jwt.issue(user.id, user.role, epoch)?;
    let refresh_token = random_token();
    let expires_at =
        Utc::now() + chrono::Duration::from_std(cfg.refresh_ttl).map_err(anyhow::Error::from)?;
    tokens::insert(
        db,
        user.id,
        family_id,
        &token_digest(&refresh_token),
        expires_at,
    )
    .await?;
    Ok(Session {
        response: AuthResponse {
            user,
            access_token,
            expires_in: cfg.jwt.ttl_secs(),
        },
        refresh_token,
    })
}

/// Starts a new session (new refresh-token family) for `user`.
pub async fn start(db: &PgPool, cfg: SessionConfig<'_>, user: User) -> Result<Session, AppError> {
    issue(db, cfg, user, uuid::Uuid::now_v7()).await
}

/// Verifies credentials with timing-safe failure paths and account lockout.
/// Every failure returns the same generic 401. A suspended account is told
/// so, but only once its password was right.
pub async fn login(
    db: &PgPool,
    cfg: SessionConfig<'_>,
    email: &str,
    pw: &str,
) -> Result<Session, AppError> {
    let user = authenticate(db, email, pw).await?;
    start(db, cfg, user).await
}

/// The first step of signing in: who `email` and `pw` are, without starting
/// a session yet. The caller checks a second factor in between.
pub async fn authenticate(db: &PgPool, email: &str, pw: &str) -> Result<User, AppError> {
    let creds = users::credentials(db, email).await?;
    let (pw_owned, hash) = (
        pw.to_owned(),
        creds.as_ref().map(|c| c.password_hash.clone()),
    );
    // Argon2 is CPU bound: keep it off the async workers. Unknown users still
    // pay for one verification so that timing does not reveal them.
    let valid = tokio::task::spawn_blocking(move || match hash {
        Some(hash) => password::verify_password(&pw_owned, &hash),
        None => {
            password::verify_dummy(&pw_owned);
            false
        }
    })
    .await
    .map_err(anyhow::Error::from)?;
    let Some(creds) = creds else {
        return Err(AppError::Unauthorized(INVALID_CREDENTIALS));
    };
    let locked = creds.locked_until.is_some_and(|until| until > Utc::now());
    if locked || !valid {
        if !locked {
            let failures = users::record_failed_login(db, creds.user.id).await?;
            if let Some(duration) = lockout_duration(failures) {
                users::lock_until(db, creds.user.id, Utc::now() + duration).await?;
                tracing::warn!(user_id = %creds.user.id, failures, "account temporarily locked");
            }
        }
        return Err(AppError::Unauthorized(INVALID_CREDENTIALS));
    }
    users::reset_login_failures(db, creds.user.id).await?;
    if creds.suspended {
        return Err(AppError::Forbidden(SUSPENDED.into()));
    }
    Ok(creds.user)
}

/// Rotates a refresh token. Reuse of a consumed token revokes its family.
pub async fn rotate(
    db: &PgPool,
    cfg: SessionConfig<'_>,
    refresh_token: &str,
) -> Result<Session, AppError> {
    let Some(stored) = tokens::find(db, &token_digest(refresh_token)).await? else {
        return Err(AppError::Unauthorized(INVALID_SESSION));
    };
    if stored.revoked_at.is_some() || stored.expires_at <= Utc::now() {
        return Err(AppError::Unauthorized(INVALID_SESSION));
    }
    if stored.used_at.is_some() || !tokens::mark_used(db, stored.id).await? {
        tokens::revoke_family(db, stored.family_id).await?;
        tracing::warn!(user_id = %stored.user_id, family = %stored.family_id, "refresh token reuse detected; family revoked");
        return Err(AppError::Unauthorized(INVALID_SESSION));
    }
    let user = users::find_active(db, stored.user_id)
        .await?
        .ok_or(AppError::Unauthorized(INVALID_SESSION))?;
    issue(db, cfg, user, stored.family_id).await
}

/// Revokes the family of `refresh_token` (logout). Unknown tokens are ignored.
pub async fn revoke(db: &PgPool, refresh_token: &str) -> Result<(), AppError> {
    if let Some(stored) = tokens::find(db, &token_digest(refresh_token)).await? {
        tokens::revoke_family(db, stored.family_id).await?;
    }
    Ok(())
}
