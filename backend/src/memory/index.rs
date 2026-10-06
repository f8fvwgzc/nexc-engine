//! In-process index of each workspace's memories.
//!
//! Retrieval runs for every planned graph and every executed node. Reading
//! the candidates from PostgreSQL each time means a full-text query plus
//! decoding one 256-float embedding per row; this index keeps the decoded
//! memories of a workspace in memory so that a retrieval is one pass over a
//! slice. Entries are dropped when this process writes to the workspace's
//! memories, and expire after [`TTL`] so that writes made by another backend
//! instance are picked up without a coordination protocol.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use sqlx::PgPool;
use uuid::Uuid;

use crate::repo::memories::{self, StoredMemory};

/// How long a loaded entry is trusted before it is read again.
pub const TTL: Duration = Duration::from_secs(30);
/// Most memories kept per workspace (the most recently updated ones).
pub const MAX_PER_WORKSPACE: i64 = 20_000;

struct Entry {
    loaded_at: Instant,
    memories: Arc<[StoredMemory]>,
}

/// A workspace with at least this many memories is searched through the
/// database's HNSW index (when it has one) instead of being scanned here.
pub const ANN_THRESHOLD: usize = 5_000;

/// Decoded memories per workspace.
pub struct MemoryIndex {
    workspaces: DashMap<Uuid, Entry>,
    /// Whether the database offers vector search (see `memory::vectors`).
    vector_search: AtomicBool,
    ann_threshold: AtomicUsize,
}

impl Default for MemoryIndex {
    fn default() -> Self {
        MemoryIndex {
            workspaces: DashMap::new(),
            vector_search: AtomicBool::new(false),
            ann_threshold: AtomicUsize::new(ANN_THRESHOLD),
        }
    }
}

impl MemoryIndex {
    /// The memories of `workspace`, from the index when fresh, else from the database.
    pub async fn load(
        &self,
        db: &PgPool,
        workspace: Uuid,
    ) -> Result<Arc<[StoredMemory]>, sqlx::Error> {
        if let Some(entry) = self.workspaces.get(&workspace)
            && entry.loaded_at.elapsed() < TTL
        {
            return Ok(entry.memories.clone());
        }
        // No lock is held across the query: two concurrent misses both load and the later
        // insert wins, which is harmless.
        let loaded_at = Instant::now();
        let memories: Arc<[StoredMemory]> = memories::all_for(db, workspace, MAX_PER_WORKSPACE)
            .await?
            .into();
        self.workspaces.insert(
            workspace,
            Entry {
                loaded_at,
                memories: memories.clone(),
            },
        );
        Ok(memories)
    }

    /// Records whether the database offers vector search.
    pub fn set_vector_search(&self, available: bool) {
        self.vector_search.store(available, Ordering::Relaxed);
    }

    /// Whether the database offers vector search.
    pub fn vector_search(&self) -> bool {
        self.vector_search.load(Ordering::Relaxed)
    }

    /// Changes from how many memories on a workspace is searched in the database.
    pub fn set_ann_threshold(&self, memories: usize) {
        self.ann_threshold.store(memories, Ordering::Relaxed);
    }

    /// Whether a workspace holding `memories` memories is searched in the database.
    pub fn uses_database_search(&self, memories: usize) -> bool {
        self.vector_search() && memories >= self.ann_threshold.load(Ordering::Relaxed)
    }

    /// Forgets what is cached for `workspace`; call after writing its memories.
    pub fn invalidate(&self, workspace: Uuid) {
        self.workspaces.remove(&workspace);
    }

    /// Number of workspaces currently cached.
    pub fn len(&self) -> usize {
        self.workspaces.len()
    }

    /// True when nothing is cached.
    pub fn is_empty(&self) -> bool {
        self.workspaces.is_empty()
    }
}
