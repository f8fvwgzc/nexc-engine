//! In-process index of each owner's memories.
//!
//! Retrieval runs for every planned graph and every executed node. Reading
//! the candidates from PostgreSQL each time means a full-text query plus
//! decoding one 256-float embedding per row; this index keeps the decoded
//! memories of an owner in memory so that a retrieval is one pass over a
//! slice. Entries are dropped when this process writes to the owner's
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
/// Most memories kept per owner (the most recently updated ones).
pub const MAX_PER_OWNER: i64 = 20_000;

struct Entry {
    loaded_at: Instant,
    memories: Arc<[StoredMemory]>,
}

/// Decoded memories per owner.
#[derive(Default)]
pub struct MemoryIndex {
    owners: DashMap<Uuid, Entry>,
}

impl MemoryIndex {
    /// The memories of `owner`, from the index when fresh, else from the database.
    pub async fn load(&self, db: &PgPool, owner: Uuid) -> Result<Arc<[StoredMemory]>, sqlx::Error> {
        if let Some(entry) = self.owners.get(&owner)
            && entry.loaded_at.elapsed() < TTL
        {
            return Ok(entry.memories.clone());
        }
        // No lock is held across the query: two concurrent misses both load and the later
        // insert wins, which is harmless.
        let loaded_at = Instant::now();
        let memories: Arc<[StoredMemory]> =
            memories::all_for(db, owner, MAX_PER_OWNER).await?.into();
        self.owners.insert(
            owner,
            Entry {
                loaded_at,
                memories: memories.clone(),
            },
        );
        Ok(memories)
    }

    /// Forgets what is cached for `owner`; call after writing their memories.
    pub fn invalidate(&self, owner: Uuid) {
        self.owners.remove(&owner);
    }

    /// Number of owners currently cached.
    pub fn len(&self) -> usize {
        self.owners.len()
    }

    /// True when nothing is cached.
    pub fn is_empty(&self) -> bool {
        self.owners.is_empty()
    }
}
