//! Optional approximate search over memory embeddings with pgvector.
//!
//! When the PostgreSQL server offers the `vector` extension, memories get an
//! `embedding_vec vector(256)` column with an HNSW index (cosine distance).
//! It is created at startup rather than by a migration so that a database
//! that gains the extension later (a different server image) picks it up, and
//! one without it keeps working: retrieval then ranks in process only.

use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use crate::kernel::{self, EMBED_DIM, Embedding};
use crate::repo::memories::StoredMemory;

// The column type below spells the dimension out (SQL must be a literal).
const _: () = assert!(EMBED_DIM == 256);

/// Rows converted per statement while backfilling.
const BACKFILL_BATCH: i64 = 500;

/// Creates the extension, the column and the HNSW index when the server has
/// pgvector and they are missing. Returns whether vector search is available.
pub async fn ensure(db: &PgPool) -> Result<bool, sqlx::Error> {
    // One statement, so that two instances starting together cannot interleave;
    // lacking the privilege to create the extension is not an error, just "unavailable".
    sqlx::query(
        "DO $$
         BEGIN
           IF EXISTS (SELECT 1 FROM pg_available_extensions WHERE name = 'vector') THEN
             PERFORM pg_advisory_xact_lock(hashtext('nexc.memory.vectors'));
             CREATE EXTENSION IF NOT EXISTS vector;
             ALTER TABLE memories ADD COLUMN IF NOT EXISTS embedding_vec vector(256);
             CREATE INDEX IF NOT EXISTS memories_embedding_hnsw
               ON memories USING hnsw (embedding_vec vector_cosine_ops);
           END IF;
         EXCEPTION WHEN insufficient_privilege THEN
           RAISE NOTICE 'pgvector is installed but this role may not enable it';
         END $$",
    )
    .execute(db)
    .await?;
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.columns
                        WHERE table_name = 'memories' AND column_name = 'embedding_vec')",
    )
    .fetch_one(db)
    .await
}

/// pgvector's text form of an embedding: `[0.1,0.2,...]`.
fn literal(embedding: &Embedding) -> String {
    let mut out = String::with_capacity(EMBED_DIM * 10);
    out.push('[');
    for (i, value) in embedding.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&value.to_string());
    }
    out.push(']');
    out
}

/// Stores the vector form of one memory's embedding.
pub async fn set(
    db: impl PgExecutor<'_>,
    id: Uuid,
    embedding: &Embedding,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE memories SET embedding_vec = $2::vector WHERE id = $1")
        .bind(id)
        .bind(literal(embedding))
        .execute(db)
        .await?;
    Ok(())
}

/// Gives every memory that lacks one its vector, converted from the stored
/// bytes. Returns how many rows were filled.
pub async fn backfill(db: &PgPool) -> Result<u64, sqlx::Error> {
    let mut filled = 0;
    loop {
        let rows: Vec<(Uuid, Vec<u8>)> = sqlx::query_as(
            "SELECT id, embedding FROM memories WHERE embedding_vec IS NULL LIMIT $1",
        )
        .bind(BACKFILL_BATCH)
        .fetch_all(db)
        .await?;
        if rows.is_empty() {
            return Ok(filled);
        }
        for (id, bytes) in rows {
            // A row whose bytes do not decode would be selected forever; give it a zero vector.
            let embedding = kernel::embedding_from_bytes(&bytes).unwrap_or([0.0; EMBED_DIM]);
            set(db, id, &embedding).await?;
            filled += 1;
        }
    }
}

/// Candidates for a query in a workspace: its nearest neighbours by cosine
/// distance (HNSW) together with its full-text matches, at most `k` of each.
/// The caller filters by visibility and ranks.
pub async fn candidates(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    query: &str,
    query_embedding: &Embedding,
    k: i64,
) -> Result<Vec<StoredMemory>, sqlx::Error> {
    sqlx::query_as(
        "SELECT * FROM memories WHERE id IN (
            (SELECT id FROM memories
             WHERE workspace_id = $1 AND embedding_vec IS NOT NULL
             ORDER BY embedding_vec <=> $2::vector LIMIT $4)
            UNION
            (SELECT id FROM memories
             WHERE workspace_id = $1 AND content_tsv @@ plainto_tsquery('simple', $3)
             ORDER BY updated_at DESC LIMIT $4))",
    )
    .bind(workspace_id)
    .bind(literal(query_embedding))
    .bind(query)
    .bind(k)
    .fetch_all(db)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_are_what_pgvector_parses() {
        let mut e = [0.0f32; EMBED_DIM];
        e[0] = 0.5;
        e[1] = -1.0;
        let text = literal(&e);
        assert!(text.starts_with("[0.5,-1,0,"));
        assert!(text.ends_with(",0]"));
        assert_eq!(text.matches(',').count(), EMBED_DIM - 1);
    }
}
