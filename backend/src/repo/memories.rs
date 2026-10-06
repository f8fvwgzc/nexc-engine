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
                topic_id: row.try_get("topic_id")?,
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

/// The words that occur in more than `share` of all memories, from the
/// statistics PostgreSQL keeps for the full-text column (refreshed by
/// autovacuum's ANALYZE). Searching for such a word matches a large part of
/// the table and says little; empty until the table has been analysed.
pub async fn common_terms(db: impl PgExecutor<'_>, share: f64) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT u.elem
         FROM pg_stats s,
              unnest(s.most_common_elems::text::text[], s.most_common_elem_freqs) AS u(elem, freq)
         WHERE s.schemaname = current_schema() AND s.tablename = 'memories'
           AND s.attname = 'content_tsv' AND u.elem IS NOT NULL AND u.freq > $1",
    )
    .bind(share)
    .fetch_all(db)
    .await
}

/// How many memories a workspace has.
pub async fn count_for(db: impl PgExecutor<'_>, workspace_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM memories WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_one(db)
        .await
}

/// Who may read which memories of a workspace, as SQL: the memories of the
/// graphs in `$2` and the user-scope memories of `$3`.
macro_rules! readable {
    () => {
        "workspace_id = $1 AND (graph_id = ANY($2) OR (graph_id IS NULL AND owner_id = $3))"
    };
}

/// Which page of a reader's memories to read.
#[derive(Debug, Clone, Copy)]
pub struct Page {
    /// Narrows to one graph plus the reader's own notes.
    pub only_graph: Option<Uuid>,
    pub topic_id: Option<Uuid>,
    pub limit: i64,
    pub offset: i64,
}

/// One page of the memories a reader may see, newest first, read from the
/// database: `only_graph` narrows to one graph plus the reader's own notes.
pub async fn page_readable(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    graphs: &[Uuid],
    reader: Uuid,
    page: Page,
) -> Result<Vec<StoredMemory>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT * FROM memories WHERE ",
        readable!(),
        " AND ($4::uuid IS NULL OR graph_id = $4 OR scope = 'user')
           AND ($7::uuid IS NULL OR topic_id = $7)
         ORDER BY updated_at DESC, id LIMIT $5 OFFSET $6"
    ))
    .bind(workspace_id)
    .bind(graphs)
    .bind(reader)
    .bind(page.only_graph)
    .bind(page.limit)
    .bind(page.offset)
    .bind(page.topic_id)
    .fetch_all(db)
    .await
}

// ---------- topics ----------

/// A stored topic with its centre.
#[derive(Debug, Clone, FromRow)]
pub struct StoredTopic {
    pub id: Uuid,
    pub label: String,
    pub terms: Vec<String>,
    pub centroid: Vec<u8>,
}

/// The topics of a workspace.
pub async fn topics(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<Vec<StoredTopic>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, label, terms, centroid FROM memory_topics WHERE workspace_id = $1
         ORDER BY label, id",
    )
    .bind(workspace_id)
    .fetch_all(db)
    .await
}

/// How many memories of each topic a reader may see.
pub async fn topic_counts(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    graphs: &[Uuid],
    reader: Uuid,
) -> Result<Vec<(Uuid, i64)>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT topic_id, count(*) FROM memories WHERE ",
        readable!(),
        " AND topic_id IS NOT NULL GROUP BY topic_id"
    ))
    .bind(workspace_id)
    .bind(graphs)
    .bind(reader)
    .fetch_all(db)
    .await
}

/// Up to `limit` memories the whole workspace can read (learned in graphs
/// that are not in a private team), picked evenly across the workspace: what
/// topics are found in and named from. Personal notes and private teams'
/// memories never shape a name others see.
pub async fn sample_shared(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    limit: i64,
) -> Result<Vec<(Uuid, String, Vec<u8>)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT m.id, m.content, m.embedding
         FROM memories m
         JOIN graphs g ON g.id = m.graph_id
         LEFT JOIN teams t ON t.id = g.team_id
         WHERE m.workspace_id = $1 AND (t.id IS NULL OR NOT t.private)
         ORDER BY md5(m.id::text) LIMIT $2",
    )
    .bind(workspace_id)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// The next memories of a workspace after `after` (by id) with their
/// embeddings; `only_unplaced` skips those that already have a topic.
pub async fn embeddings_after(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    after: Option<Uuid>,
    only_unplaced: bool,
    limit: i64,
) -> Result<Vec<(Uuid, Vec<u8>)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, embedding FROM memories
         WHERE workspace_id = $1 AND ($2::uuid IS NULL OR id > $2)
           AND (NOT $3 OR topic_id IS NULL)
         ORDER BY id LIMIT $4",
    )
    .bind(workspace_id)
    .bind(after)
    .bind(only_unplaced)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// Replaces the topics of a workspace; memories lose their old topic.
pub async fn replace_topics(
    db: &mut sqlx::PgConnection,
    workspace_id: Uuid,
    topics: &[(Uuid, String, Vec<String>, Vec<u8>)],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM memory_topics WHERE workspace_id = $1")
        .bind(workspace_id)
        .execute(&mut *db)
        .await?;
    for (id, label, terms, centroid) in topics {
        sqlx::query(
            "INSERT INTO memory_topics (id, workspace_id, label, terms, centroid)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(id)
        .bind(workspace_id)
        .bind(label)
        .bind(terms)
        .bind(centroid)
        .execute(&mut *db)
        .await?;
    }
    Ok(())
}

/// Gives a batch of memories their topics.
pub async fn assign_topics(
    db: impl PgExecutor<'_>,
    memory_ids: &[Uuid],
    topic_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE memories m SET topic_id = u.topic
         FROM UNNEST($1::uuid[], $2::uuid[]) AS u(id, topic) WHERE m.id = u.id",
    )
    .bind(memory_ids)
    .bind(topic_ids)
    .execute(db)
    .await?;
    Ok(())
}

/// One memory by id, whoever may read it.
pub async fn find_stored(
    db: impl PgExecutor<'_>,
    id: Uuid,
) -> Result<Option<StoredMemory>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM memories WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await
}

/// The readable memories that match any term of `tsquery` (terms joined by
/// `|`), best full-text rank first, together with the most recently updated
/// ones: the candidates a large workspace is ranked from. Uses the GIN index
/// on `content_tsv` and the `(workspace_id, updated_at)` index.
pub async fn text_candidates(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    graphs: &[Uuid],
    reader: Uuid,
    tsquery: &str,
    matches: i64,
    recent: i64,
) -> Result<Vec<StoredMemory>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT * FROM memories WHERE id IN (
            (SELECT id FROM memories WHERE ",
        readable!(),
        " AND $4 <> '' AND content_tsv @@ to_tsquery('simple', $4)
             ORDER BY ts_rank(content_tsv, to_tsquery('simple', $4)) DESC, updated_at DESC
             LIMIT $5)
            UNION
            (SELECT id FROM memories WHERE ",
        readable!(),
        " ORDER BY updated_at DESC LIMIT $6))"
    ))
    .bind(workspace_id)
    .bind(graphs)
    .bind(reader)
    .bind(tsquery)
    .bind(matches)
    .bind(recent)
    .fetch_all(db)
    .await
}

/// The memories of a graph that match any term of `tsquery`, for consolidating
/// into a graph that has learned more than fits in one read.
pub async fn graph_text_matches(
    db: impl PgExecutor<'_>,
    graph_id: Uuid,
    tsquery: &str,
    limit: i64,
) -> Result<Vec<StoredMemory>, sqlx::Error> {
    sqlx::query_as(
        "SELECT * FROM memories
         WHERE scope = 'graph' AND graph_id = $1 AND $2 <> ''
           AND content_tsv @@ to_tsquery('simple', $2)
         ORDER BY ts_rank(content_tsv, to_tsquery('simple', $2)) DESC LIMIT $3",
    )
    .bind(graph_id)
    .bind(tsquery)
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
