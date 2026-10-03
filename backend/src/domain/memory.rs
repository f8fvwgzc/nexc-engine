//! Long-term memory (Mem0 / Hindsight style): kinds, consolidation and
//! hybrid retrieval scoring rules.

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::string_enum;

/// Name of the structured-output schema used for memory extraction.
pub const MEMORY_SCHEMA_NAME: &str = "memory_extraction";

/// Maximum characters of one memory.
pub const MEMORY_MAX_CHARS: usize = 1_000;
/// Cosine similarity at or above which a candidate duplicates a memory.
pub const DUPLICATE_THRESHOLD: f32 = 0.92;
/// Cosine similarity at or above which a candidate updates a memory.
pub const UPDATE_THRESHOLD: f32 = 0.75;
/// Half-life style decay constant for recency, in days.
pub const RECENCY_DECAY_DAYS: f64 = 30.0;

string_enum!(
    /// What a memory captures.
    MemoryKind {
        Fact => "fact",
        Experience => "experience",
        Observation => "observation",
        Preference => "preference",
    }
);

string_enum!(
    /// Visibility scope of a memory.
    MemoryScope {
        User => "user",
        Graph => "graph",
        Node => "node",
    }
);

/// A stored memory. `score` is set only for search results.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Memory {
    pub id: Uuid,
    pub scope: MemoryScope,
    #[schema(required = true)]
    pub graph_id: Option<Uuid>,
    #[schema(required = true)]
    pub node_id: Option<Uuid>,
    pub kind: MemoryKind,
    pub content: String,
    /// Importance in `[0, 1]`.
    pub importance: f64,
    pub access_count: i64,
    #[schema(required = true)]
    pub score: Option<f64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// What to do with a newly extracted memory candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consolidation {
    /// Already known: only reinforce the existing memory.
    Noop,
    /// Supersedes the existing memory: replace its content.
    Update,
    /// New information: store it.
    Add,
}

/// Decides how a candidate relates to its most similar existing memory.
pub fn consolidate(best_similarity: Option<f32>) -> Consolidation {
    match best_similarity {
        Some(s) if s >= DUPLICATE_THRESHOLD => Consolidation::Noop,
        Some(s) if s >= UPDATE_THRESHOLD => Consolidation::Update,
        _ => Consolidation::Add,
    }
}

/// Recency × importance factor in `[0, 1]`.
pub fn recency_weight(age_days: f64, importance: f64) -> f64 {
    (-age_days.max(0.0) / RECENCY_DECAY_DAYS).exp() * importance.clamp(0.0, 1.0)
}

/// Hybrid retrieval score: `0.5·cosine + 0.3·bm25 + 0.2·recency×importance`,
/// with `bm25` already normalised to `[0, 1]`.
pub fn hybrid_score(cosine: f64, bm25_normalized: f64, age_days: f64, importance: f64) -> f64 {
    0.5 * cosine.max(0.0) + 0.3 * bm25_normalized + 0.2 * recency_weight(age_days, importance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consolidation_thresholds() {
        assert_eq!(consolidate(Some(0.95)), Consolidation::Noop);
        assert_eq!(consolidate(Some(0.92)), Consolidation::Noop);
        assert_eq!(consolidate(Some(0.8)), Consolidation::Update);
        assert_eq!(consolidate(Some(0.5)), Consolidation::Add);
        assert_eq!(consolidate(None), Consolidation::Add);
    }

    #[test]
    fn scoring_prefers_relevant_recent_important() {
        let fresh = hybrid_score(0.9, 1.0, 0.0, 1.0);
        let stale = hybrid_score(0.9, 1.0, 365.0, 1.0);
        let unrelated = hybrid_score(0.1, 0.0, 0.0, 1.0);
        assert!(fresh > stale && stale > unrelated);
        assert!((fresh - 1.0).abs() < 0.06);
        assert_eq!(recency_weight(-5.0, 2.0), 1.0);
    }
}
