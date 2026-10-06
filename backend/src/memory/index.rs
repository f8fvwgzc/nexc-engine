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

/// Decoded memories per workspace.
#[derive(Default)]
pub struct MemoryIndex {
    workspaces: DashMap<Uuid, Entry>,
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
