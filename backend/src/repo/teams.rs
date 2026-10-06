//! Teams and their members. Every query is scoped by `workspace_id` so a
//! team id from another workspace never matches.

use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::workspace::{Team, TeamMember, TeamRole};

impl FromRow<'_, PgRow> for Team {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let role: Option<String> = row.try_get("role")?;
        Ok(Team {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            name: row.try_get("name")?,
            key: row.try_get("key")?,
            description: row.try_get("description")?,
            private: row.try_get("private")?,
            role: role.and_then(|r| r.parse().ok()),
            member_count: row.try_get("member_count")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

impl FromRow<'_, PgRow> for TeamMember {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(TeamMember {
            user_id: row.try_get("user_id")?,
            name: row.try_get("name")?,
            email: row.try_get("email")?,
            role: enum_col(row, "role")?,
            joined_at: row.try_get("joined_at")?,
        })
    }
}

/// Teams with the role of user `$1` in each (one LEFT JOIN, no per-team query).
macro_rules! team_for_user {
    ($tail:literal) => {
        concat!(
            "SELECT t.id, t.workspace_id, t.name, t.key, t.description, t.private,
        t.created_at, tm.role,
        (SELECT count(*) FROM team_members c WHERE c.team_id = t.id) AS member_count
    FROM teams t LEFT JOIN team_members tm ON tm.team_id = t.id AND tm.user_id = $1",
            " ",
            $tail
        )
    };
}

/// Inserts a team. A key already used in the workspace is a unique violation (409).
pub async fn create(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    name: &str,
    key: &str,
    description: &str,
    private: bool,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO teams (id, workspace_id, name, key, description, private)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(workspace_id)
    .bind(name)
    .bind(key)
    .bind(description)
    .bind(private)
    .fetch_one(db)
    .await
}

/// Every team of a workspace with the caller's role in it; the caller
/// decides which of them are visible.
pub async fn list(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<Vec<Team>, sqlx::Error> {
    sqlx::query_as(team_for_user!(
        "WHERE t.workspace_id = $2 ORDER BY lower(t.name), t.id"
    ))
    .bind(user_id)
    .bind(workspace_id)
    .fetch_all(db)
    .await
}

/// One team of a workspace with the caller's role in it.
pub async fn find(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<Team>, sqlx::Error> {
    sqlx::query_as(team_for_user!("WHERE t.workspace_id = $2 AND t.id = $3"))
        .bind(user_id)
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(db)
        .await
}

/// The workspace of a team and the role of `user_id` in the team, if any.
pub async fn membership(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    id: Uuid,
) -> Result<Option<(Uuid, Option<TeamRole>)>, sqlx::Error> {
    let row: Option<(Uuid, Option<String>)> = sqlx::query_as(
        "SELECT t.workspace_id, tm.role FROM teams t
         LEFT JOIN team_members tm ON tm.team_id = t.id AND tm.user_id = $1 WHERE t.id = $2",
    )
    .bind(user_id)
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(row.map(|(workspace, role)| (workspace, role.and_then(|r| r.parse().ok()))))
}

/// Updates name / description / visibility (each optional).
pub async fn update(
    db: impl PgExecutor<'_>,
    id: Uuid,
    name: Option<&str>,
    description: Option<&str>,
    private: Option<bool>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE teams SET name = COALESCE($2, name), description = COALESCE($3, description),
                private = COALESCE($4, private), updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(name)
    .bind(description)
    .bind(private)
    .execute(db)
    .await?;
    Ok(())
}

/// Deletes a team and its memberships.
pub async fn delete(db: impl PgExecutor<'_>, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM teams WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// Adds a member or changes their role.
pub async fn upsert_member(
    db: impl PgExecutor<'_>,
    team_id: Uuid,
    user_id: Uuid,
    role: TeamRole,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO team_members (team_id, user_id, role) VALUES ($1, $2, $3)
         ON CONFLICT (team_id, user_id) DO UPDATE SET role = EXCLUDED.role",
    )
    .bind(team_id)
    .bind(user_id)
    .bind(role.as_str())
    .execute(db)
    .await?;
    Ok(())
}

/// Removes a member. Returns false if they were not in the team.
pub async fn remove_member(
    db: impl PgExecutor<'_>,
    team_id: Uuid,
    user_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query("DELETE FROM team_members WHERE team_id = $1 AND user_id = $2")
        .bind(team_id)
        .bind(user_id)
        .execute(db)
        .await?;
    Ok(done.rows_affected() == 1)
}

/// Members of a team: owners first, then by name.
pub async fn members(
    db: impl PgExecutor<'_>,
    team_id: Uuid,
) -> Result<Vec<TeamMember>, sqlx::Error> {
    sqlx::query_as(
        "SELECT u.id AS user_id, u.name, u.email, tm.role, tm.created_at AS joined_at
         FROM team_members tm JOIN users u ON u.id = tm.user_id
         WHERE tm.team_id = $1
         ORDER BY (tm.role <> 'owner'), lower(u.name), u.id",
    )
    .bind(team_id)
    .fetch_all(db)
    .await
}
