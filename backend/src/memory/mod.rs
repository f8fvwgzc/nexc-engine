//! Long-term memory (Mem0 / Hindsight style).
//!
//! * **Extraction** – after a node succeeds, a low-effort structured LLM call
//!   pulls salient facts out of its output.
//! * **Consolidation** – each candidate is compared (C-kernel embedding
//!   cosine) with memories in the same scope: ≥ 0.92 reinforces the existing
//!   memory, 0.75–0.92 replaces it, otherwise the candidate is added.
//! * **Retrieval** – hybrid score `0.5·cosine + 0.3·BM25 + 0.2·recency×importance`
//!   over every memory in scope, read from the in-process [`index`].
#![forbid(unsafe_code)]

pub mod index;
pub mod vectors;

use std::collections::HashSet;

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
use crate::llm::{JsonSchema, LlmRequest, LlmTarget, Message, Usage, collect};
use crate::repo::memories::{self, NewMemory, StoredMemory};
use index::{MemoryIndex, Snapshot};

const MAX_CANDIDATES_PER_NODE: usize = 8;
const OUTPUT_CHARS_FOR_EXTRACTION: usize = 8_000;

/// Nearest neighbours (and as many full-text matches) fetched before ranking.
const ANN_CANDIDATES: i64 = 200;

/// How much a memory learned in another graph counts next to the same
/// memory learned in the graph being worked on.
const OTHER_GRAPH_WEIGHT: f64 = 0.85;

/// Which memories a retrieval looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Everything the reader may see in the workspace.
    All,
    /// One graph and the reader's own user-scope memories (a filter in the UI).
    Only(Uuid),
    /// Everything the reader may see, favouring what this graph learned
    /// itself: work on a graph draws on the rest of the workspace.
    Prefer(Uuid),
}

/// Who is reading: memories of a graph are visible to those who can open the
/// graph, user-scope memories only to their author.
#[derive(Debug, Clone)]
pub struct Reader {
    pub user: Uuid,
    /// Graphs of the workspace the user may work on.
    pub graphs: HashSet<Uuid>,
}

impl Reader {
    fn may_read(&self, m: &StoredMemory) -> bool {
        match m.memory.graph_id {
            Some(graph) => self.graphs.contains(&graph),
            None => m.owner_id == self.user,
        }
    }
}

fn in_view(m: &StoredMemory, view: View) -> bool {
    match view {
        View::All | View::Prefer(_) => true,
        View::Only(graph) => {
            m.memory.graph_id == Some(graph) || m.memory.scope == MemoryScope::User
        }
    }
}

/// Ranks `candidates` for `query` and returns the best `limit` with scores.
/// Only the winners are cloned.
pub fn rank(
    candidates: &[&StoredMemory],
    query: &str,
    limit: usize,
    now: DateTime<Utc>,
    view: View,
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
    let mut scored: Vec<(f64, &StoredMemory)> = candidates
        .iter()
        .zip(lexical)
        .map(|(c, lex)| {
            let age_days = (now - c.memory.updated_at).num_seconds() as f64 / 86_400.0;
            let cosine = f64::from(kernel::dot(&q, &c.embedding));
            let mut score = hybrid_score(cosine, lex, age_days, c.memory.importance);
            if let View::Prefer(home) = view
                && c.memory.graph_id.is_some_and(|g| g != home)
            {
                score *= OTHER_GRAPH_WEIGHT;
            }
            ((score * 1000.0).round() / 1000.0, *c)
        })
        .collect();
    // Only the top `limit` need to be in order.
    let top = limit.min(scored.len());
    if top < scored.len() {
        scored.select_nth_unstable_by(top - 1, |a, b| b.0.total_cmp(&a.0));
        scored.truncate(top);
    }
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored
        .into_iter()
        .map(|(score, c)| Memory {
            score: Some(score),
            ..c.memory.clone()
        })
        .collect()
}

async fn reader(db: &PgPool, user: Uuid, workspace: Uuid) -> Result<Reader, sqlx::Error> {
    let graphs = crate::repo::graphs::accessible_ids(db, user, workspace).await?;
    Ok(Reader {
        user,
        graphs: graphs.into_iter().collect(),
    })
}

/// Most distinct terms of a query that are searched for in the database.
const MAX_QUERY_TERMS: usize = 16;
/// Full-text matches and recent memories fetched before ranking a large workspace.
const TEXT_CANDIDATES: i64 = 200;
const RECENT_CANDIDATES: i64 = 50;
/// A graph's memories read for consolidation; past this, matches are looked up per candidate.
const CONSOLIDATION_WINDOW: usize = 1_000;

/// The terms of `text` as a PostgreSQL `tsquery` that matches any of them:
/// its longest distinct words, which are the rarest and so the cheapest and
/// most telling to look up. Empty when the text has no word worth searching.
pub fn any_term_query(text: &str) -> String {
    any_term_query_without(text, &HashSet::new())
}

/// [`any_term_query`] leaving out the words in `common`: a word that most
/// memories contain matches most of the table and distinguishes nothing.
pub fn any_term_query_without(text: &str, common: &HashSet<String>) -> String {
    let mut terms: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 4)
        .map(str::to_lowercase)
        .filter(|w| !common.contains(w))
        .collect();
    terms.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    terms.dedup();
    terms.truncate(MAX_QUERY_TERMS);
    terms.join(" | ")
}

/// One page of the memories of a workspace that `user` may read, newest
/// first. Read from the database, so it reaches every memory however many
/// the workspace has.
pub async fn visible(
    db: &PgPool,
    user: Uuid,
    workspace: Uuid,
    view: View,
    limit: usize,
    offset: usize,
) -> anyhow::Result<Vec<Memory>> {
    let reader = reader(db, user, workspace).await?;
    let graphs: Vec<Uuid> = reader.graphs.iter().copied().collect();
    let only = match view {
        View::Only(graph) => Some(graph),
        View::All | View::Prefer(_) => None,
    };
    let page = memories::page_readable(
        db,
        workspace,
        &graphs,
        user,
        only,
        i64::try_from(limit).unwrap_or(i64::MAX),
        i64::try_from(offset).unwrap_or(i64::MAX),
    )
    .await?;
    Ok(page.into_iter().map(|m| m.memory).collect())
}

/// One memory of a workspace, if `user` may read it.
pub async fn find(
    db: &PgPool,
    user: Uuid,
    workspace: Uuid,
    id: Uuid,
) -> anyhow::Result<Option<Memory>> {
    let reader = reader(db, user, workspace).await?;
    Ok(memories::find_stored(db, id)
        .await?
        .filter(|m| reader.may_read(m))
        .map(|m| m.memory))
}

/// The candidates a large workspace is ranked from: full-text matches of the
/// query's terms and the most recent memories, both already limited to what
/// the reader may see, plus the nearest neighbours when the database has a
/// vector index. A few hundred rows whatever the size of the workspace.
async fn database_candidates(
    index: &MemoryIndex,
    db: &PgPool,
    reader: &Reader,
    workspace: Uuid,
    query: &str,
) -> anyhow::Result<Vec<StoredMemory>> {
    let graphs: Vec<Uuid> = reader.graphs.iter().copied().collect();
    let common = index.common_terms(db).await;
    let mut found = memories::text_candidates(
        db,
        workspace,
        &graphs,
        reader.user,
        &any_term_query_without(query, &common),
        TEXT_CANDIDATES,
        RECENT_CANDIDATES,
    )
    .await?;
    if index.vector_search() {
        let mut seen: HashSet<Uuid> = found.iter().map(|m| m.memory.id).collect();
        let nearest =
            vectors::candidates(db, workspace, query, &kernel::embed(query), ANN_CANDIDATES)
                .await?;
        found.extend(nearest.into_iter().filter(|m| seen.insert(m.memory.id)));
    }
    Ok(found)
}

/// Hybrid retrieval over the memory of a workspace, limited to what `user`
/// may read. Counts an access for each returned memory in the background, so
/// the caller never waits for that write.
pub async fn retrieve(
    index: &MemoryIndex,
    db: &PgPool,
    user: Uuid,
    workspace: Uuid,
    view: View,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<Memory>> {
    let reader = reader(db, user, workspace).await?;
    // A small workspace is scanned exactly, in process. A large one is narrowed by the
    // database first, so neither memory nor time grows with what the workspace has learned.
    let narrowed;
    let small;
    let pool: &[StoredMemory] = match index.snapshot(db, workspace).await? {
        Snapshot::Small(all) => {
            small = all;
            &small
        }
        Snapshot::Large(_) => {
            narrowed = database_candidates(index, db, &reader, workspace, query).await?;
            &narrowed
        }
    };
    let candidates: Vec<&StoredMemory> = pool
        .iter()
        .filter(|m| reader.may_read(m) && in_view(m, view))
        .collect();
    let ranked = rank(&candidates, query, limit, Utc::now(), view);
    let ids: Vec<Uuid> = ranked.iter().map(|m| m.id).collect();
    if !ids.is_empty() {
        let db = db.clone();
        tokio::spawn(async move {
            if let Err(err) = memories::record_access(&db, &ids).await {
                tracing::warn!(error = %err, "cannot record memory access");
            }
        });
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

/// Asks the LLM for memory candidates from a node's output. Also returns
/// what the call spent, including when its answer could not be used.
pub async fn extract(
    llm: &LlmService,
    target: LlmTarget,
    goal: &str,
    node: &GraphNode,
    output: &str,
) -> anyhow::Result<(Vec<Candidate>, Usage)> {
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
    // An unusable answer still cost tokens; report them with no candidates.
    let Ok(parsed) = serde_json::from_str::<Extraction>(done.text.trim()) else {
        return Ok((Vec::new(), done.usage));
    };
    let candidates = parsed
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
        .collect();
    Ok((candidates, done.usage))
}

/// Consolidates candidates into a graph's memory; returns the decisions taken.
pub async fn store(
    index: &MemoryIndex,
    db: &PgPool,
    owner: Uuid,
    workspace: Option<Uuid>,
    graph_id: Uuid,
    candidates: &[Candidate],
) -> anyhow::Result<Vec<Consolidation>> {
    // Everyone who runs the graph adds to the same memory.
    let mut existing = memories::of_graph(db, graph_id).await?;
    let mut decisions = Vec::with_capacity(candidates.len());
    // A graph that has learned more than one read holds is not compared in full: the
    // memories that share words with the candidate are fetched for it instead.
    let partial = existing.len() >= CONSOLIDATION_WINDOW;
    for c in candidates {
        let embedding = kernel::embed(&c.content);
        if partial {
            let known: HashSet<Uuid> = existing.iter().map(|m| m.memory.id).collect();
            let similar =
                memories::graph_text_matches(db, graph_id, &any_term_query(&c.content), 20).await?;
            existing.extend(
                similar
                    .into_iter()
                    .filter(|m| !known.contains(&m.memory.id)),
            );
        }
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
                if index.vector_search() {
                    vectors::set(db, m.memory.id, &embedding).await?;
                }
                m.memory.content.clone_from(&c.content);
                m.embedding = embedding;
            }
            _ => {
                let new = NewMemory {
                    owner_id: owner,
                    workspace_id: workspace,
                    scope: MemoryScope::Graph,
                    graph_id: Some(graph_id),
                    node_id: None,
                    kind: c.kind,
                    content: &c.content,
                    embedding: &embedding,
                    importance: c.importance,
                };
                let id = memories::insert(db, &new).await?;
                if index.vector_search() {
                    vectors::set(db, id, &embedding).await?;
                }
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
                    owner_id: owner,
                    embedding,
                });
            }
        }
        decisions.push(decision);
    }
    if let Some(workspace) = workspace
        && !decisions.is_empty()
    {
        index.invalidate(workspace);
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
            owner_id: Uuid::nil(),
            embedding: kernel::embed(content),
        }
    }

    #[test]
    fn ranks_relevant_memories_first() {
        let memories = [
            stored("The report must follow APA citation style", 1, 0.8),
            stored("Deploy target is Kubernetes on GKE", 1, 0.8),
            stored("Citation style for the report is APA 7th edition", 200, 0.2),
        ];
        let candidates: Vec<&StoredMemory> = memories.iter().collect();
        let query = "which citation style does the report use";
        let ranked = rank(&candidates, query, 2, Utc::now(), View::All);
        assert_eq!(ranked.len(), 2);
        assert!(ranked[0].content.contains("APA"));
        assert!(ranked[0].score.unwrap() >= ranked[1].score.unwrap());
        assert!(rank(&[], "x", 5, Utc::now(), View::All).is_empty());
        // Asking for fewer results returns a prefix of the full ranking.
        let all = rank(&candidates, query, 10, Utc::now(), View::All);
        assert_eq!(all.len(), 3);
        assert_eq!(ranked[0].id, all[0].id);
        assert_eq!(
            rank(&candidates, query, 1, Utc::now(), View::All)[0].id,
            all[0].id
        );
    }

    #[test]
    fn readers_see_their_graphs_and_their_own_notes() {
        let (mine, theirs, me) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        let mut of_mine = stored("a", 0, 0.5);
        of_mine.memory.graph_id = Some(mine);
        let mut of_theirs = stored("b", 0, 0.5);
        of_theirs.memory.graph_id = Some(theirs);
        let mut my_note = stored("c", 0, 0.5);
        my_note.memory.scope = MemoryScope::User;
        my_note.owner_id = me;
        let mut their_note = my_note.clone();
        their_note.owner_id = Uuid::now_v7();
        let reader = Reader {
            user: me,
            graphs: HashSet::from([mine]),
        };
        assert!(reader.may_read(&of_mine) && reader.may_read(&my_note));
        assert!(
            !reader.may_read(&of_theirs),
            "a graph the reader cannot open"
        );
        assert!(
            !reader.may_read(&their_note),
            "someone else's personal note"
        );
        assert!(in_view(&of_mine, View::Only(mine)) && in_view(&my_note, View::Only(mine)));
        assert!(!in_view(&of_theirs, View::Only(mine)));
        assert!(
            in_view(&of_theirs, View::Prefer(mine)),
            "work draws on the whole workspace"
        );
    }

    #[test]
    fn the_graph_being_worked_on_is_preferred() {
        let home = Uuid::now_v7();
        let mut here = stored("Citation style is APA", 1, 0.5);
        here.memory.graph_id = Some(home);
        let mut elsewhere = stored("Citation style is APA", 1, 0.5);
        elsewhere.memory.graph_id = Some(Uuid::now_v7());
        let candidates = [&elsewhere, &here];
        let ranked = rank(
            &candidates,
            "citation style",
            2,
            Utc::now(),
            View::Prefer(home),
        );
        assert_eq!(ranked[0].id, here.memory.id);
        assert!(ranked[0].score.unwrap() > ranked[1].score.unwrap());
        let flat = rank(&candidates, "citation style", 2, Utc::now(), View::All);
        assert_eq!(flat[0].score, flat[1].score);
    }

    /// Not a correctness test: prints how long ranking a large owner takes.
    #[test]
    #[ignore = "timing; run with --ignored --nocapture"]
    fn ranking_speed() {
        let memories: Vec<StoredMemory> = (0..5_000)
            .map(|i| {
                stored(
                    &format!("fact {i} about topic {} and detail {}", i % 97, i % 13),
                    1,
                    0.5,
                )
            })
            .collect();
        let candidates: Vec<&StoredMemory> = memories.iter().collect();
        let started = std::time::Instant::now();
        for _ in 0..20 {
            assert_eq!(
                rank(
                    &candidates,
                    "detail 7 about topic 42",
                    5,
                    Utc::now(),
                    View::All
                )
                .len(),
                5
            );
        }
        println!(
            "rank over 5000 memories: {:?} per query",
            started.elapsed() / 20
        );
    }
}
