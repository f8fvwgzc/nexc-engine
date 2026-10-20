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

use std::time::Duration;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::vocabulary::{GraphRef, graph_key};
use super::{OntoError, OntologyBackend, Principal};
use crate::app::AppState;
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
    /// Graphs onto-intelli already held as nexc would have written them.
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
async fn initial(state: &AppState, graph: GraphRef<'_>) -> Result<Ontology, sqlx::Error> {
    let stored = if repo::is_migrated(&state.db, graph.id).await? {
        None
    } else {
        repo::legacy_ontology(&state.db, graph.id).await?
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

/// Initialises a graph onto-intelli does not know (its vocabulary was not
/// found): copies what nexc stored for it, or the starter set, and records it.
pub async fn ensure(
    state: &AppState,
    backend: &dyn OntologyBackend,
    graph: GraphRef<'_>,
) -> Result<Ontology, EnsureError> {
    let Some(workspace_id) = graph.workspace_id else {
        return Ok(Ontology::starter());
    };
    let vocabulary = initial(state, graph).await?;
    let service = Principal::service(&state.settings.onto_workspace_prefix, workspace_id);
    let written = write(backend, &service, graph, &vocabulary).await?;
    repo::mark_migrated(&state.db, graph.id, workspace_id).await?;
    Ok(written)
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
            match migrate_one(state, backend.as_ref(), graph).await {
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

/// Copies one graph; false when onto-intelli held it already and nexc
/// stored nothing of its own for it (nothing is written then).
async fn migrate_one(
    state: &AppState,
    backend: &dyn OntologyBackend,
    graph: GraphRef<'_>,
) -> Result<bool, EnsureError> {
    let Some(workspace_id) = graph.workspace_id else {
        return Ok(false);
    };
    let service = Principal::service(&state.settings.onto_workspace_prefix, workspace_id);
    let stored = repo::legacy_ontology(&state.db, graph.id).await?;
    match backend.vocabulary(&service, &graph_key(graph.id)).await {
        Ok(_) if stored.is_none() => {
            repo::mark_migrated(&state.db, graph.id, workspace_id).await?;
            Ok(false)
        }
        Ok(_) | Err(OntoError::NotFound(_)) => {
            let mut vocabulary = stored.unwrap_or_else(Ontology::starter);
            vocabulary.normalize();
            write(backend, &service, graph, &vocabulary).await?;
            repo::mark_migrated(&state.db, graph.id, workspace_id).await?;
            Ok(true)
        }
        Err(err) => Err(err.into()),
    }
}

/// The start-up pass, in the background: it runs until no graph fails,
/// trying failed graphs again every minute (graphs created later are
/// initialised on their first read).
pub fn spawn_at_start(state: &AppState, shutdown: CancellationToken) -> bool {
    if state.ontology.is_none() || !state.settings.onto_migrate_on_start {
        return false;
    }
    let state = state.clone();
    tokio::spawn(async move {
        loop {
            let done = match run(&state).await {
                Ok(report) => {
                    if report.migrated + report.failed > 0 {
                        tracing::info!(?report, "graph vocabularies copied to onto-intelli");
                    }
                    report.failed == 0
                }
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
