//! Evidence-bound outputs (`docs/AGENTSECOPS.md` §2): every piece of context
//! a node is given carries a citation id the model is told to cite after
//! each claim that rests on it: `[doc:<chunk id>]` for a document passage,
//! `[mem:<memory id>]` for a memory, `[node:<node id>]` for an upstream
//! output and `[fact:<id>]` for an ontology fact. A claim that rests on
//! nothing is declared the model's own with `[assumption]`.
//!
//! The verifier here is deterministic and pure: it finds the citations in an
//! output (ids inside code are not citations), checks each against what the
//! node was given (an id the node never saw is forged), approximates the
//! claims as the sentences and bullets of the prose, and reads a coverage
//! figure and a verdict off them.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::string_enum;

string_enum!(
    /// What a citation points at.
    CitationKind {
        /// A passage of a document of the knowledge base (`document_chunks.id`).
        Doc => "doc",
        /// A memory (`memories.id`).
        Mem => "mem",
        /// The output of an upstream node (`nodes.id`).
        Node => "node",
        /// A fact of the audit's ontology (its entity id).
        Fact => "fact",
    }
);

/// A citation as written: `[doc:0199…]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Citation {
    pub kind: CitationKind,
    /// The id after the colon, as written.
    pub id: String,
}

impl Citation {
    /// A citation of a thing by its uuid.
    pub fn of(kind: CitationKind, id: Uuid) -> Self {
        Citation {
            kind,
            id: id.to_string(),
        }
    }

    /// The id as written in a prompt and an output: `[doc:<id>]`.
    pub fn render(&self) -> String {
        format!("[{}:{}]", self.kind, self.id)
    }

    /// `doc:<id>`, how the id is stored and listed.
    pub fn key(&self) -> String {
        format!("{}:{}", self.kind, self.id)
    }

    /// Reads one `kind:id` token; `None` when it is not one.
    pub fn parse(token: &str) -> Option<Citation> {
        let (kind, id) = token.split_once(':')?;
        let kind = kind.parse::<CitationKind>().ok()?;
        if id.is_empty() || id.len() > ID_MAX || !id.chars().all(is_id_char) {
            return None;
        }
        Some(Citation {
            kind,
            id: id.to_owned(),
        })
    }
}

impl std::fmt::Display for Citation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.key())
    }
}

/// Longest id a citation may carry.
pub const ID_MAX: usize = 120;

fn is_id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/')
}

/// What a node was given to read, by id: the set its citations are checked
/// against. Carried on the `data_read` span (`citation_ids`) and used by the
/// verifier once the node answered.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Given {
    /// Chunk ids of the document passages.
    pub documents: Vec<Uuid>,
    pub memories: Vec<Uuid>,
    /// The upstream nodes whose outputs were given.
    pub nodes: Vec<Uuid>,
    /// Ontology facts, by their entity id (`agent:…`, `document:…`).
    pub facts: Vec<String>,
}

impl Given {
    /// Whether the node was given anything at all.
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
            && self.memories.is_empty()
            && self.nodes.is_empty()
            && self.facts.is_empty()
    }

    /// How many things were given.
    pub fn len(&self) -> usize {
        self.documents.len() + self.memories.len() + self.nodes.len() + self.facts.len()
    }

    /// Whether `citation` names something the node was given.
    pub fn contains(&self, citation: &Citation) -> bool {
        let uuid = || citation.id.parse::<Uuid>().ok();
        match citation.kind {
            CitationKind::Doc => uuid().is_some_and(|id| self.documents.contains(&id)),
            CitationKind::Mem => uuid().is_some_and(|id| self.memories.contains(&id)),
            CitationKind::Node => uuid().is_some_and(|id| self.nodes.contains(&id)),
            CitationKind::Fact => self.facts.contains(&citation.id),
        }
    }

    /// Every id as written in a prompt, documents first.
    pub fn citations(&self) -> Vec<Citation> {
        let mut out = Vec::with_capacity(self.len());
        out.extend(
            self.documents
                .iter()
                .map(|id| Citation::of(CitationKind::Doc, *id)),
        );
        out.extend(
            self.memories
                .iter()
                .map(|id| Citation::of(CitationKind::Mem, *id)),
        );
        out.extend(
            self.nodes
                .iter()
                .map(|id| Citation::of(CitationKind::Node, *id)),
        );
        out.extend(self.facts.iter().map(|id| Citation {
            kind: CitationKind::Fact,
            id: id.clone(),
        }));
        out
    }

    /// The ids as `kind:id` strings (what the `data_read` span lists).
    pub fn keys(&self) -> Vec<String> {
        self.citations().iter().map(Citation::key).collect()
    }
}

/// The heading of a document passage in a prompt: `[doc:<chunk id>] <source>`.
pub fn document_head(chunk_id: Uuid, source: &str) -> String {
    format!(
        "{} {source}",
        Citation::of(CitationKind::Doc, chunk_id).render()
    )
}

/// A memory as given to a model: `[mem:<id>] <content>`.
pub fn memory_line(id: Uuid, content: &str) -> String {
    format!("{} {content}", Citation::of(CitationKind::Mem, id).render())
}

/// Splits a prompt line of the form `[kind:id] rest` into its citation and the rest.
pub fn split_head(line: &str) -> Option<(Citation, &str)> {
    let rest = line.strip_prefix('[')?;
    let (token, rest) = rest.split_once(']')?;
    let citation = Citation::parse(token)?;
    Some((citation, rest.trim_start()))
}

/// How a model is told to cite, in every prompt that carries ids.
pub const CITATION_INSTRUCTION: &str = "Every document passage, memory and upstream result you \
    are given carries an id in brackets, such as [doc:…], [mem:…] or [node:…]. After each claim \
    that rests on one of them, cite its id in brackets exactly as given; several ids may share \
    one bracket, separated by commas. Say explicitly when a statement is your own reasoning or an \
    assumption by ending it with [assumption]. Never invent an id, and never cite what you were \
    not given.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn citations_render_parse_and_resolve_against_what_was_given() {
        let chunk = Uuid::now_v7();
        let memory = Uuid::now_v7();
        let node = Uuid::now_v7();
        let given = Given {
            documents: vec![chunk],
            memories: vec![memory],
            nodes: vec![node],
            facts: vec!["document:0199".into()],
        };
        assert_eq!(given.len(), 4);
        assert!(!given.is_empty() && Given::default().is_empty());
        let doc = Citation::of(CitationKind::Doc, chunk);
        assert_eq!(doc.render(), format!("[doc:{chunk}]"));
        assert_eq!(doc.key(), format!("doc:{chunk}"));
        assert_eq!(Citation::parse(&format!("doc:{chunk}")), Some(doc.clone()));
        assert!(given.contains(&doc));
        assert!(given.contains(&Citation::of(CitationKind::Mem, memory)));
        assert!(given.contains(&Citation::of(CitationKind::Node, node)));
        assert!(given.contains(&Citation::parse("fact:document:0199").unwrap()));
        // Another id of the right kind, or the right id of the wrong kind, is not given.
        assert!(!given.contains(&Citation::of(CitationKind::Doc, Uuid::now_v7())));
        assert!(!given.contains(&Citation::of(CitationKind::Mem, chunk)));
        assert!(!given.contains(&Citation::parse("fact:document:0200").unwrap()));
        assert_eq!(given.keys().len(), 4);
        assert!(given.keys()[0].starts_with("doc:"));
        assert!(given.keys()[3].starts_with("fact:"));

        assert!(Citation::parse("doc:").is_none());
        assert!(Citation::parse("doc").is_none());
        assert!(Citation::parse("url:https://x").is_none());
        assert!(Citation::parse("doc:has space").is_none());
        assert!(Citation::parse(&format!("doc:{}", "x".repeat(ID_MAX + 1))).is_none());
        assert_eq!(
            Citation::parse("node:not-a-uuid").map(|c| c.kind),
            Some(CitationKind::Node),
            "a malformed id is still a citation, an unknown one"
        );
    }

    #[test]
    fn prompt_heads_carry_the_id_before_the_source() {
        let chunk = Uuid::now_v7();
        let head = document_head(chunk, "customs.pdf, p. 17");
        assert_eq!(head, format!("[doc:{chunk}] customs.pdf, p. 17"));
        let (citation, source) = split_head(&head).unwrap();
        assert_eq!(citation, Citation::of(CitationKind::Doc, chunk));
        assert_eq!(source, "customs.pdf, p. 17");
        let memory = Uuid::now_v7();
        let line = memory_line(memory, "The team writes in British English.");
        let (citation, text) = split_head(&line).unwrap();
        assert_eq!(citation.kind, CitationKind::Mem);
        assert_eq!(text, "The team writes in British English.");
        assert!(split_head("[customs.pdf, p. 17] text").is_none());
        assert!(split_head("no head").is_none());
        assert!(CITATION_INSTRUCTION.contains("[assumption]"));
    }
}
