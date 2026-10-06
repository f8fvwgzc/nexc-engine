//! Approximate nearest-neighbour search over passage embeddings with pgvector.
//!
//! Workspaces choose their embedding model, so vectors of several sizes live
//! in one untyped `vector` column. Each size in use gets its own partial HNSW
//! index over the column cast to that size; a search names the size, so it
//! uses exactly that index. Created at startup and on first use rather than
//! by a migration, so a database without the extension keeps working: search
//! is then gated by keywords only.

use std::sync::atomic::{AtomicBool, Ordering};

use sqlx::{AssertSqlSafe, PgPool};
use uuid::Uuid;

use super::knowledge::Candidate;

/// pgvector indexes `vector` up to this many dimensions.
pub const MAX_INDEXED_DIMS: usize = 2_000;
const BACKFILL_BATCH: i64 = 500;

/// What this process knows about vector search in its database.
#[derive(Debug, Default)]
pub struct Support {
    available: AtomicBool,
    /// pgvector 0.8+: keep scanning the index until enough rows pass the filter.
    iterative: AtomicBool,
}

impl Support {
    /// Whether passages can be searched by vector.
    pub fn available(&self) -> bool {
        self.available.load(Ordering::Relaxed)
    }
}

/// Creates the extension and the column when the server has pgvector.
/// Returns whether vector search is available.
pub async fn ensure(db: &PgPool, support: &Support) -> Result<bool, sqlx::Error> {
    sqlx::query(
        "DO $$
         BEGIN
           IF EXISTS (SELECT 1 FROM pg_available_extensions WHERE name = 'vector') THEN
             PERFORM pg_advisory_xact_lock(hashtext('nexc.knowledge.vectors'));
             CREATE EXTENSION IF NOT EXISTS vector;
             ALTER TABLE document_chunks ADD COLUMN IF NOT EXISTS embedding_vec vector;
           END IF;
         EXCEPTION WHEN insufficient_privilege THEN
           RAISE NOTICE 'pgvector is installed but this role may not enable it';
         END $$",
    )
    .execute(db)
    .await?;
    let column: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.columns
                        WHERE table_schema = current_schema()
                          AND table_name = 'document_chunks' AND column_name = 'embedding_vec')",
    )
    .fetch_one(db)
    .await?;
    if column {
        let version: Option<String> =
            sqlx::query_scalar("SELECT extversion FROM pg_extension WHERE extname = 'vector'")
                .fetch_optional(db)
                .await?;
        let iterative = version.as_deref().is_some_and(|v| {
            let mut parts = v.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
            (parts.next().unwrap_or(0), parts.next().unwrap_or(0)) >= (0, 8)
        });
        support.iterative.store(iterative, Ordering::Relaxed);
    }
    support.available.store(column, Ordering::Relaxed);
    Ok(column)
}

fn indexable(dims: usize) -> bool {
    (2..=MAX_INDEXED_DIMS).contains(&dims)
}

/// Creates the HNSW index for vectors of `dims` dimensions if it is missing.
/// `dims` is a checked integer, the only thing spliced into the statement.
pub async fn ensure_index(db: &PgPool, support: &Support, dims: usize) -> Result<(), sqlx::Error> {
    if !support.available() || !indexable(dims) {
        return Ok(());
    }
    let sql = format!(
        "CREATE INDEX IF NOT EXISTS document_chunks_vec_{dims}
           ON document_chunks USING hnsw ((embedding_vec::vector({dims})) vector_cosine_ops)
           WHERE vector_dims(embedding_vec) = {dims}"
    );
    // Built without parallel workers: they share memory through /dev/shm, which a container
    // gives 64 MB by default, and a build on a table that already has rows then fails.
    let mut tx = db.begin().await?;
    sqlx::query("SET LOCAL max_parallel_maintenance_workers = 0")
        .execute(&mut *tx)
        .await?;
    sqlx::query(AssertSqlSafe(sql)).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// pgvector's text form of a vector: `[0.1,0.2,...]`.
pub fn literal(vector: &[f32]) -> String {
    let mut out = String::with_capacity(vector.len() * 10 + 2);
    out.push('[');
    for (i, value) in vector.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&value.to_string());
    }
    out.push(']');
    out
}

/// Stores the vector form of a batch of passages' embeddings.
pub async fn set(
    db: &PgPool,
    support: &Support,
    ids: &[Uuid],
    vectors: &[Vec<f32>],
) -> Result<(), sqlx::Error> {
    if !support.available() || ids.is_empty() {
        return Ok(());
    }
    let literals: Vec<String> = vectors.iter().map(|v| literal(v)).collect();
    sqlx::query(
        "UPDATE document_chunks c SET embedding_vec = u.v::vector
         FROM UNNEST($1::uuid[], $2::text[]) AS u(id, v) WHERE c.id = u.id",
    )
    .bind(ids)
    .bind(&literals)
    .execute(db)
    .await?;
    if let Some(first) = vectors.first() {
        ensure_index(db, support, first.len()).await?;
    }
    Ok(())
}

/// Gives every embedded passage that lacks one its vector form (a database
/// that gained pgvector after documents were ingested). Returns how many.
pub async fn backfill(db: &PgPool, support: &Support) -> Result<u64, sqlx::Error> {
    if !support.available() {
        return Ok(0);
    }
    let mut filled = 0;
    loop {
        let rows: Vec<(Uuid, Vec<u8>)> = sqlx::query_as(
            "SELECT id, embedding FROM document_chunks
             WHERE embedding IS NOT NULL AND embedding_vec IS NULL LIMIT $1",
        )
        .bind(BACKFILL_BATCH)
        .fetch_all(db)
        .await?;
        if rows.is_empty() {
            return Ok(filled);
        }
        // One statement per vector size, so each batch names one index.
        let mut by_dims: std::collections::BTreeMap<usize, (Vec<Uuid>, Vec<Vec<f32>>)> =
            std::collections::BTreeMap::new();
        for (id, bytes) in rows {
            let vector: Vec<f32> = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect();
            // Bytes that are not whole floats would be selected forever; give them one zero.
            let vector = if vector.is_empty() { vec![0.0] } else { vector };
            let entry = by_dims.entry(vector.len()).or_default();
            entry.0.push(id);
            entry.1.push(vector);
            filled += 1;
        }
        for (ids, vectors) in by_dims.values() {
            set(db, support, ids, vectors).await?;
        }
    }
}

/// The passages of a workspace's ready documents nearest to `query` among
/// those embedded with `model`, nearest first. Empty when vector search is
/// unavailable or the vector size cannot be indexed.
pub async fn nearest(
    db: &PgPool,
    support: &Support,
    workspace_id: Uuid,
    model: &str,
    query: &[f32],
    limit: i64,
) -> Result<Vec<Candidate>, sqlx::Error> {
    let dims = query.len();
    if !support.available() || !indexable(dims) {
        return Ok(Vec::new());
    }
    let mut tx = db.begin().await?;
    if support.iterative.load(Ordering::Relaxed) {
        // Passages of other workspaces are filtered out after the index scan; without this a
        // small workspace in a large database would get fewer rows than it asked for.
        sqlx::query("SET LOCAL hnsw.iterative_scan = relaxed_order")
            .execute(&mut *tx)
            .await?;
    }
    let sql = format!(
        "SELECT c.id, c.document_id, d.name AS document_name, c.page, c.section_path, c.kind,
                c.content, c.embedding, c.embedding_model, c.topic_id
         FROM document_chunks c JOIN documents d ON d.id = c.document_id
         WHERE c.workspace_id = $1 AND d.status = 'ready' AND c.embedding_model = $2
           AND vector_dims(c.embedding_vec) = {dims}
         ORDER BY c.embedding_vec::vector({dims}) <=> $3::vector({dims}) LIMIT $4"
    );
    let rows = sqlx::query_as(AssertSqlSafe(sql))
        .bind(workspace_id)
        .bind(model)
        .bind(literal(query))
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(rows)
}
