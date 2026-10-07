//! What the platform console reads and writes: every workspace and account
//! of the installation, and the log of what its administrators did.
//!
//! Nothing here is scoped to a member: these queries are only reachable
//! through `http::extract::PlatformAdmin`.

use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::platform::{
    PlatformAction, PlatformEvent, PlatformMember, PlatformUser, PlatformWorkspace,
    WorkspaceFootprint,
};
use crate::domain::user::Role;

/// A page of a platform list: an optional search term, a size and a start.
#[derive(Debug, Clone)]
pub struct Page {
    pub q: Option<String>,
    pub limit: i64,
    pub offset: i64,
}

impl FromRow<'_, PgRow> for PlatformMember {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(PlatformMember {
            user_id: row.try_get("user_id")?,
            name: row.try_get("name")?,
            email: row.try_get("email")?,
            role: enum_col(row, "role")?,
            suspended: row.try_get("suspended")?,
            platform_admin: row.try_get("platform_admin")?,
            joined_at: row.try_get("joined_at")?,
        })
    }
}

impl FromRow<'_, PgRow> for PlatformUser {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(PlatformUser {
            id: row.try_get("id")?,
            email: row.try_get("email")?,
            name: row.try_get("name")?,
            role: enum_col(row, "role")?,
            workspace_count: row.try_get("workspace_count")?,
            owned_count: row.try_get("owned_count")?,
            locked: row.try_get("locked")?,
            suspended: row.try_get("suspended")?,
            suspended_reason: row.try_get("suspended_reason")?,
            two_factor: row.try_get("two_factor")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

impl FromRow<'_, PgRow> for PlatformEvent {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(PlatformEvent {
            id: row.try_get("id")?,
            action: enum_col(row, "action")?,
            actor_id: row.try_get("actor_id")?,
            actor_name: row.try_get("actor_name")?,
            subject: row.try_get("subject")?,
            detail: row.try_get("detail")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// A workspace row with its first owner and its counts; `$w` filters it.
macro_rules! workspace_rows {
    ($w:literal) => {
        concat!(
            "SELECT w.id, w.name, o.name AS owner_name, o.email AS owner_email,
                    (SELECT count(*) FROM workspace_members m WHERE m.workspace_id = w.id)
                        AS member_count,
                    (SELECT count(*) FROM teams t WHERE t.workspace_id = w.id) AS team_count,
                    (SELECT count(*) FROM issues i WHERE i.workspace_id = w.id) AS issue_count,
                    (SELECT count(*) FROM graphs g WHERE g.workspace_id = w.id) AS graph_count,
                    w.created_at
             FROM workspaces w
             LEFT JOIN LATERAL (
                 SELECT u.name, u.email FROM workspace_members m JOIN users u ON u.id = m.user_id
                 WHERE m.workspace_id = w.id AND m.role = 'owner'
                 ORDER BY m.created_at, u.id LIMIT 1) o ON true
             WHERE ",
            $w
        )
    };
}

/// Every workspace, newest first, matched by its name or its owner.
pub async fn workspaces(
    db: impl PgExecutor<'_>,
    page: &Page,
) -> Result<Vec<PlatformWorkspace>, sqlx::Error> {
    sqlx::query_as(concat!(
        workspace_rows!(
            "$1::text IS NULL OR w.name ILIKE '%' || $1 || '%'
                OR o.email ILIKE '%' || $1 || '%' OR o.name ILIKE '%' || $1 || '%'"
        ),
        " ORDER BY w.created_at DESC, w.id LIMIT $2 OFFSET $3"
    ))
    .bind(&page.q)
    .bind(page.limit)
    .bind(page.offset)
    .fetch_all(db)
    .await
}

/// One workspace.
pub async fn workspace(
    db: impl PgExecutor<'_>,
    id: Uuid,
) -> Result<Option<PlatformWorkspace>, sqlx::Error> {
    sqlx::query_as(workspace_rows!("w.id = $1"))
        .bind(id)
        .fetch_optional(db)
        .await
}

/// Members of a workspace: owners first, then by name.
pub async fn members(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<Vec<PlatformMember>, sqlx::Error> {
    sqlx::query_as(
        "SELECT u.id AS user_id, u.name, u.email, m.role,
                u.suspended_at IS NOT NULL AS suspended, u.role = 'admin' AS platform_admin,
                m.created_at AS joined_at
         FROM workspace_members m JOIN users u ON u.id = m.user_id
         WHERE m.workspace_id = $1
         ORDER BY array_position(ARRAY['owner', 'admin', 'member', 'guest'], m.role),
                  lower(u.name), u.id",
    )
    .bind(workspace_id)
    .fetch_all(db)
    .await
}

/// How much a workspace holds.
pub async fn footprint(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<WorkspaceFootprint, sqlx::Error> {
    sqlx::query_as(
        "SELECT
            (SELECT count(*) FROM projects p WHERE p.workspace_id = $1) AS project_count,
            (SELECT count(*) FROM documents d WHERE d.workspace_id = $1) AS document_count,
            (SELECT COALESCE(sum(d.size_bytes), 0)::bigint FROM documents d
              WHERE d.workspace_id = $1) AS document_bytes,
            (SELECT count(*) FROM memories x WHERE x.workspace_id = $1) AS memory_count,
            (SELECT count(*) FROM runs r JOIN graphs g ON g.id = r.graph_id
              WHERE g.workspace_id = $1) AS run_count",
    )
    .bind(workspace_id)
    .fetch_one(db)
    .await
}

/// An account row with its memberships; `$w` filters it. Deleted accounts
/// are left out: nothing in them names anyone any more.
macro_rules! user_rows {
    ($w:literal) => {
        concat!(
            "SELECT u.id, u.email, u.name, u.role,
                    (SELECT count(*) FROM workspace_members m WHERE m.user_id = u.id)
                        AS workspace_count,
                    (SELECT count(*) FROM workspace_members m
                      WHERE m.user_id = u.id AND m.role = 'owner') AS owned_count,
                    COALESCE(u.locked_until > now(), false) AS locked,
                    u.suspended_at IS NOT NULL AS suspended, u.suspended_reason,
                    u.totp_enabled_at IS NOT NULL AS two_factor, u.created_at
             FROM users u WHERE u.deleted_at IS NULL AND (",
            $w,
            ")"
        )
    };
}

/// Every account, newest first, matched by name or e-mail.
pub async fn users(db: impl PgExecutor<'_>, page: &Page) -> Result<Vec<PlatformUser>, sqlx::Error> {
    sqlx::query_as(concat!(
        user_rows!(
            "$1::text IS NULL OR u.email ILIKE '%' || $1 || '%' OR u.name ILIKE '%' || $1 || '%'"
        ),
        " ORDER BY u.created_at DESC, u.id LIMIT $2 OFFSET $3"
    ))
    .bind(&page.q)
    .bind(page.limit)
    .bind(page.offset)
    .fetch_all(db)
    .await
}

/// One account.
pub async fn user(db: impl PgExecutor<'_>, id: Uuid) -> Result<Option<PlatformUser>, sqlx::Error> {
    sqlx::query_as(user_rows!("u.id = $1"))
        .bind(id)
        .fetch_optional(db)
        .await
}

/// One account, by its e-mail address.
pub async fn user_by_email(
    db: impl PgExecutor<'_>,
    email: &str,
) -> Result<Option<PlatformUser>, sqlx::Error> {
    sqlx::query_as(user_rows!("lower(u.email) = lower($1)"))
        .bind(email)
        .fetch_optional(db)
        .await
}

/// Names of the workspaces that would be left without a usable owner if
/// this account stopped working in them: it is their only owner and they
/// have other members.
pub async fn sole_owner_of(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT w.name FROM workspaces w
         JOIN workspace_members mine ON mine.workspace_id = w.id
              AND mine.user_id = $1 AND mine.role = 'owner'
         WHERE NOT EXISTS (SELECT 1 FROM workspace_members o WHERE o.workspace_id = w.id
                           AND o.role = 'owner' AND o.user_id <> $1)
           AND EXISTS (SELECT 1 FROM workspace_members o WHERE o.workspace_id = w.id
                       AND o.user_id <> $1)
         ORDER BY lower(w.name)",
    )
    .bind(user_id)
    .fetch_all(db)
    .await
}

/// Sets the platform role and ends the account's access tokens. Returns the
/// new session epoch, or `None` when the role was already that.
pub async fn set_role(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    role: Role,
) -> Result<Option<i32>, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE users SET role = $2, session_epoch = session_epoch + 1, session_epoch_at = now(),
                updated_at = now()
         WHERE id = $1 AND role <> $2 RETURNING session_epoch",
    )
    .bind(user_id)
    .bind(role.as_str())
    .fetch_optional(db)
    .await
}

/// Suspends an account and ends its access tokens. Returns the new session
/// epoch, or `None` when it was suspended already.
pub async fn suspend(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    reason: &str,
) -> Result<Option<i32>, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE users SET suspended_at = now(), suspended_reason = $2,
                session_epoch = session_epoch + 1, session_epoch_at = now(), updated_at = now()
         WHERE id = $1 AND suspended_at IS NULL RETURNING session_epoch",
    )
    .bind(user_id)
    .bind(reason)
    .fetch_optional(db)
    .await
}

/// Lets a suspended account sign in again. Returns false when it was not suspended.
pub async fn reactivate(db: impl PgExecutor<'_>, user_id: Uuid) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE users SET suspended_at = NULL, suspended_reason = '', updated_at = now()
         WHERE id = $1 AND suspended_at IS NOT NULL",
    )
    .bind(user_id)
    .execute(db)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Appends an entry to the platform's activity log; the administrator's
/// name is copied from their account.
pub async fn record(
    db: impl PgExecutor<'_>,
    actor_id: Uuid,
    action: PlatformAction,
    subject: &str,
    detail: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO platform_events (id, actor_id, actor_name, action, subject, detail)
         VALUES ($1, $2, COALESCE((SELECT name || ' <' || email || '>' FROM users WHERE id = $2), ''),
                 $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(actor_id)
    .bind(action.as_str())
    .bind(subject)
    .bind(detail)
    .execute(db)
    .await?;
    Ok(())
}

/// The platform's activity log, newest first.
pub async fn events(
    db: impl PgExecutor<'_>,
    page: &Page,
) -> Result<Vec<PlatformEvent>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, action, actor_id, actor_name, subject, detail, created_at
         FROM platform_events
         WHERE $1::text IS NULL OR subject ILIKE '%' || $1 || '%'
            OR actor_name ILIKE '%' || $1 || '%' OR detail ILIKE '%' || $1 || '%'
         ORDER BY created_at DESC, id DESC LIMIT $2 OFFSET $3",
    )
    .bind(&page.q)
    .bind(page.limit)
    .bind(page.offset)
    .fetch_all(db)
    .await
}
