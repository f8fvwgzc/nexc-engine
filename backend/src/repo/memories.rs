//! Long-term memories with their embeddings (256 × f32 LE in a BYTEA).

use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::memory::{Memory, MemoryKind, MemoryScope};
use crate::kernel::{self, Embedding};

/// A memory with its embedding, as needed for ranking and consolidation.
#[derive(Debug, Clone)]
pub struct StoredMemory {
    pub memory: Memory,
    /// Who the memory was learned for (the author of the run that produced it).
    pub owner_id: Uuid,
    pub embedding: Embedding,
}

impl FromRow<'_, PgRow> for StoredMemory {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let bytes: Vec<u8> = row.try_get("embedding")?;
        let embedding =
            kernel::embedding_from_bytes(&bytes).ok_or_else(|| sqlx::Error::ColumnDecode {
                index: "embedding".into(),
                source: "embedding has the wrong length".into(),
            })?;
        Ok(StoredMemory {
            memory: Memory {
                id: row.try_get("id")?,
                scope: enum_col(row, "scope")?,
                graph_id: row.try_get("graph_id")?,
                node_id: row.try_get("node_id")?,
                kind: enum_col(row, "kind")?,
                content: row.try_get("content")?,
                importance: row.try_get("importance")?,
                access_count: row.try_get("access_count")?,
                score: None,
                created_at: row.try_get("created_at")?,
                updated_at: row.try_get("updated_at")?,
            },
            owner_id: row.try_get("owner_id")?,
            embedding,
        })
    }
}

/// A memory to insert.
#[derive(Debug, Clone)]
pub struct NewMemory<'a> {
    pub owner_id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub scope: MemoryScope,
    pub graph_id: Option<Uuid>,
    pub node_id: Option<Uuid>,
    pub kind: MemoryKind,
    pub content: &'a str,
    pub embedding: &'a Embedding,
    pub importance: f64,
}

/// Inserts a memory.
pub async fn insert(db: impl PgExecutor<'_>, m: &NewMemory<'_>) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO memories (id, owner_id, scope, graph_id, node_id, kind, content, embedding, importance,
                               workspace_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(m.owner_id)
    .bind(m.scope.as_str())
    .bind(m.graph_id)
    .bind(m.node_id)
    .bind(m.kind.as_str())
    .bind(m.content)
    .bind(kernel::embedding_to_bytes(m.embedding))
    .bind(m.importance)
    .bind(m.workspace_id)
    .fetch_one(db)
    .await
}

/// Memories visible for a graph (user scope plus that graph), or all of the
/// owner's memories when `graph_id` is `None`, newest first.
pub async fn list(
    db: impl PgExecutor<'_>,
    owner_id: Uuid,
    graph_id: Option<Uuid>,
    limit: i64,
) -> Result<Vec<StoredMemory>, sqlx::Error> {
    sqlx::query_as(
        "SELECT * FROM memories
         WHERE owner_id = $1 AND ($2::uuid IS NULL OR graph_id = $2 OR scope = 'user')
         ORDER BY updated_at DESC LIMIT $3",
    )
    .bind(owner_id)
    .bind(graph_id)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// Every memory of a workspace (the `limit` most recently updated), for the
/// in-process index.
pub async fn all_for(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    limit: i64,
) -> Result<Vec<StoredMemory>, sqlx::Error> {
    sqlx::query_as(
        "SELECT * FROM memories WHERE workspace_id = $1 ORDER BY updated_at DESC LIMIT $2",
    )
    .bind(workspace_id)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// What a graph has learned so far, whoever ran it (for consolidation).
pub async fn of_graph(
    db: impl PgExecutor<'_>,
    graph_id: Uuid,
) -> Result<Vec<StoredMemory>, sqlx::Error> {
    sqlx::query_as(
        "SELECT * FROM memories WHERE scope = 'graph' AND graph_id = $1
         ORDER BY updated_at DESC LIMIT 1000",
    )
    .bind(graph_id)
    .fetch_all(db)
    .await
}

/// Replaces the content of a memory (UPDATE consolidation).
pub async fn replace_content(
    db: impl PgExecutor<'_>,
    id: Uuid,
    content: &str,
    embedding: &Embedding,
    importance: f64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE memories SET content = $2, embedding = $3, importance = GREATEST(importance, $4), updated_at = now()
         WHERE id = $1",
    )
    .bind(id)
    .bind(content)
    .bind(kernel::embedding_to_bytes(embedding))
    .bind(importance)
    .execute(db)
    .await?;
    Ok(())
}

/// Reinforces a memory (NOOP consolidation): raise importance, count an access.
pub async fn reinforce(
    db: impl PgExecutor<'_>,
    id: Uuid,
    importance: f64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE memories SET importance = GREATEST(importance, $2), access_count = access_count + 1,
                last_accessed_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(importance)
    .execute(db)
    .await?;
    Ok(())
}

/// Counts a retrieval of each memory.
pub async fn record_access(db: impl PgExecutor<'_>, ids: &[Uuid]) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE memories SET access_count = access_count + 1, last_accessed_at = now() WHERE id = ANY($1)")
        .bind(ids)
        .execute(db)
        .await?;
    Ok(())
}

/// Who wrote a memory and where it lives: `(author, workspace, graph)`.
pub async fn provenance(
    db: impl PgExecutor<'_>,
    id: Uuid,
) -> Result<Option<(Uuid, Option<Uuid>, Option<Uuid>)>, sqlx::Error> {
    sqlx::query_as("SELECT owner_id, workspace_id, graph_id FROM memories WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await
}

/// Deletes a memory. Returns false if absent.
pub async fn delete(db: impl PgExecutor<'_>, id: Uuid) -> Result<bool, sqlx::Error> {
    let done = sqlx::query("DELETE FROM memories WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(done.rows_affected() == 1)
}

/// Gives memories that predate workspaces the workspace of their graph, or
/// of their author when they are not tied to a graph.
pub async fn adopt_orphans(db: &sqlx::PgPool) -> Result<u64, sqlx::Error> {
    let by_graph = sqlx::query(
        "UPDATE memories m SET workspace_id = g.workspace_id FROM graphs g
         WHERE m.workspace_id IS NULL AND m.graph_id = g.id AND g.workspace_id IS NOT NULL",
    )
    .execute(db)
    .await?;
    let by_author = sqlx::query(
        "UPDATE memories m SET workspace_id = (
            SELECT w.workspace_id FROM workspace_members w
            WHERE w.user_id = m.owner_id AND w.role <> 'guest'
            ORDER BY w.created_at, w.workspace_id LIMIT 1)
         WHERE m.workspace_id IS NULL AND m.graph_id IS NULL",
    )
    .execute(db)
    .await?;
    Ok(by_graph.rows_affected() + by_author.rows_affected())
}
