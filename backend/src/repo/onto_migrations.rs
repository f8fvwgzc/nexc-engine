//! Which graphs have their vocabulary in onto-intelli, and the ontology nexc
//! stored for each before (read only to copy it there once).

use sqlx::types::Json;
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

use crate::domain::ontology::Ontology;

/// A graph still to copy.
#[derive(Debug, Clone, FromRow)]
pub struct Pending {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
}

/// The ontology stored with a graph before onto-intelli held it; `None`
/// when it stored none (`{}`, the starter set).
pub async fn legacy_ontology(
    db: impl PgExecutor<'_>,
    graph_id: Uuid,
) -> Result<Option<Ontology>, sqlx::Error> {
    let stored: Option<Json<Ontology>> =
        sqlx::query_scalar("SELECT ontology FROM graphs WHERE id = $1")
            .bind(graph_id)
            .fetch_optional(db)
            .await?;
    Ok(stored
        .map(|Json(o)| o)
        .filter(|o| !o.node_types.is_empty() || !o.relation_types.is_empty()))
}

/// Serialises everyone who would copy a graph's vocabulary into onto-intelli
/// (its first read, the migration pass, on any replica) until the
/// transaction ends: whoever comes second sees what the first did.
pub async fn lock(db: impl PgExecutor<'_>, graph_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('nexc.onto.vocabulary:' || $1::text, 0))",
    )
    .bind(graph_id)
    .execute(db)
    .await?;
    Ok(())
}

/// Takes the lock of the migration pass on this connection, when no other
/// replica holds it (released by [`unlock_pass`], or when the connection closes).
pub async fn try_lock_pass(db: impl PgExecutor<'_>) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT pg_try_advisory_lock(hashtext('nexc.onto.migrate'))")
        .fetch_one(db)
        .await
}

pub async fn unlock_pass(db: impl PgExecutor<'_>) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_unlock(hashtext('nexc.onto.migrate'))")
        .execute(db)
        .await?;
    Ok(())
}

/// Whether onto-intelli holds the graph's vocabulary.
pub async fn is_migrated(db: impl PgExecutor<'_>, graph_id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM onto_vocabulary_migrations
                         WHERE graph_id = $1 AND migrated_at IS NOT NULL)",
    )
    .bind(graph_id)
    .fetch_one(db)
    .await
}

/// Records that onto-intelli holds the graph's vocabulary.
pub async fn mark_migrated(
    db: impl PgExecutor<'_>,
    graph_id: Uuid,
    workspace_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO onto_vocabulary_migrations (graph_id, workspace_id, migrated_at, attempts)
         VALUES ($1, $2, now(), 1)
         ON CONFLICT (graph_id) DO UPDATE SET
             migrated_at = COALESCE(onto_vocabulary_migrations.migrated_at, now()),
             attempts = onto_vocabulary_migrations.attempts + 1,
             last_error = NULL, updated_at = now()",
    )
    .bind(graph_id)
    .bind(workspace_id)
    .execute(db)
    .await?;
    Ok(())
}

/// Records why copying the graph failed (it is tried again on the next pass).
pub async fn note_failure(
    db: impl PgExecutor<'_>,
    graph_id: Uuid,
    workspace_id: Uuid,
    error: &str,
) -> Result<(), sqlx::Error> {
    let error: String = error.chars().take(500).collect();
    sqlx::query(
        "INSERT INTO onto_vocabulary_migrations (graph_id, workspace_id, attempts, last_error)
         VALUES ($1, $2, 1, $3)
         ON CONFLICT (graph_id) DO UPDATE SET
             attempts = onto_vocabulary_migrations.attempts + 1,
             last_error = EXCLUDED.last_error, updated_at = now()",
    )
    .bind(graph_id)
    .bind(workspace_id)
    .bind(error)
    .execute(db)
    .await?;
    Ok(())
}

/// Graphs of a workspace whose vocabulary is not in onto-intelli yet, after
/// `after` in id order (a pass resumes where it stopped).
pub async fn pending(
    db: impl PgExecutor<'_>,
    after: Option<Uuid>,
    limit: i64,
) -> Result<Vec<Pending>, sqlx::Error> {
    sqlx::query_as(
        "SELECT g.id, g.workspace_id, g.name FROM graphs g
         WHERE g.workspace_id IS NOT NULL AND ($1::uuid IS NULL OR g.id > $1)
           AND NOT EXISTS (SELECT 1 FROM onto_vocabulary_migrations m
                            WHERE m.graph_id = g.id AND m.migrated_at IS NOT NULL)
         ORDER BY g.id LIMIT $2",
    )
    .bind(after)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// Graphs of a workspace whose vocabulary is not in onto-intelli yet.
pub async fn remaining(db: impl PgExecutor<'_>) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM graphs g
         WHERE g.workspace_id IS NOT NULL
           AND NOT EXISTS (SELECT 1 FROM onto_vocabulary_migrations m
                            WHERE m.graph_id = g.id AND m.migrated_at IS NOT NULL)",
    )
    .fetch_one(db)
    .await
}
