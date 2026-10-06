//! Documents of a workspace, their passages, and how the workspace embeds them.

use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgConnection, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use super::settings::KeyUpdate;
use crate::domain::knowledge::{Chunk, ChunkKind, Document, DocumentStatus};

impl FromRow<'_, PgRow> for Document {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(Document {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            name: row.try_get("name")?,
            size_bytes: row.try_get("size_bytes")?,
            status: enum_col(row, "status")?,
            error: row.try_get("error")?,
            page_count: row.try_get("page_count")?,
            chunk_count: row.try_get("chunk_count")?,
            uploaded_by: row.try_get("uploaded_by")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

// ---------- documents ----------

/// Registers an uploaded file; `None` when the workspace already has a file
/// with these bytes.
pub async fn insert(
    db: impl PgExecutor<'_>,
    id: Uuid,
    workspace_id: Uuid,
    name: &str,
    size_bytes: i64,
    sha256: &str,
    uploaded_by: Uuid,
) -> Result<Option<Document>, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO documents (id, workspace_id, name, size_bytes, sha256, uploaded_by)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (workspace_id, sha256) DO NOTHING RETURNING *",
    )
    .bind(id)
    .bind(workspace_id)
    .bind(name)
    .bind(size_bytes)
    .bind(sha256)
    .bind(uploaded_by)
    .fetch_optional(db)
    .await
}

pub async fn find(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<Document>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM documents WHERE workspace_id = $1 AND id = $2")
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(db)
        .await
}

pub async fn find_by_hash(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    sha256: &str,
) -> Result<Option<Document>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM documents WHERE workspace_id = $1 AND sha256 = $2")
        .bind(workspace_id)
        .bind(sha256)
        .fetch_optional(db)
        .await
}

/// One page of a workspace's documents, newest first; `q` filters by name.
pub async fn list(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    q: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<Document>, sqlx::Error> {
    sqlx::query_as(
        "SELECT * FROM documents
         WHERE workspace_id = $1 AND ($2::text IS NULL OR name ILIKE '%' || $2 || '%')
         ORDER BY created_at DESC, id LIMIT $3 OFFSET $4",
    )
    .bind(workspace_id)
    .bind(q)
    .bind(limit)
    .bind(offset)
    .fetch_all(db)
    .await
}

pub async fn count(db: impl PgExecutor<'_>, workspace_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM documents WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_one(db)
        .await
}

/// Deletes a document and its passages.
pub async fn delete(db: impl PgExecutor<'_>, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM documents WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// Takes up to `limit` waiting documents for this process: they become
/// `parsing`, and no other instance takes the same ones.
pub async fn claim_pending(
    db: impl PgExecutor<'_>,
    limit: i64,
) -> Result<Vec<Document>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE documents SET status = 'parsing', error = '', updated_at = now()
         WHERE id IN (SELECT id FROM documents WHERE status = 'pending'
                      ORDER BY created_at LIMIT $1 FOR UPDATE SKIP LOCKED)
         RETURNING *",
    )
    .bind(limit)
    .fetch_all(db)
    .await
}

/// Puts documents whose worker went away (no progress for `secs`) back in the queue.
pub async fn requeue_stalled(db: impl PgExecutor<'_>, secs: i64) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE documents SET status = 'pending', updated_at = now()
         WHERE status IN ('parsing', 'embedding')
           AND updated_at < now() - make_interval(secs => $1::double precision)",
    )
    .bind(secs)
    .execute(db)
    .await?;
    Ok(result.rows_affected())
}

pub async fn set_status(
    db: impl PgExecutor<'_>,
    id: Uuid,
    status: DocumentStatus,
    error: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE documents SET status = $2, error = $3, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(status.as_str())
        .bind(error)
        .execute(db)
        .await?;
    Ok(())
}

/// Records progress, so a long embedding is not taken for a stalled one.
pub async fn touch(db: impl PgExecutor<'_>, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE documents SET updated_at = now() WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

// ---------- passages ----------

/// Rows written per statement.
const INSERT_BATCH: usize = 500;

/// Replaces the passages of a document and records how many it has.
pub async fn replace_chunks(
    db: &mut PgConnection,
    document_id: Uuid,
    workspace_id: Uuid,
    page_count: Option<i32>,
    chunks: &[Chunk],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM document_chunks WHERE document_id = $1")
        .bind(document_id)
        .execute(&mut *db)
        .await?;
    for (batch_index, batch) in chunks.chunks(INSERT_BATCH).enumerate() {
        let start = batch_index * INSERT_BATCH;
        let ids: Vec<Uuid> = batch.iter().map(|_| Uuid::now_v7()).collect();
        let ordinals: Vec<i32> = (0..batch.len())
            .map(|i| i32::try_from(start + i).unwrap_or(i32::MAX))
            .collect();
        let pages: Vec<Option<i32>> = batch.iter().map(|c| c.page).collect();
        let sections: Vec<&str> = batch.iter().map(|c| c.section_path.as_str()).collect();
        let kinds: Vec<&str> = batch.iter().map(|c| c.kind.as_str()).collect();
        let contents: Vec<&str> = batch.iter().map(|c| c.content.as_str()).collect();
        sqlx::query(
            "INSERT INTO document_chunks
                (id, document_id, workspace_id, ordinal, page, section_path, kind, content)
             SELECT u.id, $2, $3, u.ordinal, u.page, u.section_path, u.kind, u.content
             FROM UNNEST($1::uuid[], $4::int[], $5::int[], $6::text[], $7::text[], $8::text[])
                  AS u(id, ordinal, page, section_path, kind, content)",
        )
        .bind(&ids)
        .bind(document_id)
        .bind(workspace_id)
        .bind(&ordinals)
        .bind(&pages)
        .bind(&sections)
        .bind(&kinds)
        .bind(&contents)
        .execute(&mut *db)
        .await?;
    }
    sqlx::query(
        "UPDATE documents SET status = 'embedding', page_count = $2, chunk_count = $3,
                updated_at = now() WHERE id = $1",
    )
    .bind(document_id)
    .bind(page_count)
    .bind(i32::try_from(chunks.len()).unwrap_or(i32::MAX))
    .execute(&mut *db)
    .await?;
    Ok(())
}

/// A passage waiting for its vector.
#[derive(Debug, FromRow)]
pub struct Unembedded {
    pub id: Uuid,
    pub section_path: String,
    pub content: String,
}

/// The next passages of a document that have no vector of `model` yet.
pub async fn unembedded(
    db: impl PgExecutor<'_>,
    document_id: Uuid,
    model: &str,
    limit: i64,
) -> Result<Vec<Unembedded>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, section_path, content FROM document_chunks
         WHERE document_id = $1 AND embedding_model IS DISTINCT FROM $2
         ORDER BY ordinal LIMIT $3",
    )
    .bind(document_id)
    .bind(model)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// Stores the vectors of a batch of passages.
pub async fn set_embeddings(
    db: impl PgExecutor<'_>,
    ids: &[Uuid],
    vectors: &[Vec<u8>],
    model: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE document_chunks c SET embedding = u.embedding, embedding_model = $3
         FROM UNNEST($1::uuid[], $2::bytea[]) AS u(id, embedding) WHERE c.id = u.id",
    )
    .bind(ids)
    .bind(vectors)
    .bind(model)
    .execute(db)
    .await?;
    Ok(())
}

/// A passage that matched a search by keywords, with what ranking needs.
#[derive(Debug)]
pub struct Candidate {
    pub chunk_id: Uuid,
    pub document_id: Uuid,
    pub document_name: String,
    pub page: Option<i32>,
    pub section_path: String,
    pub kind: ChunkKind,
    pub content: String,
    pub embedding: Option<Vec<u8>>,
    pub embedding_model: Option<String>,
    pub topic_id: Option<Uuid>,
}

impl FromRow<'_, PgRow> for Candidate {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(Candidate {
            chunk_id: row.try_get("id")?,
            document_id: row.try_get("document_id")?,
            document_name: row.try_get("document_name")?,
            page: row.try_get("page")?,
            section_path: row.try_get("section_path")?,
            kind: enum_col(row, "kind")?,
            content: row.try_get("content")?,
            embedding: row.try_get("embedding")?,
            embedding_model: row.try_get("embedding_model")?,
            topic_id: row.try_get("topic_id")?,
        })
    }
}

/// The passages of a workspace's ready documents that match any term of
/// `tsquery`, best full-text rank first. Uses the GIN index; the number of
/// rows read is bounded by how rare the terms are, not by the corpus.
pub async fn keyword_candidates(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    tsquery: &str,
    topic_id: Option<Uuid>,
    limit: i64,
) -> Result<Vec<Candidate>, sqlx::Error> {
    sqlx::query_as(
        "SELECT c.id, c.document_id, d.name AS document_name, c.page, c.section_path, c.kind,
                c.content, c.embedding, c.embedding_model, c.topic_id
         FROM document_chunks c JOIN documents d ON d.id = c.document_id
         WHERE c.workspace_id = $1 AND d.status = 'ready' AND $2 <> ''
           AND c.content_tsv @@ to_tsquery('simple', $2)
           AND ($4::uuid IS NULL OR c.topic_id = $4)
         ORDER BY ts_rank(c.content_tsv, to_tsquery('simple', $2)) DESC, c.id LIMIT $3",
    )
    .bind(workspace_id)
    .bind(tsquery)
    .bind(limit)
    .bind(topic_id)
    .fetch_all(db)
    .await
}

/// The words in more than `share` of all passages (see `repo::memories::common_terms`).
pub async fn common_terms(db: impl PgExecutor<'_>, share: f64) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT u.elem
         FROM pg_stats s,
              unnest(s.most_common_elems::text::text[], s.most_common_elem_freqs) AS u(elem, freq)
         WHERE s.schemaname = current_schema() AND s.tablename = 'document_chunks'
           AND s.attname = 'content_tsv' AND u.elem IS NOT NULL AND u.freq > $1",
    )
    .bind(share)
    .fetch_all(db)
    .await
}

// ---------- settings ----------

/// A workspace's stored knowledge settings.
#[derive(Debug, Clone, FromRow)]
pub struct StoredSettings {
    pub embed_base_url: Option<String>,
    pub embed_model: Option<String>,
    pub embed_dims: Option<i32>,
    pub api_key_enc: Option<Vec<u8>>,
    pub key_hint: Option<String>,
    pub passages: i32,
    pub budget_chars: i32,
    pub use_in_nodes: bool,
    pub use_in_plan: bool,
}

pub async fn settings(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<Option<StoredSettings>, sqlx::Error> {
    sqlx::query_as(
        "SELECT embed_base_url, embed_model, embed_dims, api_key_enc, key_hint, passages,
                budget_chars, use_in_nodes, use_in_plan
         FROM workspace_knowledge_settings WHERE workspace_id = $1",
    )
    .bind(workspace_id)
    .fetch_optional(db)
    .await
}

/// What `PUT` of the settings writes.
#[derive(Debug)]
pub struct SettingsUpdate<'a> {
    pub embed_base_url: Option<&'a str>,
    pub embed_model: Option<&'a str>,
    pub embed_dims: Option<i32>,
    pub key: KeyUpdate,
    pub passages: i32,
    pub budget_chars: i32,
    pub use_in_nodes: bool,
    pub use_in_plan: bool,
}

pub async fn upsert_settings(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    updated_by: Uuid,
    u: SettingsUpdate<'_>,
) -> Result<(), sqlx::Error> {
    let (keep, enc, hint) = u.key.parts();
    sqlx::query(
        "INSERT INTO workspace_knowledge_settings
            (workspace_id, embed_base_url, embed_model, embed_dims, api_key_enc, key_hint,
             passages, budget_chars, use_in_nodes, use_in_plan, updated_by)
         VALUES ($1, $2, $3, $4, $5, $6, $8, $9, $10, $11, $12)
         ON CONFLICT (workspace_id) DO UPDATE SET
            embed_base_url = EXCLUDED.embed_base_url,
            embed_model = EXCLUDED.embed_model,
            embed_dims = EXCLUDED.embed_dims,
            api_key_enc = CASE WHEN $7 THEN workspace_knowledge_settings.api_key_enc
                               ELSE EXCLUDED.api_key_enc END,
            key_hint = CASE WHEN $7 THEN workspace_knowledge_settings.key_hint
                            ELSE EXCLUDED.key_hint END,
            passages = EXCLUDED.passages,
            budget_chars = EXCLUDED.budget_chars,
            use_in_nodes = EXCLUDED.use_in_nodes,
            use_in_plan = EXCLUDED.use_in_plan,
            updated_by = EXCLUDED.updated_by,
            updated_at = now()",
    )
    .bind(workspace_id)
    .bind(u.embed_base_url)
    .bind(u.embed_model)
    .bind(u.embed_dims)
    .bind(enc)
    .bind(hint)
    .bind(keep)
    .bind(u.passages)
    .bind(u.budget_chars)
    .bind(u.use_in_nodes)
    .bind(u.use_in_plan)
    .bind(updated_by)
    .execute(db)
    .await?;
    Ok(())
}

/// Documents whose passages were embedded with another model than `model`
/// go back in the queue to be embedded again.
pub async fn requeue_for_model(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    model: &str,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE documents d SET status = 'pending', updated_at = now()
         WHERE d.workspace_id = $1 AND d.status = 'ready'
           AND EXISTS (SELECT 1 FROM document_chunks c
                       WHERE c.document_id = d.id AND c.embedding_model IS DISTINCT FROM $2)",
    )
    .bind(workspace_id)
    .bind(model)
    .execute(db)
    .await?;
    Ok(result.rows_affected())
}

// ---------- topics ----------

/// A stored topic with its centre.
#[derive(Debug, Clone, FromRow)]
pub struct StoredTopic {
    pub id: Uuid,
    pub label: String,
    pub terms: Vec<String>,
    pub centroid: Vec<u8>,
    pub embedding_model: String,
    pub chunk_count: i32,
}

/// The topics of a workspace, largest first.
pub async fn topics(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<Vec<StoredTopic>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, label, terms, centroid, embedding_model, chunk_count
         FROM knowledge_topics WHERE workspace_id = $1 ORDER BY chunk_count DESC, label",
    )
    .bind(workspace_id)
    .fetch_all(db)
    .await
}

/// A passage as the topic model reads it.
#[derive(Debug, FromRow)]
pub struct EmbeddedChunk {
    pub id: Uuid,
    pub content: String,
    pub embedding: Vec<u8>,
}

/// Up to `limit` passages of a workspace embedded with `model`, picked
/// evenly across the corpus (by id, which is random within a millisecond)
/// rather than the newest, so a sample represents the whole.
pub async fn sample_embedded(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    model: &str,
    limit: i64,
) -> Result<Vec<EmbeddedChunk>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, content, embedding FROM document_chunks
         WHERE workspace_id = $1 AND embedding_model = $2 AND embedding IS NOT NULL
         ORDER BY md5(id::text) LIMIT $3",
    )
    .bind(workspace_id)
    .bind(model)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// The next passages (after `after`, by id) embedded with `model`, with
/// their vectors, for assigning every passage to its nearest topic.
pub async fn embedded_after(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    model: &str,
    after: Option<Uuid>,
    limit: i64,
) -> Result<Vec<(Uuid, Vec<u8>)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, embedding FROM document_chunks
         WHERE workspace_id = $1 AND embedding_model = $2 AND embedding IS NOT NULL
           AND ($3::uuid IS NULL OR id > $3)
         ORDER BY id LIMIT $4",
    )
    .bind(workspace_id)
    .bind(model)
    .bind(after)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// A topic to store.
#[derive(Debug)]
pub struct NewTopic {
    pub id: Uuid,
    pub label: String,
    pub terms: Vec<String>,
    pub centroid: Vec<u8>,
}

/// Replaces the topics of a workspace. Passages lose their old topic (the
/// foreign key clears it) and are assigned again by the caller.
pub async fn replace_topics(
    db: &mut PgConnection,
    workspace_id: Uuid,
    model: &str,
    topics: &[NewTopic],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM knowledge_topics WHERE workspace_id = $1")
        .bind(workspace_id)
        .execute(&mut *db)
        .await?;
    for topic in topics {
        sqlx::query(
            "INSERT INTO knowledge_topics (id, workspace_id, label, terms, centroid, embedding_model)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(topic.id)
        .bind(workspace_id)
        .bind(&topic.label)
        .bind(&topic.terms)
        .bind(&topic.centroid)
        .bind(model)
        .execute(&mut *db)
        .await?;
    }
    Ok(())
}

/// Gives a batch of passages their topics.
pub async fn assign_topics(
    db: impl PgExecutor<'_>,
    chunk_ids: &[Uuid],
    topic_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE document_chunks c SET topic_id = u.topic
         FROM UNNEST($1::uuid[], $2::uuid[]) AS u(id, topic) WHERE c.id = u.id",
    )
    .bind(chunk_ids)
    .bind(topic_ids)
    .execute(db)
    .await?;
    Ok(())
}

/// Sets each topic's passage count from the passages that carry it.
pub async fn recount_topics(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE knowledge_topics t SET chunk_count =
            (SELECT count(*) FROM document_chunks c WHERE c.topic_id = t.id)
         WHERE t.workspace_id = $1",
    )
    .bind(workspace_id)
    .execute(db)
    .await?;
    Ok(())
}
