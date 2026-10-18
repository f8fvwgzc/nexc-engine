//! Single sign-on: the provider identities linked to accounts, the sign-ins
//! in flight, and the roles the group mapping gave.

use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgConnection, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::user::{Role, User};
use crate::domain::workspace::WorkspaceRole;

/// An authorization request waiting for the provider to send the browser back.
#[derive(Debug, Clone, FromRow)]
pub struct PendingLogin {
    pub browser_hash: String,
    pub nonce: String,
    pub code_verifier: String,
    pub return_to: String,
    pub expires_at: DateTime<Utc>,
}

/// Records an authorization request under the digest of its `state`.
pub async fn start_login(
    db: impl PgExecutor<'_>,
    state_hash: &str,
    login: &PendingLogin,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO oidc_logins (state_hash, browser_hash, nonce, code_verifier, return_to, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(state_hash)
    .bind(&login.browser_hash)
    .bind(&login.nonce)
    .bind(&login.code_verifier)
    .bind(&login.return_to)
    .bind(login.expires_at)
    .execute(db)
    .await?;
    Ok(())
}

/// Takes the request a `state` names: it is deleted as it is read, so a state
/// works once, however many callbacks race for it.
pub async fn take_login(
    db: impl PgExecutor<'_>,
    state_hash: &str,
) -> Result<Option<PendingLogin>, sqlx::Error> {
    sqlx::query_as(
        "DELETE FROM oidc_logins WHERE state_hash = $1
         RETURNING browser_hash, nonce, code_verifier, return_to, expires_at",
    )
    .bind(state_hash)
    .fetch_optional(db)
    .await
}

/// Deletes the requests whose time is up.
pub async fn purge_logins(db: impl PgExecutor<'_>) -> Result<u64, sqlx::Error> {
    let done = sqlx::query("DELETE FROM oidc_logins WHERE expires_at < now()")
        .execute(db)
        .await?;
    Ok(done.rows_affected())
}

/// The account an identity of `issuer` is linked to.
pub async fn linked_user(
    db: impl PgExecutor<'_>,
    issuer: &str,
    subject: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT user_id FROM user_identities WHERE issuer = $1 AND subject = $2")
        .bind(issuer)
        .bind(subject)
        .fetch_optional(db)
        .await
}

/// Whether the account has an identity of `issuer` already.
pub async fn has_identity_at(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    issuer: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM user_identities WHERE user_id = $1 AND issuer = $2)",
    )
    .bind(user_id)
    .bind(issuer)
    .fetch_one(db)
    .await
}

/// Links an identity to an account. `false` when the identity, or another
/// identity of the issuer for this account, is linked already.
pub async fn link(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    issuer: &str,
    subject: &str,
    email: &str,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "INSERT INTO user_identities (id, user_id, issuer, subject, email, last_login_at)
         VALUES ($1, $2, $3, $4, $5, now())
         ON CONFLICT DO NOTHING",
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(issuer)
    .bind(subject)
    .bind(email)
    .execute(db)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Notes a sign-in with an identity, and the address the provider gave this time.
pub async fn touch(
    db: impl PgExecutor<'_>,
    issuer: &str,
    subject: &str,
    email: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE user_identities SET last_login_at = now(), email = $3
         WHERE issuer = $1 AND subject = $2",
    )
    .bind(issuer)
    .bind(subject)
    .bind(email)
    .execute(db)
    .await?;
    Ok(())
}

/// One identity linked to an account, as its holder sees it.
#[derive(Debug, Clone, FromRow)]
pub struct IdentityRow {
    pub issuer: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
}

/// The identities linked to an account, oldest first.
pub async fn identities(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
) -> Result<Vec<IdentityRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT issuer, email, created_at, last_login_at FROM user_identities
         WHERE user_id = $1 ORDER BY created_at, id",
    )
    .bind(user_id)
    .fetch_all(db)
    .await
}

/// Whether an account has a password ("!" is none: single sign-on created it).
pub async fn has_password(db: impl PgExecutor<'_>, user_id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT password_hash <> '!' FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(db)
        .await
        .map(|found| found.unwrap_or(false))
}

/// An account as a sign-in through the provider sees it.
#[derive(Debug, Clone)]
pub struct SsoAccount {
    pub user: User,
    pub suspended: bool,
    /// The platform role came from the admin groups (`NEXC_OIDC_ADMIN_GROUPS`).
    pub admin_by_sso: bool,
}

impl FromRow<'_, PgRow> for SsoAccount {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(SsoAccount {
            user: User::from_row(row)?,
            suspended: row.try_get("suspended")?,
            admin_by_sso: row.try_get("admin_by_sso")?,
        })
    }
}

/// An account that was not deleted, by id; its row is locked until the
/// transaction ends when `db` is one, so that two sign-ins of one account
/// apply the group mapping one after the other.
pub async fn account(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
) -> Result<Option<SsoAccount>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, email, name, role, created_at, suspended_at IS NOT NULL AS suspended,
                admin_by_sso
         FROM users WHERE id = $1 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(user_id)
    .fetch_optional(db)
    .await
}

/// Sets the platform role and whether the group mapping gave it, and ends
/// the account's access tokens. Returns the new session epoch, or `None`
/// when the role was that already.
pub async fn set_platform_role(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    role: Role,
    by_sso: bool,
) -> Result<Option<i32>, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE users SET role = $2, admin_by_sso = $3, session_epoch = session_epoch + 1,
                session_epoch_at = now(), updated_at = now()
         WHERE id = $1 AND role <> $2 RETURNING session_epoch",
    )
    .bind(user_id)
    .bind(role.as_str())
    .bind(by_sso)
    .fetch_optional(db)
    .await
}

/// A workspace role the group mapping gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleGrant {
    pub workspace_id: Uuid,
    pub role: WorkspaceRole,
    /// The role before the grant; `None` when the account was not a member.
    pub prior_role: Option<WorkspaceRole>,
}

impl FromRow<'_, PgRow> for RoleGrant {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let prior: Option<String> = row.try_get("prior_role")?;
        Ok(RoleGrant {
            workspace_id: row.try_get("workspace_id")?,
            role: enum_col(row, "role")?,
            prior_role: prior.and_then(|p| p.parse().ok()),
        })
    }
}

/// The roles the mapping gave the account, locked until the transaction ends.
pub async fn lock_grants(
    db: &mut PgConnection,
    user_id: Uuid,
) -> Result<Vec<RoleGrant>, sqlx::Error> {
    sqlx::query_as(
        "SELECT workspace_id, role, prior_role FROM sso_role_grants
         WHERE user_id = $1 ORDER BY workspace_id FOR UPDATE",
    )
    .bind(user_id)
    .fetch_all(db)
    .await
}

/// Records (or replaces) what the mapping gave in a workspace.
pub async fn put_grant(
    db: &mut PgConnection,
    user_id: Uuid,
    grant: &RoleGrant,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO sso_role_grants (user_id, workspace_id, role, prior_role)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (user_id, workspace_id)
         DO UPDATE SET role = EXCLUDED.role, prior_role = EXCLUDED.prior_role",
    )
    .bind(user_id)
    .bind(grant.workspace_id)
    .bind(grant.role.as_str())
    .bind(grant.prior_role.map(WorkspaceRole::as_str))
    .execute(db)
    .await?;
    Ok(())
}

/// Forgets what the mapping gave in a workspace.
pub async fn drop_grant(
    db: &mut PgConnection,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM sso_role_grants WHERE user_id = $1 AND workspace_id = $2")
        .bind(user_id)
        .bind(workspace_id)
        .execute(db)
        .await?;
    Ok(())
}
