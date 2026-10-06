//! The knowledge base of a workspace: documents, the passages they are split
//! into, and how a parsed document becomes passages.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::string_enum;

pub const DOCUMENT_NAME_MAX: usize = 255;
pub const DOCUMENT_MAX_BYTES: usize = 50 * 1024 * 1024;
pub const DOCUMENTS_MAX: i64 = 50_000;
/// Passages kept per document; a longer document is cut there and says so.
pub const CHUNKS_MAX: usize = 20_000;
/// Characters a passage aims for (about 400 tokens) and may not exceed.
pub const CHUNK_TARGET_CHARS: usize = 1_600;
pub const CHUNK_MAX_CHARS: usize = 2_400;
/// The embedding used when no embeddings endpoint is configured.
pub const BUILTIN_EMBED_MODEL: &str = "builtin-hash-256";

string_enum!(
    /// Where a document is on its way into the knowledge base.
    DocumentStatus {
        Pending => "pending",
        Parsing => "parsing",
        Embedding => "embedding",
        Ready => "ready",
        Failed => "failed",
    }
);

string_enum!(
    /// What a passage was made from.
    ChunkKind {
        Text => "text",
        Table => "table",
    }
);

/// An uploaded document.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Document {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub size_bytes: i64,
    pub status: DocumentStatus,
    /// Why it failed; empty otherwise.
    pub error: String,
    /// Pages, slides or sheets, when the format has them.
    #[schema(required = true)]
    pub page_count: Option<i32>,
    /// Passages it was split into.
    pub chunk_count: i32,
    #[schema(required = true)]
    pub uploaded_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A passage found by a search, with where it comes from.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Passage {
    pub chunk_id: Uuid,
    pub document_id: Uuid,
    pub document_name: String,
    #[schema(required = true)]
    pub page: Option<i32>,
    pub section_path: String,
    pub kind: ChunkKind,
    pub content: String,
    /// The topic the passage was grouped under, once topics exist.
    #[schema(required = true)]
    pub topic_id: Option<Uuid>,
    /// 0-1, higher is better; comparable within one search only.
    pub score: f64,
}

/// A topic of a workspace's documents, found by clustering their passages.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Topic {
    pub id: Uuid,
    /// The words that set the topic apart: "customs · invoices · port".
    pub label: String,
    pub terms: Vec<String>,
    /// Passages grouped under it.
    pub chunk_count: i32,
}

impl Passage {
    /// "Report.pdf, p. 12 › Revenue › By region": where a reader finds the passage.
    pub fn citation(&self) -> String {
        let mut out = self.document_name.clone();
        if let Some(page) = self.page {
            out.push_str(&format!(", p. {page}"));
        }
        if !self.section_path.is_empty() {
            out.push_str(&format!(" › {}", self.section_path));
        }
        out
    }
}

/// How a workspace embeds and uses its documents, as its members see it
/// (the key is never returned, only a hint of it).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct KnowledgeSettings {
    /// The OpenAI-compatible embeddings endpoint in use; `null` for the built-in embedding.
    #[schema(required = true)]
    pub embed_base_url: Option<String>,
    /// The embedding model in use (`builtin-hash-256` when none is configured).
    pub embed_model: String,
    #[schema(required = true)]
    pub embed_dims: Option<i32>,
    /// Whether the workspace stored its own key, and its last characters.
    pub has_api_key: bool,
    #[schema(required = true)]
    pub key_hint: Option<String>,
    /// True when the embedding understands meaning; the built-in one only matches words.
    pub semantic: bool,
    /// Passages given to a node or to the planner (0 turns documents off for them).
    pub passages: i32,
    /// Characters those passages may take together.
    pub budget_chars: i32,
    pub use_in_nodes: bool,
    pub use_in_plan: bool,
}

/// One block of a parsed document, as the agent runtime returns it.
#[derive(Debug, Clone, Deserialize)]
pub struct Block {
    pub kind: BlockKind,
    #[serde(default)]
    pub level: Option<u8>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub page: Option<i32>,
    #[serde(default)]
    pub rows: Option<Vec<Vec<String>>>,
    #[serde(default)]
    pub header_rows: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    Heading,
    Text,
    Table,
}

/// A parsed document.
#[derive(Debug, Clone, Deserialize)]
pub struct Parsed {
    #[serde(default)]
    pub pages: Option<i32>,
    pub blocks: Vec<Block>,
}

/// A passage ready to be stored.
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub page: Option<i32>,
    pub section_path: String,
    pub kind: ChunkKind,
    pub content: String,
}

impl Chunk {
    /// What is embedded: the passage with where it sits, so that it means
    /// something on its own ("Q2" is nothing without "Revenue › By region").
    pub fn embed_text(&self, document: &str) -> String {
        if self.section_path.is_empty() {
            format!("{document}\n{}", self.content)
        } else {
            format!("{document} › {}\n{}", self.section_path, self.content)
        }
    }
}

fn squeeze(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Cuts `text` into pieces of at most `max` characters, at sentence ends or
/// spaces where it can.
fn split_long(text: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text.trim();
    while rest.chars().count() > max {
        let cut = rest.char_indices().nth(max).map_or(rest.len(), |(i, _)| i);
        let window = &rest[..cut];
        let at = window
            .rfind(". ")
            .map(|i| i + 1)
            .filter(|i| *i > cut / 2)
            .or_else(|| window.rfind(' ').filter(|i| *i > cut / 2))
            .unwrap_or(cut);
        out.push(rest[..at].trim().to_owned());
        rest = rest[at..].trim_start();
    }
    if !rest.is_empty() {
        out.push(rest.to_owned());
    }
    out
}

/// The label of each column of a table: its header cells from top to bottom,
/// joined, so that a two-row header "2024 | Q1" names the column "2024 / Q1".
/// A header cell left empty under a merged cell takes the value to its left.
fn column_labels(rows: &[Vec<String>], header_rows: usize, width: usize) -> Vec<String> {
    let mut labels = vec![String::new(); width];
    for row in rows.iter().take(header_rows) {
        let mut carried = String::new();
        for (col, label) in labels.iter_mut().enumerate() {
            let cell = squeeze(row.get(col).map_or("", String::as_str));
            let cell = if cell.is_empty() {
                carried.clone()
            } else {
                carried.clone_from(&cell);
                cell
            };
            if !cell.is_empty() && !label.split(" / ").any(|part| part == cell) {
                if !label.is_empty() {
                    label.push_str(" / ");
                }
                label.push_str(&cell);
            }
        }
    }
    labels
}

/// A table as passages: every data row becomes one line of `label: value`
/// pairs, so a row is readable without the header that may be pages above it.
fn table_chunks(block: &Block, section: &str, out: &mut Vec<Chunk>) {
    let Some(rows) = block.rows.as_deref().filter(|r| !r.is_empty()) else {
        return;
    };
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 {
        return;
    }
    let header_rows = block
        .header_rows
        .unwrap_or(1)
        .min(rows.len().saturating_sub(1));
    let labels = column_labels(rows, header_rows, width);
    let mut current = String::new();
    let flush = |current: &mut String, out: &mut Vec<Chunk>| {
        if !current.trim().is_empty() {
            out.push(Chunk {
                page: block.page,
                section_path: section.to_owned(),
                kind: ChunkKind::Table,
                content: std::mem::take(current).trim_end().to_owned(),
            });
        }
    };
    for row in rows.iter().skip(header_rows) {
        let cells: Vec<String> = (0..width)
            .filter_map(|col| {
                let value = squeeze(row.get(col).map_or("", String::as_str));
                if value.is_empty() {
                    return None;
                }
                Some(match labels[col].as_str() {
                    "" => value,
                    label => format!("{label}: {value}"),
                })
            })
            .collect();
        if cells.is_empty() {
            continue;
        }
        let mut line = cells.join("; ");
        if line.chars().count() > CHUNK_MAX_CHARS {
            line = line.chars().take(CHUNK_MAX_CHARS).collect();
        }
        if !current.is_empty()
            && current.chars().count() + line.chars().count() > CHUNK_TARGET_CHARS
        {
            flush(&mut current, out);
        }
        current.push_str(&line);
        current.push('\n');
    }
    flush(&mut current, out);
}

/// Splits a parsed document into passages: text is gathered under its
/// headings up to [`CHUNK_TARGET_CHARS`], a new heading starts a new
/// passage, and tables become row-wise passages. At most [`CHUNKS_MAX`]
/// passages are returned; the flag says whether the document was cut there.
pub fn chunk(parsed: &Parsed) -> (Vec<Chunk>, bool) {
    let mut out = Vec::new();
    // The headings above the current position, by level.
    let mut headings: Vec<(u8, String)> = Vec::new();
    let mut section = String::new();
    let mut current = String::new();
    let mut page = None;
    fn flush(current: &mut String, page: Option<i32>, section: &str, out: &mut Vec<Chunk>) {
        if !current.trim().is_empty() {
            out.push(Chunk {
                page,
                section_path: section.to_owned(),
                kind: ChunkKind::Text,
                content: std::mem::take(current).trim().to_owned(),
            });
        }
        current.clear();
    }
    for block in &parsed.blocks {
        if out.len() >= CHUNKS_MAX {
            break;
        }
        match block.kind {
            BlockKind::Heading => {
                flush(&mut current, page, &section, &mut out);
                let text = squeeze(&block.text);
                if text.is_empty() {
                    continue;
                }
                let level = block.level.unwrap_or(1).clamp(1, 6);
                headings.retain(|(l, _)| *l < level);
                headings.push((level, text.chars().take(120).collect()));
                section = headings
                    .iter()
                    .map(|(_, t)| t.as_str())
                    .collect::<Vec<_>>()
                    .join(" › ");
            }
            BlockKind::Text => {
                let text = squeeze(&block.text);
                if text.is_empty() {
                    continue;
                }
                for piece in split_long(&text, CHUNK_MAX_CHARS) {
                    if !current.is_empty()
                        && current.chars().count() + piece.chars().count() > CHUNK_TARGET_CHARS
                    {
                        flush(&mut current, page, &section, &mut out);
                    }
                    if current.is_empty() {
                        page = block.page;
                    } else {
                        current.push('\n');
                    }
                    current.push_str(&piece);
                }
            }
            BlockKind::Table => {
                flush(&mut current, page, &section, &mut out);
                table_chunks(block, &section, &mut out);
            }
        }
    }
    flush(&mut current, page, &section, &mut out);
    let cut = out.len() > CHUNKS_MAX;
    out.truncate(CHUNKS_MAX);
    (out, cut)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heading(level: u8, text: &str) -> Block {
        Block {
            kind: BlockKind::Heading,
            level: Some(level),
            text: text.into(),
            page: Some(1),
            rows: None,
            header_rows: None,
        }
    }

    fn text(text: &str, page: i32) -> Block {
        Block {
            kind: BlockKind::Text,
            level: None,
            text: text.into(),
            page: Some(page),
            rows: None,
            header_rows: None,
        }
    }

    #[test]
    fn passages_follow_the_headings_they_sit_under() {
        let parsed = Parsed {
            pages: Some(2),
            blocks: vec![
                heading(1, "Report"),
                text("Intro   one.", 1),
                text("Intro two.", 1),
                heading(2, "Revenue"),
                text("Up  10%.", 2),
                heading(2, "Costs"),
                text("Flat.", 2),
                heading(1, "Appendix"),
                text("Notes.", 2),
            ],
        };
        let (chunks, cut) = chunk(&parsed);
        assert!(!cut);
        let seen: Vec<(&str, &str, Option<i32>)> = chunks
            .iter()
            .map(|c| (c.section_path.as_str(), c.content.as_str(), c.page))
            .collect();
        assert_eq!(
            seen,
            vec![
                ("Report", "Intro one.\nIntro two.", Some(1)),
                ("Report › Revenue", "Up 10%.", Some(2)),
                ("Report › Costs", "Flat.", Some(2)),
                ("Appendix", "Notes.", Some(2)),
            ]
        );
        assert_eq!(
            chunks[1].embed_text("q3.pdf"),
            "q3.pdf › Report › Revenue\nUp 10%."
        );
    }

    #[test]
    fn long_text_is_cut_at_sentences_and_never_exceeds_the_limit() {
        let sentence = "This sentence is exactly forty chars long. ";
        let parsed = Parsed {
            pages: None,
            blocks: vec![text(&sentence.repeat(200), 1)],
        };
        let (chunks, _) = chunk(&parsed);
        assert!(chunks.len() > 3);
        for c in &chunks {
            assert!(
                c.content.chars().count() <= CHUNK_MAX_CHARS,
                "{}",
                c.content.len()
            );
            assert!(
                c.content.ends_with('.'),
                "cut at a sentence end: …{}",
                &c.content[c.content.len() - 20..]
            );
        }
        let total: usize = chunks
            .iter()
            .map(|c| c.content.matches("forty").count())
            .sum();
        assert_eq!(total, 200, "nothing is lost");
    }

    #[test]
    fn a_table_row_carries_its_multi_row_header() {
        let rows = |r: &[&[&str]]| -> Vec<Vec<String>> {
            r.iter()
                .map(|row| row.iter().map(|c| (*c).to_owned()).collect())
                .collect()
        };
        let table = Block {
            kind: BlockKind::Table,
            level: None,
            text: String::new(),
            page: Some(7),
            rows: Some(rows(&[
                &["Region", "2024", "", "2025"],
                &["", "Q1", "Q2", "Q1"],
                &["EMEA", "10", "12", "15"],
                &["", "", "", ""],
                &["APAC", "7", "", "9"],
            ])),
            header_rows: Some(2),
        };
        let parsed = Parsed {
            pages: Some(7),
            blocks: vec![heading(1, "Sales"), table],
        };
        let (chunks, _) = chunk(&parsed);
        assert_eq!(chunks.len(), 1);
        assert_eq!(
            (
                chunks[0].kind,
                chunks[0].page,
                chunks[0].section_path.as_str()
            ),
            (ChunkKind::Table, Some(7), "Sales")
        );
        assert_eq!(
            chunks[0].content,
            "Region: EMEA; 2024 / Q1: 10; 2024 / Q2: 12; 2025 / Q1: 15\n\
             Region: APAC; 2024 / Q1: 7; 2025 / Q1: 9"
        );
    }

    #[test]
    fn a_large_table_is_split_between_rows() {
        let mut rows = vec![vec!["Name".to_owned(), "Note".to_owned()]];
        for i in 0..100 {
            rows.push(vec![format!("item {i}"), "x".repeat(60)]);
        }
        let table = Block {
            kind: BlockKind::Table,
            level: None,
            text: String::new(),
            page: None,
            rows: Some(rows),
            header_rows: Some(1),
        };
        let (chunks, _) = chunk(&Parsed {
            pages: None,
            blocks: vec![table],
        });
        assert!(chunks.len() > 3);
        assert!(
            chunks
                .iter()
                .all(|c| c.content.chars().count() <= CHUNK_MAX_CHARS)
        );
        assert!(chunks.iter().all(|c| c.content.starts_with("Name: item ")));
        let lines: usize = chunks.iter().map(|c| c.content.lines().count()).sum();
        assert_eq!(lines, 100);
    }
}
