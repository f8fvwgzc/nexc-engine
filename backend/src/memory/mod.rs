//! Long-term memory (Mem0 / Hindsight style).
//!
//! * **Extraction** – after a node succeeds, a low-effort structured LLM call
//!   pulls salient facts out of its output.
//! * **Consolidation** – each candidate is compared (C-kernel embedding
//!   cosine) with memories in the same scope: ≥ 0.92 reinforces the existing
//!   memory, 0.75–0.92 replaces it, otherwise the candidate is added.
//! * **Retrieval** – hybrid score `0.5·cosine + 0.3·BM25 + 0.2·recency×importance`
//!   over full-text and recency candidates.
#![forbid(unsafe_code)]

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::graph::GraphNode;
use crate::domain::memory::{
    Consolidation, MEMORY_MAX_CHARS, MEMORY_SCHEMA_NAME, Memory, MemoryKind, MemoryScope,
    consolidate, hybrid_score,
};
use crate::domain::prompt::OUTPUT_MARKER;
use crate::dsa::bm25::Bm25Index;
use crate::kernel;
use crate::llm::service::LlmService;
use crate::llm::{JsonSchema, LlmRequest, LlmTarget, Message, collect};
use crate::repo::memories::{self, NewMemory, StoredMemory};

const MAX_CANDIDATES_PER_NODE: usize = 8;
const OUTPUT_CHARS_FOR_EXTRACTION: usize = 8_000;

/// Ranks `candidates` for `query` and returns the best `limit` with scores.
pub fn rank(
    candidates: Vec<StoredMemory>,
    query: &str,
    limit: usize,
    now: DateTime<Utc>,
) -> Vec<Memory> {
    if candidates.is_empty() || limit == 0 {
        return Vec::new();
    }
    let q = kernel::embed(query);
    let bm25 = Bm25Index::build(
        &candidates
            .iter()
            .map(|c| c.memory.content.as_str())
            .collect::<Vec<_>>(),
    );
    let lexical = bm25.normalized_scores(query);
    let mut scored: Vec<Memory> = candidates
        .into_iter()
        .zip(lexical)
        .map(|(c, lex)| {
            let age_days = (now - c.memory.updated_at).num_seconds() as f64 / 86_400.0;
            let cosine = f64::from(kernel::dot(&q, &c.embedding));
            let mut memory = c.memory;
            memory.score = Some(
                (hybrid_score(cosine, lex, age_days, memory.importance) * 1000.0).round() / 1000.0,
            );
            memory
        })
        .collect();
    scored.sort_by(|a, b| b.score.unwrap_or(0.0).total_cmp(&a.score.unwrap_or(0.0)));
    scored.truncate(limit);
    scored
}

/// Hybrid retrieval of a user's memories (optionally restricted to a graph
/// plus user-scope memories). Counts an access for each returned memory.
pub async fn retrieve(
    db: &PgPool,
    owner: Uuid,
    graph_id: Option<Uuid>,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<Memory>> {
    let candidates = memories::candidates(db, owner, graph_id, query).await?;
    let ranked = rank(candidates, query, limit, Utc::now());
    let ids: Vec<Uuid> = ranked.iter().map(|m| m.id).collect();
    if !ids.is_empty() {
        memories::record_access(db, &ids).await?;
    }
    Ok(ranked)
}

/// One memory proposed by the extraction call.
#[derive(Debug, Clone, Deserialize)]
pub struct Candidate {
    pub kind: MemoryKind,
    pub content: String,
    pub importance: f64,
}

#[derive(Debug, Deserialize)]
struct Extraction {
    memories: Vec<Candidate>,
}

fn extraction_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["memories"],
        "properties": {
            "memories": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["kind", "content", "importance"],
                    "properties": {
                        "kind": { "type": "string", "enum": ["fact", "experience", "observation", "preference"] },
                        "content": { "type": "string" },
                        "importance": { "type": "number" }
                    }
                }
            }
        }
    })
}

const EXTRACTION_SYSTEM: &str = "You maintain the long-term memory of a project assistant. From the node \
    output below, extract at most 8 short, self-contained statements worth remembering for future \
    tasks of this project: facts, decisions, user preferences and lessons learned. Skip anything \
    generic or obvious. importance is between 0 and 1. Return an empty list if nothing qualifies.";

/// Asks the LLM for memory candidates from a node's output.
pub async fn extract(
    llm: &LlmService,
    target: LlmTarget,
    goal: &str,
    node: &GraphNode,
    output: &str,
) -> anyhow::Result<Vec<Candidate>> {
    let excerpt: String = output.chars().take(OUTPUT_CHARS_FOR_EXTRACTION).collect();
    let prompt = format!(
        "Project goal: {goal}\nNode: {}\n\n{OUTPUT_MARKER}\n{excerpt}",
        node.title
    );
    let request = LlmRequest {
        target,
        system: EXTRACTION_SYSTEM.into(),
        messages: vec![Message::user(prompt)],
        max_tokens: 16_000,
        json_schema: Some(JsonSchema {
            name: MEMORY_SCHEMA_NAME,
            schema: extraction_schema(),
        }),
        effort: Some("low"),
        cacheable: true,
    };
    let done = collect(llm.stream(request), |_| {}).await?;
    let parsed: Extraction = serde_json::from_str(done.text.trim())?;
    Ok(parsed
        .memories
        .into_iter()
        .filter_map(|mut c| {
            c.content = c.content.trim().chars().take(MEMORY_MAX_CHARS).collect();
            c.importance = if c.importance.is_finite() {
                c.importance.clamp(0.0, 1.0)
            } else {
                0.5
            };
            (!c.content.is_empty()).then_some(c)
        })
        .take(MAX_CANDIDATES_PER_NODE)
        .collect())
}

/// Consolidates candidates into a graph's memory; returns the decisions taken.
pub async fn store(
    db: &PgPool,
    owner: Uuid,
    graph_id: Uuid,
    candidates: &[Candidate],
) -> anyhow::Result<Vec<Consolidation>> {
    let mut existing = memories::in_scope(db, owner, MemoryScope::Graph, Some(graph_id)).await?;
    let mut decisions = Vec::with_capacity(candidates.len());
    for c in candidates {
        let embedding = kernel::embed(&c.content);
        let best = existing
            .iter()
            .enumerate()
            .map(|(i, m)| (kernel::dot(&embedding, &m.embedding), i))
            .max_by(|a, b| a.0.total_cmp(&b.0));
        let decision = consolidate(best.map(|(similarity, _)| similarity));
        match (decision, best) {
            (Consolidation::Noop, Some((_, i))) => {
                memories::reinforce(db, existing[i].memory.id, c.importance).await?
            }
            (Consolidation::Update, Some((_, i))) => {
                let m = &mut existing[i];
                memories::replace_content(db, m.memory.id, &c.content, &embedding, c.importance)
                    .await?;
                m.memory.content.clone_from(&c.content);
                m.embedding = embedding;
            }
            _ => {
                let new = NewMemory {
                    owner_id: owner,
                    scope: MemoryScope::Graph,
                    graph_id: Some(graph_id),
                    node_id: None,
                    kind: c.kind,
                    content: &c.content,
                    embedding: &embedding,
                    importance: c.importance,
                };
                let id = memories::insert(db, &new).await?;
                let now = Utc::now();
                existing.push(StoredMemory {
                    memory: Memory {
                        id,
                        scope: MemoryScope::Graph,
                        graph_id: Some(graph_id),
                        node_id: None,
                        kind: c.kind,
                        content: c.content.clone(),
                        importance: c.importance,
                        access_count: 0,
                        score: None,
                        created_at: now,
                        updated_at: now,
                    },
                    embedding,
                });
            }
        }
        decisions.push(decision);
    }
    Ok(decisions)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(content: &str, days_old: i64, importance: f64) -> StoredMemory {
        let at = Utc::now() - chrono::Duration::days(days_old);
        StoredMemory {
            memory: Memory {
                id: Uuid::now_v7(),
                scope: MemoryScope::Graph,
                graph_id: None,
                node_id: None,
                kind: MemoryKind::Fact,
                content: content.into(),
                importance,
                access_count: 0,
                score: None,
                created_at: at,
                updated_at: at,
            },
            embedding: kernel::embed(content),
        }
    }

    #[test]
    fn ranks_relevant_memories_first() {
        let ranked = rank(
            vec![
                stored("The report must follow APA citation style", 1, 0.8),
                stored("Deploy target is Kubernetes on GKE", 1, 0.8),
                stored("Citation style for the report is APA 7th edition", 200, 0.2),
            ],
            "which citation style does the report use",
            2,
            Utc::now(),
        );
        assert_eq!(ranked.len(), 2);
        assert!(ranked[0].content.contains("APA"));
        assert!(ranked[0].score.unwrap() >= ranked[1].score.unwrap());
        assert!(rank(vec![], "x", 5, Utc::now()).is_empty());
    }
}
