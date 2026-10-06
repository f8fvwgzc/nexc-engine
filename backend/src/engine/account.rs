//! What a person may ask of their own account: a copy of what is held about
//! them, and its deletion.
//!
//! Deleting does not remove the row. Graphs, agents, runs and memories
//! cascade from an account's id, so removing it would take a team's shared
//! work with it. Instead the account is emptied of everything personal and
//! of every way in, and what its owner made stays, attributed to "Deleted
//! account".

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::AssertSqlSafe;
use utoipa::ToSchema;
use uuid::Uuid;

use super::{artifacts, workspaces};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::account::AccountEventKind;
use crate::domain::audit::AuditAction;
use crate::domain::user::User;
use crate::repo::audit::Subject;
use crate::repo::{self, OrNotFound};

/// Most rows of one section of an export; a longer section says it was cut.
pub const EXPORT_SECTION_MAX: usize = 5_000;
/// What a deleted account is called wherever its work is still shown.
pub const DELETED_NAME: &str = "Deleted account";

/// Notes something in an account's security activity. A failure is logged,
/// not returned: what it describes has already happened.
pub async fn note(
    state: &AppState,
    user: Uuid,
    kind: AccountEventKind,
    ip: Option<std::net::IpAddr>,
    detail: &str,
) {
    if let Err(err) = repo::account_events::record(&state.db, user, kind, ip, detail).await {
        tracing::error!(user_id = %user, %kind, error = %err, "account event was not written");
    }
}

/// A copy of what the installation holds about one account.
#[derive(Debug, Serialize, ToSchema)]
pub struct AccountExport {
    pub exported_at: DateTime<Utc>,
    pub account: User,
    /// `workspaces`, `teams`, `issues_created`, `issues_assigned`,
    /// `comments`, `graphs`, `documents`, `memories`, `ai_account`, `usage`,
    /// `security_activity`, `notifications`: each a list of plain objects.
    #[schema(value_type = Object)]
    pub sections: Map<String, Value>,
    /// Sections that hold more than [`EXPORT_SECTION_MAX`] rows and were cut there.
    pub truncated: Vec<String>,
}

/// The sections of an export: a name and the rows of account `$1`.
const SECTIONS: &[(&str, &str)] = &[
    (
        "workspaces",
        "SELECT w.id, w.name, m.role, m.created_at AS joined_at
         FROM workspace_members m JOIN workspaces w ON w.id = m.workspace_id
         WHERE m.user_id = $1 ORDER BY m.created_at",
    ),
    (
        "teams",
        "SELECT w.name AS workspace, t.name AS team, t.key, tm.role, tm.created_at AS joined_at
         FROM team_members tm JOIN teams t ON t.id = tm.team_id
         JOIN workspaces w ON w.id = t.workspace_id
         WHERE tm.user_id = $1 ORDER BY tm.created_at",
    ),
    (
        "issues_created",
        "SELECT t.key || '-' || i.number AS identifier, i.title, i.description,
                w.name AS workspace, i.created_at
         FROM issues i JOIN teams t ON t.id = i.team_id JOIN workspaces w ON w.id = i.workspace_id
         WHERE i.creator_id = $1 ORDER BY i.created_at",
    ),
    (
        "issues_assigned",
        "SELECT t.key || '-' || i.number AS identifier, i.title, w.name AS workspace
         FROM issues i JOIN teams t ON t.id = i.team_id JOIN workspaces w ON w.id = i.workspace_id
         WHERE i.assignee_id = $1 ORDER BY i.created_at",
    ),
    (
        "comments",
        "SELECT t.key || '-' || i.number AS issue, e.body, e.created_at
         FROM issue_events e JOIN issues i ON i.id = e.issue_id JOIN teams t ON t.id = i.team_id
         WHERE e.actor_id = $1 AND e.kind = 'comment' ORDER BY e.created_at",
    ),
    (
        "graphs",
        "SELECT g.id, g.name, g.goal, w.name AS workspace, g.created_at
         FROM graphs g LEFT JOIN workspaces w ON w.id = g.workspace_id
         WHERE g.owner_id = $1 ORDER BY g.created_at",
    ),
    (
        "documents",
        "SELECT d.name, d.size_bytes, w.name AS workspace, d.created_at
         FROM documents d JOIN workspaces w ON w.id = d.workspace_id
         WHERE d.uploaded_by = $1 ORDER BY d.created_at",
    ),
    (
        "memories",
        "SELECT m.content, m.kind, m.scope, m.created_at FROM memories m
         WHERE m.owner_id = $1 AND m.scope = 'user' ORDER BY m.created_at",
    ),
    (
        "ai_account",
        "SELECT s.provider, s.model, s.base_url, s.api_key_enc IS NOT NULL AS has_api_key,
                s.updated_at
         FROM llm_settings s WHERE s.user_id = $1",
    ),
    (
        "usage",
        "SELECT to_char(date_trunc('month', u.created_at), 'YYYY-MM') AS month, u.purpose,
                count(*) AS calls, sum(u.tokens_in) AS tokens_in, sum(u.tokens_out) AS tokens_out
         FROM llm_usage u WHERE u.user_id = $1 GROUP BY 1, 2 ORDER BY 1, 2",
    ),
    (
        "security_activity",
        "SELECT e.kind, e.ip, e.detail, e.created_at FROM account_events e
         WHERE e.user_id = $1 ORDER BY e.created_at",
    ),
    (
        "notifications",
        "SELECT n.kind, n.created_at, n.read_at FROM notifications n
         WHERE n.user_id = $1 ORDER BY n.created_at",
    ),
];

/// Everything held about `user`: the account, where it belongs, what it
/// wrote and what it spent. Never a credential: no password hash, no key.
pub async fn export(state: &AppState, user: Uuid) -> Result<AccountExport, AppError> {
    let account = repo::users::find(&state.db, user)
        .await
        .or_not_found("user")?;
    let mut sections = Map::new();
    let mut truncated = Vec::new();
    for (name, rows) in SECTIONS {
        let sql = format!(
            "SELECT COALESCE(jsonb_agg(to_jsonb(x)), '[]'::jsonb) FROM ({rows} LIMIT $2) x"
        );
        let mut found: Value = sqlx::query_scalar(AssertSqlSafe(sql))
            .bind(user)
            .bind(i64::try_from(EXPORT_SECTION_MAX + 1).unwrap_or(i64::MAX))
            .fetch_one(&state.db)
            .await?;
        if let Some(list) = found.as_array_mut()
            && list.len() > EXPORT_SECTION_MAX
        {
            list.truncate(EXPORT_SECTION_MAX);
            truncated.push((*name).to_owned());
        }
        sections.insert((*name).to_owned(), found);
    }
    Ok(AccountExport {
        exported_at: Utc::now(),
        account,
        sections,
        truncated,
    })
}

/// Deletes an account: everything personal and every way in goes, what it
/// made in shared workspaces stays under "Deleted account".
///
/// * workspaces it is the only member of are removed, files included;
/// * it leaves every other workspace and team, its assignments and project
///   leads are cleared, and those workspaces' audit logs say that an
///   account was deleted;
/// * its personal memories, AI account, notifications and sessions go;
/// * its name and address are replaced, so the address can register again.
///
/// 409 while it is the only owner of a workspace other people work in: that
/// workspace needs another owner, or to be deleted, first.
pub async fn erase(state: &AppState, user: Uuid) -> Result<(), AppError> {
    let account = repo::users::find(&state.db, user)
        .await
        .or_not_found("user")?;
    let held = repo::platform::sole_owner_of(&state.db, user).await?;
    if !held.is_empty() {
        return Err(AppError::Conflict(format!(
            "this account is the only owner of {}; give it another owner or delete it first",
            held.join(", ")
        )));
    }
    let (alone, shared): (Vec<_>, Vec<_>) = sqlx::query_as::<_, (Uuid, i64)>(
        "SELECT m.workspace_id,
                (SELECT count(*) FROM workspace_members o WHERE o.workspace_id = m.workspace_id)
         FROM workspace_members m WHERE m.user_id = $1",
    )
    .bind(user)
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .partition(|(_, members)| *members == 1);
    for (workspace, _) in &alone {
        workspaces::erase(state, *workspace).await?;
    }
    // Graphs from before workspaces existed are the account's alone; their artifacts are on disk.
    let runs: Vec<Uuid> = sqlx::query_scalar(
        "SELECT r.id FROM runs r JOIN graphs g ON g.id = r.graph_id
         WHERE g.owner_id = $1 AND g.workspace_id IS NULL",
    )
    .bind(user)
    .fetch_all(&state.db)
    .await?;

    let mut tx = state.db.begin().await?;
    for statement in [
        "UPDATE issues SET assignee_id = NULL WHERE assignee_id = $1",
        "UPDATE projects SET lead_id = NULL WHERE lead_id = $1",
        "DELETE FROM team_members WHERE user_id = $1",
        "DELETE FROM workspace_members WHERE user_id = $1",
        "DELETE FROM notifications WHERE user_id = $1",
        "DELETE FROM llm_settings WHERE user_id = $1",
        "DELETE FROM refresh_tokens WHERE user_id = $1",
        "DELETE FROM realtime_tickets WHERE user_id = $1",
        "DELETE FROM password_resets WHERE user_id = $1",
        "DELETE FROM account_events WHERE user_id = $1",
        "DELETE FROM memories WHERE owner_id = $1 AND scope = 'user'",
        "DELETE FROM graphs WHERE owner_id = $1 AND workspace_id IS NULL",
    ] {
        sqlx::query(statement).bind(user).execute(&mut *tx).await?;
    }
    sqlx::query("DELETE FROM workspace_invites WHERE lower(email) = lower($1)")
        .bind(&account.email)
        .execute(&mut *tx)
        .await?;
    let epoch: i32 = sqlx::query_scalar(
        "UPDATE users
         SET email = 'deleted-' || id || '@deleted.invalid', name = $2, password_hash = '!',
             role = 'user', failed_logins = 0, locked_until = NULL,
             suspended_at = COALESCE(suspended_at, now()), suspended_reason = '',
             deleted_at = now(), session_epoch = session_epoch + 1, session_epoch_at = now(),
             updated_at = now()
         WHERE id = $1 RETURNING session_epoch",
    )
    .bind(user)
    .bind(DELETED_NAME)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    state.sessions.raise(user, epoch);
    artifacts::remove_runs(state, &runs).await;
    for (workspace, _) in &shared {
        state.memories.invalidate(*workspace);
        // Written after the name was replaced: the log says what happened without keeping it.
        let left = AuditAction::MemberRemoved;
        let subject = Subject::Text(DELETED_NAME);
        if let Err(err) = repo::audit::record(
            &state.db,
            *workspace,
            user,
            left,
            subject,
            "account deleted",
        )
        .await
        {
            tracing::error!(workspace_id = %workspace, error = %err, "audit entry was not written");
        }
    }
    tracing::info!(user_id = %user, "account deleted");
    Ok(())
}
