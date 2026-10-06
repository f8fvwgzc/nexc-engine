//! In-process index of each workspace's memories.
//!
//! Retrieval runs for every planned graph and every executed node. Reading
//! the candidates from PostgreSQL each time means a full-text query plus
//! decoding one 256-float embedding per row; this index keeps the decoded
//! memories of a small workspace in memory so that a retrieval is one pass
//! over a slice. A workspace with [`ANN_THRESHOLD`] memories or more is not
//! held here at all: only its size is, and it is searched in the database
//! (see `memory::retrieve`), so memory use and results do not depend on how
//! much a workspace has learned. Entries are dropped when this process writes
//! to the workspace's memories, and expire after [`TTL`] so that writes made
//! by another backend instance are picked up without a coordination protocol.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use sqlx::PgPool;
use uuid::Uuid;

use crate::repo::memories::{self, StoredMemory};

/// How long a loaded entry is trusted before it is read again.
pub const TTL: Duration = Duration::from_secs(30);

/// A workspace with at least this many memories is searched in the database
/// (full text, plus its HNSW index when it has one) instead of being scanned
/// here.
pub const ANN_THRESHOLD: usize = 5_000;

/// A word in more than this share of all memories is not searched for.
pub const COMMON_TERM_SHARE: f64 = 0.02;
/// How long the list of common words is trusted.
pub const COMMON_TERMS_TTL: Duration = Duration::from_secs(600);

/// What the index holds for a workspace.
#[derive(Clone)]
pub enum Snapshot {
    /// Every memory of the workspace, decoded.
    Small(Arc<[StoredMemory]>),
    /// Too many to hold: only how many there are.
    Large(i64),
}

struct Entry {
    loaded_at: Instant,
    snapshot: Snapshot,
}

/// Decoded memories per workspace.
pub struct MemoryIndex {
    workspaces: DashMap<Uuid, Entry>,
    /// Whether the database offers vector search (see `memory::vectors`).
    vector_search: AtomicBool,
    ann_threshold: AtomicUsize,
    /// Words too common to search for, and when they were read.
    common_terms: RwLock<Option<(Instant, Arc<HashSet<String>>)>>,
}

impl Default for MemoryIndex {
    fn default() -> Self {
        MemoryIndex {
            workspaces: DashMap::new(),
            vector_search: AtomicBool::new(false),
            ann_threshold: AtomicUsize::new(ANN_THRESHOLD),
            common_terms: RwLock::new(None),
        }
    }
}

impl MemoryIndex {
    /// The memories of `workspace` when it is small, else its size; from the
    /// index when fresh, else from the database.
    pub async fn snapshot(&self, db: &PgPool, workspace: Uuid) -> Result<Snapshot, sqlx::Error> {
        if let Some(entry) = self.workspaces.get(&workspace)
            && entry.loaded_at.elapsed() < TTL
        {
            return Ok(entry.snapshot.clone());
        }
        // No lock is held across the queries: two concurrent misses both load and the later
        // insert wins, which is harmless.
        let loaded_at = Instant::now();
        let threshold = self.ann_threshold.load(Ordering::Relaxed);
        let total = memories::count_for(db, workspace).await?;
        let snapshot = if usize::try_from(total).unwrap_or(usize::MAX) >= threshold {
            Snapshot::Large(total)
        } else {
            // The count bounds the read: at most `threshold` rows are ever decoded.
            let limit = i64::try_from(threshold).unwrap_or(i64::MAX);
            Snapshot::Small(memories::all_for(db, workspace, limit).await?.into())
        };
        self.workspaces.insert(
            workspace,
            Entry {
                loaded_at,
                snapshot: snapshot.clone(),
            },
        );
        Ok(snapshot)
    }

    /// The words that occur in more than [`COMMON_TERM_SHARE`] of all memories,
    /// read from the database's statistics at most every [`COMMON_TERMS_TTL`].
    /// A failure to read them is not an error: nothing is treated as common.
    pub async fn common_terms(&self, db: &PgPool) -> Arc<HashSet<String>> {
        if let Ok(guard) = self.common_terms.read()
            && let Some((read_at, terms)) = guard.as_ref()
            && read_at.elapsed() < COMMON_TERMS_TTL
        {
            return terms.clone();
        }
        let terms: Arc<HashSet<String>> = Arc::new(
            memories::common_terms(db, COMMON_TERM_SHARE)
                .await
                .unwrap_or_default()
                .into_iter()
                .collect(),
        );
        if let Ok(mut guard) = self.common_terms.write() {
            *guard = Some((Instant::now(), terms.clone()));
        }
        terms
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
        // What is cached was decided with the old threshold.
        self.workspaces.clear();
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
