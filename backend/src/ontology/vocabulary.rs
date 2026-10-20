//! A graph's execution vocabulary, read from and written to onto-intelli.
//!
//! The scheduler, the planner, the editor and the canvas all ask here for a
//! graph's node types and relation types (executor, default role, stage,
//! artifacts, code execution, blocking). Reads go through an in-process
//! cache with a short lifetime (`NEXC_ONTO_VOCABULARY_TTL_SECS`); a change
//! made through nexc replaces the entry here and drops it on every other
//! replica. A graph onto-intelli does not know yet (the backend's own
//! principal does not find it) is initialised on first read
//! ([`super::migrate::ensure`]): with the ontology nexc stored for it before
//! this release, or with the starter vocabulary.
//!
//! With onto-intelli off (or a graph outside any workspace) every graph has
//! the read-only starter vocabulary. When onto-intelli cannot be reached a
//! vocabulary kept from before is served, however old; without one, reads
//! fail with [`VocabularyError::Unavailable`], which a run turns into a
//! retryable failure of the node.

use std::sync::{Arc, LazyLock};

use uuid::Uuid;

use super::{OntoError, OntologyBackend, Principal, migrate};
use crate::app::AppState;
use crate::app::caches::Key;
use crate::domain::AppError;
use crate::domain::ontology::Ontology;
use crate::domain::workspace::WorkspaceRole;
use crate::engine::analysis::dependency_graph;
use crate::realtime::events::WsMessage;
use crate::repo;

static STARTER: LazyLock<Arc<Ontology>> = LazyLock::new(|| Arc::new(Ontology::starter()));

/// The graph a vocabulary belongs to.
#[derive(Debug, Clone, Copy)]
pub struct GraphRef<'a> {
    pub id: Uuid,
    /// `None` for a graph outside any workspace (it keeps the starter vocabulary).
    pub workspace_id: Option<Uuid>,
    /// Its name, the title of its entity in onto-intelli.
    pub name: &'a str,
}

/// Who reads or writes a vocabulary: onto-intelli checks and audits them.
#[derive(Debug, Clone, Copy)]
pub enum Actor {
    /// A member, with the roles of their workspace role.
    User(Uuid),
    /// The run engine (an agent).
    Runtime,
}

/// Why a vocabulary could not be had or changed.
#[derive(Debug, thiserror::Error)]
pub enum VocabularyError {
    /// onto-intelli is not reachable and nothing is kept for the graph.
    #[error("ontology unavailable: {0}")]
    Unavailable(String),
    /// onto-intelli refused the caller.
    #[error("onto-intelli refused: {0}")]
    Denied(String),
    /// onto-intelli refused the vocabulary (a field its schema does not accept).
    #[error("onto-intelli rejected the vocabulary: {0}")]
    Invalid(String),
    /// Vocabularies are read-only: onto-intelli is off.
    #[error(
        "graph vocabularies are read-only: onto-intelli is not configured (NEXC_ONTO_MODE=off)"
    )]
    ReadOnly,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("onto-intelli failed: {0}")]
    Other(String),
}

impl From<OntoError> for VocabularyError {
    fn from(err: OntoError) -> Self {
        match err {
            OntoError::Unavailable(m) => VocabularyError::Unavailable(m),
            OntoError::Denied(m) => VocabularyError::Denied(m),
            OntoError::Invalid(m) | OntoError::NotFound(m) => VocabularyError::Invalid(m),
            OntoError::Other(m) => VocabularyError::Other(m),
        }
    }
}

impl From<VocabularyError> for AppError {
    fn from(err: VocabularyError) -> Self {
        match err {
            VocabularyError::Unavailable(_) => AppError::Unavailable(err.to_string()),
            VocabularyError::Denied(_) => AppError::Forbidden(err.to_string()),
            VocabularyError::Invalid(m) => AppError::Unprocessable(m),
            VocabularyError::ReadOnly => AppError::Conflict(err.to_string()),
            VocabularyError::Database(e) => e.into(),
            VocabularyError::Other(m) => AppError::Internal(anyhow::anyhow!("onto-intelli: {m}")),
        }
    }
}

/// The starter vocabulary every graph has while onto-intelli is off.
pub fn starter() -> Arc<Ontology> {
    STARTER.clone()
}

/// The key of a graph in onto-intelli (`execution_graph:{key}`).
pub fn graph_key(graph_id: Uuid) -> String {
    graph_id.to_string()
}

/// The principal `actor` acts as in `workspace_id`'s tenant.
pub async fn principal_of(
    state: &AppState,
    workspace_id: Uuid,
    actor: Actor,
) -> Result<Principal, sqlx::Error> {
    let prefix = &state.settings.onto_workspace_prefix;
    Ok(match actor {
        Actor::User(user) => {
            let role = repo::workspaces::role_of(&state.db, workspace_id, user)
                .await?
                .unwrap_or(WorkspaceRole::Guest);
            Principal::user(prefix, workspace_id, user, role)
        }
        Actor::Runtime => Principal::runtime(prefix, workspace_id),
    })
}

/// The vocabulary of `graph`, as `actor` reads it.
pub async fn of(
    state: &AppState,
    graph: GraphRef<'_>,
    actor: Actor,
) -> Result<Arc<Ontology>, VocabularyError> {
    let (Some(backend), Some(workspace_id)) = (&state.ontology, graph.workspace_id) else {
        return Ok(starter());
    };
    let kept = state.caches.vocabulary(graph.id);
    if let Some((vocabulary, true)) = &kept {
        return Ok(vocabulary.clone());
    }
    let principal = principal_of(state, workspace_id, actor).await?;
    match fetch(state, backend.as_ref(), &principal, graph).await {
        Ok(vocabulary) => {
            let vocabulary = Arc::new(vocabulary);
            state.caches.put_vocabulary(graph.id, vocabulary.clone());
            let changed = kept.as_ref().is_some_and(|(old, _)| **old != *vocabulary);
            if changed || kept.is_none() {
                follow(state, graph.id, &vocabulary, changed).await?;
            }
            Ok(vocabulary)
        }
        Err(OntoError::Unavailable(detail)) => match kept {
            Some((vocabulary, _)) => {
                tracing::debug!(graph = %graph.id, %detail, "onto-intelli unreachable; serving the kept vocabulary");
                Ok(vocabulary)
            }
            None => Err(VocabularyError::Unavailable(detail)),
        },
        Err(err) => Err(err.into()),
    }
}

/// A vocabulary changed in onto-intelli itself (its UI, an approved
/// proposal, an action) is carried over to nexc: edges keep their relation's
/// `blocking` flag (the run DAG is built from it), unless that would close a
/// dependency cycle, in which case they keep theirs and the conflict is
/// logged; and the graph's open canvases are told (`ontology.updated`).
async fn follow(
    state: &AppState,
    graph_id: Uuid,
    vocabulary: &Arc<Ontology>,
    changed: bool,
) -> Result<(), sqlx::Error> {
    let stale: Vec<(String, bool)> = repo::edges::kinds(&state.db, graph_id)
        .await?
        .into_iter()
        .filter_map(|(kind, blocking)| {
            let wanted = vocabulary.relation_type(&kind)?.blocking;
            (wanted != blocking).then_some((kind, wanted))
        })
        .collect();
    if !stale.is_empty() {
        let nodes = repo::nodes::list(&state.db, graph_id).await?;
        let mut edges = repo::edges::list(&state.db, graph_id).await?;
        for edge in &mut edges {
            if let Some(relation) = vocabulary.relation_type(&edge.kind) {
                edge.blocking = relation.blocking;
            }
        }
        let (_, dag) = dependency_graph(&nodes, &edges);
        if dag.cycles().is_empty() {
            for (kind, blocking) in &stale {
                repo::edges::set_blocking(&state.db, graph_id, kind, *blocking).await?;
            }
        } else {
            tracing::warn!(graph = %graph_id, "a relation made blocking in onto-intelli would close a dependency cycle; the graph's edges keep their order");
        }
    }
    if changed {
        state.hub.broadcast(
            graph_id,
            WsMessage::OntologyUpdated {
                ontology: Ontology::clone(vocabulary),
            },
        );
    }
    Ok(())
}

/// Reads the vocabulary as `principal`.
///
/// onto-intelli answers "not found" both for a graph it does not hold and
/// for one the reader may not see: a private team's graph is restricted to
/// the team and `team:admins`, which leaves out the run engine and a member
/// the projection has not named in the team yet. nexc decided already that
/// the reader may work on the graph, and the cache serves every reader
/// alike, so the vocabulary is then read as the backend's own principal.
/// Only a graph the backend itself does not find is initialised
/// ([`migrate::ensure`]): never one onto-intelli holds.
///
/// "Found" needs care: the projection states every graph as the entity
/// `execution_graph:{id}`, which is also where the vocabulary hangs, so a
/// graph projected before its vocabulary was ever written reads as found,
/// with no type at all. nexc never writes a vocabulary without types (one
/// of each sort at least), so an empty one is initialised too, unless nexc
/// recorded the graph as migrated (then someone emptied it in onto-intelli,
/// and that is what it holds).
async fn fetch(
    state: &AppState,
    backend: &dyn OntologyBackend,
    principal: &Principal,
    graph: GraphRef<'_>,
) -> Result<Ontology, OntoError> {
    let initialise = || async {
        migrate::ensure(state, backend, graph)
            .await
            .map_err(|err| match err {
                migrate::EnsureError::Onto(err) => err,
                migrate::EnsureError::Database(err) => OntoError::Other(err.to_string()),
            })
    };
    match read_as(backend, principal, graph).await {
        Err(OntoError::NotFound(_)) => initialise().await,
        Ok(found) if found.is_empty() => {
            let migrated = crate::repo::onto_migrations::is_migrated(&state.db, graph.id)
                .await
                .map_err(|err| OntoError::Other(err.to_string()))?;
            if migrated {
                Ok(found)
            } else {
                initialise().await
            }
        }
        found => found,
    }
}

/// Reads the vocabulary as `principal`, or as the backend's own principal
/// when onto-intelli does not show `principal` the graph (see [`fetch`]).
async fn read_as(
    backend: &dyn OntologyBackend,
    principal: &Principal,
    graph: GraphRef<'_>,
) -> Result<Ontology, OntoError> {
    let key = graph_key(graph.id);
    match backend.vocabulary(principal, &key).await {
        Err(OntoError::NotFound(_)) if principal.acts() => {
            backend.vocabulary(&principal.as_service(), &key).await
        }
        found => found,
    }
}

/// The vocabulary onto-intelli holds right now, never the cached one: what
/// a read-modify-write starts from once it holds the graph's row lock. It
/// neither initialises the graph nor touches the cache, so read the graph
/// once (with [`of`]) before taking the lock.
pub async fn fresh(
    state: &AppState,
    graph: GraphRef<'_>,
    actor: Actor,
) -> Result<Arc<Ontology>, VocabularyError> {
    let (Some(backend), Some(workspace_id)) = (&state.ontology, graph.workspace_id) else {
        return Ok(starter());
    };
    let principal = principal_of(state, workspace_id, actor).await?;
    Ok(Arc::new(
        read_as(backend.as_ref(), &principal, graph).await?,
    ))
}

/// Adds types to `graph`'s vocabulary in onto-intelli as `user`, and only
/// them: `InitGraph` upserts the types it is given and leaves every other
/// type as it is, so what was added or changed there meanwhile stays. The
/// cache entry is dropped here and on every other replica (the next read
/// follows onto-intelli's vocabulary). Returns the whole vocabulary after.
pub async fn add(
    state: &AppState,
    graph: GraphRef<'_>,
    user: Uuid,
    added: &Ontology,
) -> Result<Arc<Ontology>, VocabularyError> {
    let (Some(backend), Some(workspace_id)) = (&state.ontology, graph.workspace_id) else {
        return Err(VocabularyError::ReadOnly);
    };
    let principal = principal_of(state, workspace_id, Actor::User(user)).await?;
    let stored = backend
        .init_graph(&principal, &graph_key(graph.id), graph.name, Some(added))
        .await?;
    state.caches.invalidate(Key::Vocabulary(graph.id));
    Ok(Arc::new(stored))
}

/// Replaces `graph`'s vocabulary in onto-intelli as `user` (one atomic,
/// audited write), keeps the new one here and drops it on the other
/// replicas. The caller checked nexc's own rules first.
pub async fn replace(
    state: &AppState,
    graph: GraphRef<'_>,
    user: Uuid,
    vocabulary: &Ontology,
) -> Result<Arc<Ontology>, VocabularyError> {
    let (Some(backend), Some(workspace_id)) = (&state.ontology, graph.workspace_id) else {
        return Err(VocabularyError::ReadOnly);
    };
    let principal = principal_of(state, workspace_id, Actor::User(user)).await?;
    let key = graph_key(graph.id);
    let stored = match backend.set_vocabulary(&principal, &key, vocabulary).await {
        Err(OntoError::NotFound(_)) => {
            fetch(state, backend.as_ref(), &principal, graph).await?;
            match backend.set_vocabulary(&principal, &key, vocabulary).await {
                // It is there (the backend read it), but not for them: a private team's
                // graph before the projection named them a member of the team.
                Err(OntoError::NotFound(what)) => {
                    return Err(VocabularyError::Denied(format!(
                        "{what} is not visible to {principal} in onto-intelli yet; try again in a moment"
                    )));
                }
                other => other?,
            }
        }
        other => other?,
    };
    let stored = Arc::new(stored);
    state.caches.invalidate(Key::Vocabulary(graph.id));
    state.caches.put_vocabulary(graph.id, stored.clone());
    Ok(stored)
}

/// Whether vocabularies can be changed here (onto-intelli is configured).
pub fn editable(state: &AppState) -> bool {
    state.ontology.is_some()
}

/// Whether `graph`'s vocabulary can be changed: onto-intelli is configured
/// and the graph belongs to a workspace (one outside any keeps the starter).
pub fn editable_for(state: &AppState, graph: GraphRef<'_>) -> bool {
    editable(state) && graph.workspace_id.is_some()
}
