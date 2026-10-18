//! API tokens of external agents (`agent_api_tokens`): issued once, kept
//! hashed, looked up by hash on every Agent Firewall call.

use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use crate::domain::agent::{Agent, AgentApiToken};

impl FromRow<'_, PgRow> for AgentApiToken {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(AgentApiToken {
            id: row.try_get("id")?,
            agent_id: row.try_get("agent_id")?,
            prefix: row.try_get("prefix")?,
            name: row.try_get("name")?,
            created_by: row.try_get("created_by")?,
            created_at: row.try_get("created_at")?,
            last_used_at: row.try_get("last_used_at")?,
            revoked_at: row.try_get("revoked_at")?,
        })
    }
}

/// Records a token's hash.
pub async fn insert(
    db: impl PgExecutor<'_>,
    agent_id: Uuid,
    hash: &[u8; 32],
    prefix: &str,
    name: &str,
    created_by: Uuid,
) -> Result<AgentApiToken, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO agent_api_tokens (id, agent_id, token_hash, prefix, name, created_by)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, agent_id, prefix, name, created_by, created_at, last_used_at, revoked_at",
    )
    .bind(Uuid::now_v7())
    .bind(agent_id)
    .bind(hash.as_slice())
    .bind(prefix)
    .bind(name)
    .bind(created_by)
    .fetch_one(db)
    .await
}

/// The tokens of an agent, newest first.
pub async fn list(
    db: impl PgExecutor<'_>,
    agent_id: Uuid,
) -> Result<Vec<AgentApiToken>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, agent_id, prefix, name, created_by, created_at, last_used_at, revoked_at
         FROM agent_api_tokens WHERE agent_id = $1 ORDER BY created_at DESC, id DESC",
    )
    .bind(agent_id)
    .fetch_all(db)
    .await
}

/// Revokes a token of an agent; `None` when there is no such live token.
pub async fn revoke(
    db: impl PgExecutor<'_>,
    agent_id: Uuid,
    id: Uuid,
) -> Result<Option<AgentApiToken>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE agent_api_tokens SET revoked_at = now()
         WHERE agent_id = $1 AND id = $2 AND revoked_at IS NULL
         RETURNING id, agent_id, prefix, name, created_by, created_at, last_used_at, revoked_at",
    )
    .bind(agent_id)
    .bind(id)
    .fetch_optional(db)
    .await
}

/// The agent a presented token belongs to, when the token is live and the
/// agent is an external one of a workspace; the token's last use is noted.
pub async fn authenticate(
    db: impl PgExecutor<'_>,
    hash: &[u8; 32],
) -> Result<Option<Agent>, sqlx::Error> {
    sqlx::query_as(
        "WITH used AS (
             UPDATE agent_api_tokens SET last_used_at = now()
             WHERE token_hash = $1 AND revoked_at IS NULL
             RETURNING agent_id
         )
         SELECT a.* FROM agents a JOIN used ON used.agent_id = a.id
         WHERE a.runtime = 'external' AND a.workspace_id IS NOT NULL",
    )
    .bind(hash.as_slice())
    .fetch_optional(db)
    .await
}
