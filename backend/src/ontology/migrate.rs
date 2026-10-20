//! Moving graph vocabularies into onto-intelli: `nexc onto migrate`, also
//! run in the background when the backend starts (`NEXC_ONTO_MIGRATE_ON_START`).
//!
//! Each graph's stored ontology (`graphs.ontology`, the starter set when it
//! stored none) is copied with `InitGraph` and `SetVocabulary`, as the
//! backend's service principal, and the graph is recorded as migrated in
//! `onto_vocabulary_migrations`. The pass is idempotent (a migrated graph is
//! never written again, so edits made in onto-intelli since are kept) and
//! resumable (a failed graph is recorded and tried on the next pass). A graph
//! read before the pass reached it is initialised the same way on its first
//! read ([`ensure`]). After the pass, `graphs.ontology` is not read or
//! written any more; the column is dropped one release later.
//!
//! Whoever copies a graph holds its lock (`repo::lock`, a transaction-scoped
//! advisory lock) and looks again under it: a graph migrated meanwhile (its
//! first read while the pass worked through a batch, another replica) is
//! left alone, and a graph onto-intelli already holds is only recorded (it
//! was written by a copy that stopped before recording it, or edited since).
//! Only one replica runs the start-up pass at a time.

use std::time::Duration;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use sqlx::PgConnection;

use super::vocabulary::{GraphRef, graph_key};
use super::{OntoError, OntologyBackend, Principal};
use crate::app::AppState;
use crate::app::caches::Key;
use crate::domain::ontology::Ontology;
use crate::repo::onto_migrations as repo;

/// Graphs read per batch.
const BATCH: i64 = 100;
/// How long the start-up pass waits before it tries failed graphs again.
const RETRY: Duration = Duration::from_secs(60);

/// What a pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Graphs copied into onto-intelli now.
    pub migrated: usize,
    /// Graphs left as they were: migrated since the pass read its batch, or
    /// held by onto-intelli already (then only recorded).
    pub already: usize,
    /// Graphs that could not be copied (tried again on the next pass).
    pub failed: usize,
    /// Graphs still to copy after the pass.
    pub remaining: i64,
}

/// Why a graph could not be initialised.
#[derive(Debug, thiserror::Error)]
pub enum EnsureError {
    #[error(transparent)]
    Onto(#[from] OntoError),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// The vocabulary a graph starts with in onto-intelli: what nexc stored for
/// it, when it has not been migrated yet and stored one, else the starter.
async fn initial(db: &mut PgConnection, graph: GraphRef<'_>) -> Result<Ontology, sqlx::Error> {
    let stored = if repo::is_migrated(&mut *db, graph.id).await? {
        None
    } else {
        repo::legacy_ontology(&mut *db, graph.id).await?
    };
    let mut vocabulary = stored.unwrap_or_else(Ontology::starter);
    vocabulary.normalize();
    Ok(vocabulary)
}

/// Writes `vocabulary` as the graph's in onto-intelli: the graph entity and
/// its types, then exactly these types.
async fn write(
    backend: &dyn OntologyBackend,
    service: &Principal,
    graph: GraphRef<'_>,
    vocabulary: &Ontology,
) -> Result<Ontology, OntoError> {
    let key = graph_key(graph.id);
    let created = backend
        .init_graph(service, &key, graph.name, Some(vocabulary))
        .await?;
    if same_types(&created, vocabulary) {
        return Ok(created);
    }
    backend.set_vocabulary(service, &key, vocabulary).await
}

fn same_types(a: &Ontology, b: &Ontology) -> bool {
    let keys = |o: &Ontology| {
        let mut n: Vec<String> = o.node_types.iter().map(|t| t.key.clone()).collect();
        let mut r: Vec<String> = o.relation_types.iter().map(|t| t.key.clone()).collect();
        n.sort();
        r.sort();
        (n, r)
    };
    keys(a) == keys(b)
}

/// Initialises a graph onto-intelli does not hold a vocabulary for (the
/// backend's own principal did not find it, or found only the graph's
/// projected entity, without a type): copies what nexc stored for it, or the
/// starter set, and records it. Under the graph's lock it asks once more, so
/// a graph someone else copied meanwhile is read, not written.
pub async fn ensure(
    state: &AppState,
    backend: &dyn OntologyBackend,
    graph: GraphRef<'_>,
) -> Result<Ontology, EnsureError> {
    let Some(workspace_id) = graph.workspace_id else {
        return Ok(Ontology::starter());
    };
    let service = Principal::service(&state.settings.onto_workspace_prefix, workspace_id);
    let mut tx = state.db.begin().await?;
    repo::lock(&mut *tx, graph.id).await?;
    let vocabulary = match backend.vocabulary(&service, &graph_key(graph.id)).await {
        Ok(found) if !found.is_empty() => found,
        Ok(_) | Err(OntoError::NotFound(_)) => {
            let vocabulary = initial(&mut tx, graph).await?;
            write(backend, &service, graph, &vocabulary).await?
        }
        Err(err) => return Err(err.into()),
    };
    repo::mark_migrated(&mut *tx, graph.id, workspace_id).await?;
    tx.commit().await?;
    Ok(vocabulary)
}

/// One pass over every graph not migrated yet.
pub async fn run(state: &AppState) -> anyhow::Result<Report> {
    let backend = state.ontology.clone().ok_or_else(|| {
        anyhow::anyhow!("onto-intelli is not configured (NEXC_ONTO_MODE=off): nothing to migrate")
    })?;
    let mut report = Report::default();
    let mut after = None;
    loop {
        let batch = repo::pending(&state.db, after, BATCH).await?;
        let Some(last) = batch.last() else {
            break;
        };
        after = Some(last.id);
        for pending in &batch {
            let graph = GraphRef {
                id: pending.id,
                workspace_id: Some(pending.workspace_id),
                name: &pending.name,
            };
            match copy(state, backend.as_ref(), graph).await {
                Ok(true) => report.migrated += 1,
                Ok(false) => report.already += 1,
                Err(err) => {
                    report.failed += 1;
                    tracing::warn!(graph = %pending.id, error = %err, "graph vocabulary not copied to onto-intelli");
                    repo::note_failure(
                        &state.db,
                        pending.id,
                        pending.workspace_id,
                        &err.to_string(),
                    )
                    .await?;
                }
            }
        }
    }
    report.remaining = repo::remaining(&state.db).await?;
    Ok(report)
}

/// Copies one graph of a pass, under its lock; false when nothing was
/// written: the graph was migrated since the pass read its batch, or
/// onto-intelli holds its vocabulary already (then it is only recorded).
pub async fn copy(
    state: &AppState,
    backend: &dyn OntologyBackend,
    graph: GraphRef<'_>,
) -> Result<bool, EnsureError> {
    let Some(workspace_id) = graph.workspace_id else {
        return Ok(false);
    };
    let service = Principal::service(&state.settings.onto_workspace_prefix, workspace_id);
    let mut tx = state.db.begin().await?;
    repo::lock(&mut *tx, graph.id).await?;
    if repo::is_migrated(&mut *tx, graph.id).await? {
        return Ok(false);
    }
    // The projection may have stated the graph's entity already: without a type, it holds no
    // vocabulary (nexc never writes one without types).
    let copied = match backend.vocabulary(&service, &graph_key(graph.id)).await {
        Ok(found) if !found.is_empty() => false,
        Ok(_) | Err(OntoError::NotFound(_)) => {
            let mut vocabulary = repo::legacy_ontology(&mut *tx, graph.id)
                .await?
                .unwrap_or_else(Ontology::starter);
            vocabulary.normalize();
            write(backend, &service, graph, &vocabulary).await?;
            true
        }
        Err(err) => return Err(err.into()),
    };
    repo::mark_migrated(&mut *tx, graph.id, workspace_id).await?;
    tx.commit().await?;
    if copied {
        state.caches.invalidate(Key::Vocabulary(graph.id));
    }
    Ok(copied)
}

/// One start-up pass, when no other replica runs one: `None` when another
/// does (this replica looks again later).
async fn run_alone(state: &AppState) -> anyhow::Result<Option<Report>> {
    let mut conn = state.db.acquire().await?;
    // A lock left on a pooled connection would outlive the pass.
    conn.close_on_drop();
    if !repo::try_lock_pass(&mut *conn).await? {
        return Ok(None);
    }
    let report = run(state).await;
    repo::unlock_pass(&mut *conn).await?;
    report.map(Some)
}

/// The start-up pass, in the background: it runs until no graph fails,
/// trying failed graphs again every minute (graphs created later are
/// initialised on their first read). One replica runs it at a time; the
/// others wait until nothing remains.
pub fn spawn_at_start(state: &AppState, shutdown: CancellationToken) -> bool {
    if state.ontology.is_none() || !state.settings.onto_migrate_on_start {
        return false;
    }
    let state = state.clone();
    tokio::spawn(async move {
        loop {
            let done = match run_alone(&state).await {
                Ok(Some(report)) => {
                    if report.migrated + report.failed > 0 {
                        tracing::info!(?report, "graph vocabularies copied to onto-intelli");
                    }
                    report.failed == 0
                }
                Ok(None) => repo::remaining(&state.db).await.is_ok_and(|left| left == 0),
                Err(err) => {
                    tracing::warn!(error = %err, "onto-intelli vocabulary migration failed");
                    false
                }
            };
            if done {
                return;
            }
            tokio::select! {
                _ = shutdown.cancelled() => return,
                _ = tokio::time::sleep(RETRY) => {}
            }
        }
    });
    true
}
