//! Workspaces, their members and pending invites.

use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::workspace::{Workspace, WorkspaceInvite, WorkspaceMember, WorkspaceRole};

impl FromRow<'_, PgRow> for Workspace {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(Workspace {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            slug: row.try_get("slug")?,
            role: enum_col(row, "role")?,
            member_count: row.try_get("member_count")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

impl FromRow<'_, PgRow> for WorkspaceMember {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(WorkspaceMember {
            user_id: row.try_get("user_id")?,
            name: row.try_get("name")?,
            email: row.try_get("email")?,
            role: enum_col(row, "role")?,
            joined_at: row.try_get("joined_at")?,
        })
    }
}

impl FromRow<'_, PgRow> for WorkspaceInvite {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(WorkspaceInvite {
            id: row.try_get("id")?,
            email: row.try_get("email")?,
            role: enum_col(row, "role")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

macro_rules! workspace_for_member {
    ($tail:literal) => {
        concat!(
            "SELECT w.id, w.name, w.slug, w.created_at, m.role,
        (SELECT count(*) FROM workspace_members c WHERE c.workspace_id = w.id) AS member_count
    FROM workspaces w JOIN workspace_members m ON m.workspace_id = w.id AND m.user_id = $1",
            " ",
            $tail
        )
    };
}

/// Inserts a workspace. A taken slug is a unique violation (409).
pub async fn create(
    db: impl PgExecutor<'_>,
    name: &str,
    slug: &str,
    created_by: Uuid,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO workspaces (id, name, slug, created_by) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(name)
    .bind(slug)
    .bind(created_by)
    .fetch_one(db)
    .await
}

/// Whether a slug is taken.
pub async fn slug_exists(db: impl PgExecutor<'_>, slug: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM workspaces WHERE slug = $1)")
        .bind(slug)
        .fetch_one(db)
        .await
}

/// The workspaces `user_id` belongs to, oldest first.
pub async fn list_for(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
) -> Result<Vec<Workspace>, sqlx::Error> {
    sqlx::query_as(workspace_for_member!("ORDER BY w.created_at, w.id"))
        .bind(user_id)
        .fetch_all(db)
        .await
}

/// One workspace as seen by `user_id`; `None` unless they are a member.
pub async fn find_for(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    id: Uuid,
) -> Result<Option<Workspace>, sqlx::Error> {
    sqlx::query_as(workspace_for_member!("WHERE w.id = $2"))
        .bind(user_id)
        .bind(id)
        .fetch_optional(db)
        .await
}

/// Renames a workspace.
pub async fn rename(db: impl PgExecutor<'_>, id: Uuid, name: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE workspaces SET name = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(name)
        .execute(db)
        .await?;
    Ok(())
}

/// Deletes a workspace with its teams, members and invites.
pub async fn delete(db: impl PgExecutor<'_>, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM workspaces WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// Number of workspaces a user belongs to.
pub async fn count_for(db: impl PgExecutor<'_>, user_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM workspace_members WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(db)
        .await
}

/// The role of `user_id` in a workspace, if they are a member.
pub async fn role_of(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<Option<WorkspaceRole>, sqlx::Error> {
    let role: Option<String> = sqlx::query_scalar(
        "SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    Ok(role.and_then(|r| r.parse().ok()))
}

/// Adds a member; returns false if they already belong to the workspace.
pub async fn add_member(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    user_id: Uuid,
    role: WorkspaceRole,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3)
         ON CONFLICT DO NOTHING",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(role.as_str())
    .execute(db)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Changes a member's role.
pub async fn set_role(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    user_id: Uuid,
    role: WorkspaceRole,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE workspace_members SET role = $3 WHERE workspace_id = $1 AND user_id = $2")
        .bind(workspace_id)
        .bind(user_id)
        .bind(role.as_str())
        .execute(db)
        .await?;
    Ok(())
}

/// Removes a member from the workspace and from all of its teams.
pub async fn remove_member(
    db: &mut sqlx::PgConnection,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM team_members tm USING teams t
         WHERE tm.team_id = t.id AND t.workspace_id = $1 AND tm.user_id = $2",
    )
    .bind(workspace_id)
    .bind(user_id)
    .execute(&mut *db)
    .await?;
    sqlx::query("DELETE FROM workspace_members WHERE workspace_id = $1 AND user_id = $2")
        .bind(workspace_id)
        .bind(user_id)
        .execute(&mut *db)
        .await?;
    Ok(())
}

/// Number of owners of a workspace. Locks the member rows so that two
/// concurrent demotions cannot both see "another owner remains".
pub async fn lock_owner_count(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<usize, sqlx::Error> {
    let owners: Vec<Uuid> = sqlx::query_scalar(
        "SELECT user_id FROM workspace_members WHERE workspace_id = $1 AND role = 'owner' FOR UPDATE",
    )
    .bind(workspace_id)
    .fetch_all(db)
    .await?;
    Ok(owners.len())
}

/// Members of a workspace: owners first, then by name.
pub async fn members(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<Vec<WorkspaceMember>, sqlx::Error> {
    sqlx::query_as(
        "SELECT u.id AS user_id, u.name, u.email, m.role, m.created_at AS joined_at
         FROM workspace_members m JOIN users u ON u.id = m.user_id
         WHERE m.workspace_id = $1
         ORDER BY array_position(ARRAY['owner', 'admin', 'member', 'guest'], m.role), lower(u.name), u.id",
    )
    .bind(workspace_id)
    .fetch_all(db)
    .await
}

/// The id of the user with this e-mail address, if any.
pub async fn user_id_by_email(
    db: impl PgExecutor<'_>,
    email: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM users WHERE lower(email) = lower($1)")
        .bind(email)
        .fetch_optional(db)
        .await
}

/// Records an invitation; inviting the same address again updates its role.
pub async fn upsert_invite(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    email: &str,
    role: WorkspaceRole,
    invited_by: Uuid,
) -> Result<WorkspaceInvite, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO workspace_invites (id, workspace_id, email, role, invited_by)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (workspace_id, lower(email)) DO UPDATE SET role = EXCLUDED.role
         RETURNING id, email, role, created_at",
    )
    .bind(Uuid::now_v7())
    .bind(workspace_id)
    .bind(email)
    .bind(role.as_str())
    .bind(invited_by)
    .fetch_one(db)
    .await
}

/// Pending invitations of a workspace, newest first.
pub async fn invites(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<Vec<WorkspaceInvite>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, email, role, created_at FROM workspace_invites
         WHERE workspace_id = $1 ORDER BY created_at DESC, id",
    )
    .bind(workspace_id)
    .fetch_all(db)
    .await
}

/// Withdraws an invitation. Returns false if absent.
pub async fn delete_invite(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query("DELETE FROM workspace_invites WHERE workspace_id = $1 AND id = $2")
        .bind(workspace_id)
        .bind(id)
        .execute(db)
        .await?;
    Ok(done.rows_affected() == 1)
}

/// Turns every invitation sent to `email` into a membership of `user_id`.
pub async fn accept_invites(
    db: &mut sqlx::PgConnection,
    user_id: Uuid,
    email: &str,
) -> Result<u64, sqlx::Error> {
    let done = sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role)
         SELECT workspace_id, $1, role FROM workspace_invites WHERE lower(email) = lower($2)
         ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(email)
    .execute(&mut *db)
    .await?;
    sqlx::query("DELETE FROM workspace_invites WHERE lower(email) = lower($1)")
        .bind(email)
        .execute(&mut *db)
        .await?;
    Ok(done.rows_affected())
}

/// The workspace a user works in when they name none: the first one they
/// joined as more than a guest.
pub async fn default_for(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT workspace_id FROM workspace_members WHERE user_id = $1 AND role <> 'guest'
         ORDER BY created_at, workspace_id LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(db)
    .await
}

/// Users that belong to no workspace yet, as `(id, name)`.
pub async fn users_without_workspace(
    db: impl PgExecutor<'_>,
) -> Result<Vec<(Uuid, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT u.id, u.name FROM users u
         WHERE NOT EXISTS (SELECT 1 FROM workspace_members m WHERE m.user_id = u.id)
         ORDER BY u.created_at",
    )
    .fetch_all(db)
    .await
}
