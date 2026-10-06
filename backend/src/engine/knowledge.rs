//! The knowledge base at work: turning an uploaded file into searchable
//! passages, and finding the passages that bear on a question.
//!
//! * **Ingestion** – a worker takes waiting documents, has the agent runtime
//!   parse them into blocks (plain text and Markdown are read here), splits
//!   the blocks into passages and embeds those in batches. Every step is
//!   bounded per document and resumable: a document whose worker went away
//!   returns to the queue.
//! * **Search** – hybrid. Keyword candidates come from the full-text index
//!   (the query's rarest words), a few hundred rows however large the
//!   workspace; they are then ordered by fusing their keyword rank with the
//!   rank of their embedding's similarity to the query.

use std::collections::HashSet;
use std::time::Duration;

use uuid::Uuid;

use crate::app::AppState;
use crate::config::Secret;
use crate::domain::knowledge::{
    BUILTIN_EMBED_MODEL, Block, BlockKind, Chunk, Document, DocumentStatus, KnowledgeSettings,
    Parsed, Passage, chunk,
};
use crate::llm::embeddings::{self, EmbedTarget};
use crate::memory::any_term_query_without;
use crate::repo;
use crate::repo::knowledge::Candidate;

/// Documents ingested at the same time by one instance.
const CONCURRENT_DOCUMENTS: i64 = 3;
/// A document with no progress for this long is taken to have lost its worker.
const STALLED_AFTER_SECS: i64 = 900;
/// Keyword matches ranked per search.
const CANDIDATES: i64 = 300;
/// A word in more than this share of all passages is not searched for.
const COMMON_TERM_SHARE: f64 = 0.02;
/// Reciprocal-rank-fusion constant: how much the very top ranks stand out.
const RRF_K: f64 = 60.0;
const PARSE_TIMEOUT: Duration = Duration::from_secs(900);

/// How a workspace embeds and uses its documents, resolved: its own settings,
/// else the server's embedding endpoint, else the built-in embedding.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub target: EmbedTarget,
    pub passages: usize,
    pub budget_chars: usize,
    pub use_in_nodes: bool,
    pub use_in_plan: bool,
    has_own_key: bool,
    key_hint: Option<String>,
}

impl Resolved {
    /// What members see of the settings.
    pub fn view(&self) -> KnowledgeSettings {
        KnowledgeSettings {
            embed_base_url: self.target.base_url.clone(),
            embed_model: self.target.model.clone(),
            embed_dims: self.target.dims.and_then(|d| i32::try_from(d).ok()),
            has_api_key: self.has_own_key,
            key_hint: self.key_hint.clone(),
            semantic: self.target.is_semantic(),
            passages: i32::try_from(self.passages).unwrap_or(i32::MAX),
            budget_chars: i32::try_from(self.budget_chars).unwrap_or(i32::MAX),
            use_in_nodes: self.use_in_nodes,
            use_in_plan: self.use_in_plan,
        }
    }
}

/// The server's embedding endpoint, if it names one.
fn server_target(state: &AppState) -> EmbedTarget {
    let s = &state.settings;
    match (&s.embed_base_url, &s.embed_model) {
        (Some(base), Some(model)) => EmbedTarget {
            base_url: Some(base.clone()),
            model: model.clone(),
            dims: s.embed_dims,
            api_key: s.embed_api_key.clone(),
        },
        _ => EmbedTarget::builtin(),
    }
}

/// Resolves the knowledge settings of a workspace.
pub async fn settings(state: &AppState, workspace: Uuid) -> anyhow::Result<Resolved> {
    let stored = repo::knowledge::settings(&state.db, workspace).await?;
    let Some(stored) = stored else {
        return Ok(Resolved {
            target: server_target(state),
            passages: 5,
            budget_chars: 6_000,
            use_in_nodes: true,
            use_in_plan: true,
            has_own_key: false,
            key_hint: None,
        });
    };
    let target = match (&stored.embed_base_url, &stored.embed_model) {
        (Some(base), Some(model)) => {
            let api_key = match &stored.api_key_enc {
                Some(sealed) => Some(Secret::new(String::from_utf8(
                    state.secret_box.open(sealed, workspace.as_bytes())?,
                )?)),
                None => None,
            };
            EmbedTarget {
                base_url: Some(base.clone()),
                model: model.clone(),
                dims: stored.embed_dims.and_then(|d| u32::try_from(d).ok()),
                api_key,
            }
        }
        _ => server_target(state),
    };
    Ok(Resolved {
        target,
        passages: usize::try_from(stored.passages).unwrap_or(0),
        budget_chars: usize::try_from(stored.budget_chars).unwrap_or(6_000),
        use_in_nodes: stored.use_in_nodes,
        use_in_plan: stored.use_in_plan,
        has_own_key: stored.api_key_enc.is_some(),
        key_hint: stored.key_hint,
    })
}

// ---------- ingestion ----------

/// Whether the file is read here, without the agent runtime.
fn is_plain_text(name: &str) -> bool {
    let lower = name.to_lowercase();
    [".txt", ".md", ".markdown", ".text"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// Plain text and Markdown as blocks: `#` lines are headings, blank lines
/// separate paragraphs.
pub fn parse_plain_text(text: &str) -> Parsed {
    let mut blocks = Vec::new();
    let mut paragraph = String::new();
    let flush = |paragraph: &mut String, blocks: &mut Vec<Block>| {
        if !paragraph.trim().is_empty() {
            blocks.push(Block {
                kind: BlockKind::Text,
                level: None,
                text: std::mem::take(paragraph),
                page: None,
                rows: None,
                header_rows: None,
            });
        }
        paragraph.clear();
    };
    for line in text.lines() {
        let trimmed = line.trim();
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block {
                kind: BlockKind::Heading,
                level: u8::try_from(hashes).ok(),
                text: trimmed[hashes..].trim().to_owned(),
                page: None,
                rows: None,
                header_rows: None,
            });
        } else if trimmed.is_empty() {
            flush(&mut paragraph, &mut blocks);
        } else {
            if !paragraph.is_empty() {
                paragraph.push(' ');
            }
            paragraph.push_str(trimmed);
        }
    }
    flush(&mut paragraph, &mut blocks);
    Parsed {
        pages: None,
        blocks,
    }
}

/// Has the agent runtime parse a file into blocks.
async fn parse_with_runtime(
    state: &AppState,
    name: &str,
    bytes: Vec<u8>,
) -> anyhow::Result<Parsed> {
    let url = format!(
        "{}/v1/parse",
        state.settings.runtime_url.trim_end_matches('/')
    );
    let response = state
        .http
        .post(&url)
        .query(&[("filename", name)])
        .bearer_auth(state.settings.runtime_token.expose())
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .timeout(PARSE_TIMEOUT)
        .body(bytes)
        .send()
        .await
        .map_err(|err| {
            anyhow::anyhow!(
                "the agent runtime that reads documents is not reachable ({})",
                err.without_url()
            )
        })?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        // The runtime explains in `detail` why a file cannot be read.
        let detail = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["detail"].as_str().map(str::to_owned))
            .unwrap_or_else(|| body.chars().take(200).collect());
        anyhow::bail!("this file could not be read ({status}): {detail}");
    }
    Ok(response.json().await?)
}

async fn parse(state: &AppState, document: &Document) -> anyhow::Result<Parsed> {
    let path = state.settings.documents_dir().join(document.id.to_string());
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| anyhow::anyhow!("the uploaded file is no longer on the server"))?;
    if is_plain_text(&document.name) {
        return Ok(parse_plain_text(&String::from_utf8_lossy(&bytes)));
    }
    parse_with_runtime(state, &document.name, bytes).await
}

/// Embeds every passage of a document that has no vector of the workspace's
/// current model, batch by batch.
async fn embed_document(
    state: &AppState,
    document: &Document,
    target: &EmbedTarget,
) -> anyhow::Result<()> {
    loop {
        let batch = repo::knowledge::unembedded(
            &state.db,
            document.id,
            &target.model,
            embeddings::BATCH as i64,
        )
        .await?;
        if batch.is_empty() {
            return Ok(());
        }
        let texts: Vec<String> = batch
            .iter()
            .map(|c| {
                Chunk {
                    page: None,
                    section_path: c.section_path.clone(),
                    kind: crate::domain::knowledge::ChunkKind::Text,
                    content: c.content.clone(),
                }
                .embed_text(&document.name)
            })
            .collect();
        let vectors = embeddings::embed(&state.http, target, &texts).await?;
        let ids: Vec<Uuid> = batch.iter().map(|c| c.id).collect();
        let bytes: Vec<Vec<u8>> = vectors.iter().map(|v| embeddings::to_bytes(v)).collect();
        repo::knowledge::set_embeddings(&state.db, &ids, &bytes, &target.model).await?;
        repo::knowledge::touch(&state.db, document.id).await?;
    }
}

/// Takes one claimed document from file to searchable passages.
async fn ingest(state: &AppState, document: &Document) -> anyhow::Result<()> {
    let resolved = settings(state, document.workspace_id).await?;
    // A document that already has passages (a change of embedding model) is not parsed again.
    if document.chunk_count == 0 {
        let parsed = parse(state, document).await?;
        let (chunks, cut) = chunk(&parsed);
        anyhow::ensure!(
            !chunks.is_empty(),
            "no text was found in this file (a scan without a text layer?)"
        );
        if cut {
            tracing::warn!(document = %document.id, "document cut at the passage limit");
        }
        let mut tx = state.db.begin().await?;
        repo::knowledge::replace_chunks(
            &mut tx,
            document.id,
            document.workspace_id,
            parsed.pages,
            &chunks,
        )
        .await?;
        tx.commit().await?;
    } else {
        repo::knowledge::set_status(&state.db, document.id, DocumentStatus::Embedding, "").await?;
    }
    embed_document(state, document, &resolved.target).await?;
    repo::knowledge::set_status(&state.db, document.id, DocumentStatus::Ready, "").await?;
    Ok(())
}

/// One round of the ingestion worker: requeue stalled documents, then take
/// and process a few waiting ones side by side.
pub async fn work(state: &AppState) -> anyhow::Result<()> {
    repo::knowledge::requeue_stalled(&state.db, STALLED_AFTER_SECS).await?;
    let claimed = repo::knowledge::claim_pending(&state.db, CONCURRENT_DOCUMENTS).await?;
    let jobs = claimed.into_iter().map(|document| async move {
        if let Err(err) = ingest(state, &document).await {
            tracing::warn!(document = %document.id, error = %err, "document ingestion failed");
            let message: String = err.to_string().chars().take(500).collect();
            let failed = DocumentStatus::Failed;
            if let Err(err) =
                repo::knowledge::set_status(&state.db, document.id, failed, &message).await
            {
                tracing::error!(document = %document.id, error = %err, "cannot record the failure");
            }
        }
    });
    futures::future::join_all(jobs).await;
    Ok(())
}

// ---------- search ----------

/// Orders keyword candidates by fusing two ranks: the full-text rank they
/// arrived in, and the rank of their similarity to the query's embedding
/// (for those embedded with the same model). Scores are 0-1.
pub fn fuse(
    candidates: Vec<Candidate>,
    query: Option<(&str, &[f32])>,
    limit: usize,
) -> Vec<Passage> {
    let similarity: Vec<Option<f32>> = candidates
        .iter()
        .map(|c| {
            let (model, vector) = query?;
            if c.embedding_model.as_deref() != Some(model) {
                return None;
            }
            embeddings::dot_bytes(c.embedding.as_deref()?, vector)
        })
        .collect();
    // Rank by similarity among those that have one.
    let mut by_similarity: Vec<usize> = (0..candidates.len())
        .filter(|i| similarity[*i].is_some())
        .collect();
    by_similarity.sort_by(|a, b| {
        similarity[*b]
            .unwrap_or(0.0)
            .total_cmp(&similarity[*a].unwrap_or(0.0))
    });
    let mut semantic_rank = vec![None; candidates.len()];
    for (rank, index) in by_similarity.into_iter().enumerate() {
        semantic_rank[index] = Some(rank);
    }
    let best = 2.0 / (RRF_K + 1.0);
    let mut scored: Vec<(f64, Candidate)> = candidates
        .into_iter()
        .enumerate()
        .map(|(keyword_rank, c)| {
            let mut score = 1.0 / (RRF_K + 1.0 + keyword_rank as f64);
            if let Some(rank) = semantic_rank[keyword_rank] {
                score += 1.0 / (RRF_K + 1.0 + rank as f64);
            }
            (score / best, c)
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored
        .into_iter()
        .take(limit)
        .map(|(score, c)| Passage {
            chunk_id: c.chunk_id,
            document_id: c.document_id,
            document_name: c.document_name,
            page: c.page,
            section_path: c.section_path,
            kind: c.kind,
            content: c.content,
            score: (score * 1000.0).round() / 1000.0,
        })
        .collect()
}

/// The passages of a workspace's documents that best answer `query`.
pub async fn search(
    state: &AppState,
    workspace: Uuid,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<Passage>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let common: HashSet<String> = repo::knowledge::common_terms(&state.db, COMMON_TERM_SHARE)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect();
    let tsquery = any_term_query_without(query, &common);
    let candidates =
        repo::knowledge::keyword_candidates(&state.db, workspace, &tsquery, CANDIDATES).await?;
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let resolved = settings(state, workspace).await?;
    // The query is embedded only when some candidate can be compared with it. A failing
    // embeddings endpoint degrades the search to keywords; it does not fail it.
    let comparable = candidates
        .iter()
        .any(|c| c.embedding_model.as_deref() == Some(resolved.target.model.as_str()));
    let vector = if comparable {
        match embeddings::embed(&state.http, &resolved.target, &[query.to_owned()]).await {
            Ok(mut vectors) => vectors.pop(),
            Err(err) => {
                tracing::warn!(error = %err, "query embedding failed; ranking by keywords only");
                None
            }
        }
    } else {
        None
    };
    let query_vector = vector
        .as_deref()
        .map(|v| (resolved.target.model.as_str(), v));
    Ok(fuse(candidates, query_vector, limit))
}

/// What documents are used for, which a workspace can turn off separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Use {
    Node,
    Plan,
}

/// The passages to put before a model for `query`, each with its citation,
/// within the workspace's passage count and character budget. Empty when the
/// workspace turned documents off for this use, or has none that match;
/// never an error: a prompt is built with or without documents.
pub async fn context(state: &AppState, workspace: Uuid, query: &str, purpose: Use) -> Vec<String> {
    let resolved = match settings(state, workspace).await {
        Ok(resolved) => resolved,
        Err(err) => {
            tracing::warn!(error = %err, "knowledge settings unreadable; no documents used");
            return Vec::new();
        }
    };
    let enabled = match purpose {
        Use::Node => resolved.use_in_nodes,
        Use::Plan => resolved.use_in_plan,
    };
    if !enabled || resolved.passages == 0 {
        return Vec::new();
    }
    let passages = match search(state, workspace, query, resolved.passages).await {
        Ok(passages) => passages,
        Err(err) => {
            tracing::warn!(error = %err, "document search failed; no documents used");
            return Vec::new();
        }
    };
    let mut left = resolved.budget_chars;
    let mut out = Vec::new();
    for passage in passages {
        if left < 200 {
            break;
        }
        let content: String = passage.content.chars().take(left).collect();
        left = left.saturating_sub(content.chars().count());
        out.push(format!("[{}]\n{}", passage.citation(), content));
    }
    out
}

/// Whether `model` is the built-in embedding.
pub fn is_builtin(model: &str) -> bool {
    model == BUILTIN_EMBED_MODEL
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::knowledge::ChunkKind;

    fn candidate(content: &str, vector: Option<&[f32]>, model: &str) -> Candidate {
        Candidate {
            chunk_id: Uuid::now_v7(),
            document_id: Uuid::now_v7(),
            document_name: "doc.pdf".into(),
            page: Some(3),
            section_path: "A › B".into(),
            kind: ChunkKind::Text,
            content: content.into(),
            embedding: vector.map(embeddings::to_bytes),
            embedding_model: Some(model.into()),
        }
    }

    #[test]
    fn plain_text_becomes_headings_and_paragraphs() {
        let parsed =
            parse_plain_text("# Title\n\nFirst line\nsecond line\n\n## Part\nBody\n#hashtag");
        let seen: Vec<(BlockKind, Option<u8>, &str)> = parsed
            .blocks
            .iter()
            .map(|b| (b.kind, b.level, b.text.as_str()))
            .collect();
        assert_eq!(
            seen,
            vec![
                (BlockKind::Heading, Some(1), "Title"),
                (BlockKind::Text, None, "First line second line"),
                (BlockKind::Heading, Some(2), "Part"),
                (BlockKind::Text, None, "Body #hashtag"),
            ]
        );
    }

    #[test]
    fn meaning_can_lift_a_passage_over_a_better_keyword_match() {
        // Keyword order: a, b, c. The query's vector is closest to c.
        let candidates = vec![
            candidate("a", Some(&[0.0, 1.0]), "m"),
            candidate("b", Some(&[0.6, 0.8]), "m"),
            candidate("c", Some(&[1.0, 0.0]), "m"),
        ];
        let ranked = fuse(candidates, Some(("m", &[1.0, 0.0])), 3);
        let order: Vec<&str> = ranked.iter().map(|p| p.content.as_str()).collect();
        // a: keyword 1st + semantic 3rd; c: keyword 3rd + semantic 1st; b: 2nd + 2nd.
        assert_eq!(order.len(), 3);
        assert!(
            (ranked[0].score - ranked[1].score).abs() < 0.01,
            "a and c tie closely"
        );
        assert_eq!(ranked[0].citation(), "doc.pdf, p. 3 › A › B");
        assert!(ranked.iter().all(|p| p.score > 0.0 && p.score <= 1.0));
    }

    #[test]
    fn vectors_of_another_model_are_not_compared() {
        let candidates = vec![
            candidate("old model", Some(&[1.0, 0.0]), "old"),
            candidate("current model", Some(&[0.9, 0.1]), "m"),
        ];
        let ranked = fuse(candidates, Some(("m", &[1.0, 0.0])), 2);
        // The second has both ranks, the first only its keyword rank.
        assert_eq!(ranked[0].content, "current model");
        // Without a query vector the keyword order stands.
        let candidates = vec![
            candidate("first", None, "m"),
            candidate("second", None, "m"),
        ];
        let ranked = fuse(candidates, None, 1);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].content, "first");
    }
}
