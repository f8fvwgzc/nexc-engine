//! Artifact metadata (files live under `<data>/artifacts/<run_id>/<node_id>/`).

use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

use crate::domain::run::Artifact;

/// An artifact with its on-disk location (relative to the artifacts directory).
#[derive(Debug, Clone, FromRow)]
pub struct StoredArtifact {
    #[sqlx(flatten)]
    pub artifact: ArtifactRow,
    pub storage_path: String,
}

/// Row form of [`Artifact`].
#[derive(Debug, Clone, FromRow)]
pub struct ArtifactRow {
    pub id: Uuid,
    pub run_id: Uuid,
    pub node_id: Uuid,
    pub path: String,
    pub size: i64,
    pub mime: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<ArtifactRow> for Artifact {
    fn from(r: ArtifactRow) -> Self {
        Artifact {
            id: r.id,
            run_id: r.run_id,
            node_id: r.node_id,
            path: r.path,
            size: r.size,
            mime: r.mime,
            created_at: r.created_at,
        }
    }
}

/// Records an artifact (replacing one with the same run, node and path).
pub async fn upsert(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
    node_id: Uuid,
    path: &str,
    size: i64,
    mime: &str,
    storage_path: &str,
) -> Result<Artifact, sqlx::Error> {
    let row: ArtifactRow = sqlx::query_as(
        "INSERT INTO artifacts (id, run_id, node_id, path, size, mime, storage_path)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT (run_id, node_id, path) DO UPDATE
            SET size = EXCLUDED.size, mime = EXCLUDED.mime, storage_path = EXCLUDED.storage_path, created_at = now()
         RETURNING id, run_id, node_id, path, size, mime, created_at",
    )
    .bind(Uuid::now_v7())
    .bind(run_id)
    .bind(node_id)
    .bind(path)
    .bind(size)
    .bind(mime)
    .bind(storage_path)
    .fetch_one(db)
    .await?;
    Ok(row.into())
}

/// Artifacts of a run with storage paths.
pub async fn list_stored(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
) -> Result<Vec<StoredArtifact>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, run_id, node_id, path, size, mime, created_at, storage_path
         FROM artifacts WHERE run_id = $1 ORDER BY created_at, path",
    )
    .bind(run_id)
    .fetch_all(db)
    .await
}

/// Artifacts of one node in one run.
pub async fn list_for_node(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
    node_id: Uuid,
) -> Result<Vec<StoredArtifact>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, run_id, node_id, path, size, mime, created_at, storage_path
         FROM artifacts WHERE run_id = $1 AND node_id = $2 ORDER BY path",
    )
    .bind(run_id)
    .bind(node_id)
    .fetch_all(db)
    .await
}

/// One artifact, only if `owner_id` may work on the graph of its run.
pub async fn find_owned(
    db: impl PgExecutor<'_>,
    owner_id: Uuid,
    id: Uuid,
) -> Result<Option<StoredArtifact>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT a.id, a.run_id, a.node_id, a.path, a.size, a.mime, a.created_at, a.storage_path
         FROM artifacts a JOIN runs r ON r.id = a.run_id JOIN graphs g ON g.id = r.graph_id
         WHERE a.id = $1 AND ",
        graph_access!("g", "$2")
    ))
    .bind(id)
    .bind(owner_id)
    .fetch_optional(db)
    .await
}
