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
/// Most ids remembered as seen inside a node's context.
pub const SEEN_MAX: usize = 256;

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
    /// Ids that appear inside the text the node was given (an upstream output
    /// citing its own sources, a memory quoting a passage), as `kind:id`:
    /// citing one is not forging, since the node saw it; they are not what
    /// the node was given and do not count toward `len`.
    #[serde(default)]
    pub seen: Vec<String>,
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

    /// Whether `citation` names something the node was given, or saw in it.
    pub fn contains(&self, citation: &Citation) -> bool {
        let uuid = || citation.id.parse::<Uuid>().ok();
        let given = match citation.kind {
            CitationKind::Doc => uuid().is_some_and(|id| self.documents.contains(&id)),
            CitationKind::Mem => uuid().is_some_and(|id| self.memories.contains(&id)),
            CitationKind::Node => uuid().is_some_and(|id| self.nodes.contains(&id)),
            CitationKind::Fact => self.facts.contains(&citation.id),
        };
        given || self.seen.contains(&citation.key())
    }

    /// Notes every citation written inside `text` (what the node was given)
    /// as seen, at most [`SEEN_MAX`] of them.
    pub fn see(&mut self, text: &str) {
        for citation in citations_in(text) {
            let key = citation.key();
            if self.seen.len() < SEEN_MAX && !self.seen.contains(&key) {
                self.seen.push(key);
            }
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

// ─────────────────────────────── the verifier ───────────────────────────────

string_enum!(
    /// How much of an output rests on what the node was given.
    EvidenceVerdict {
        /// Enough of the claims cite what the node was given (the policy's share, 0.8 unless
        /// set) and no cited id is one the node never saw.
        Verified => "verified",
        /// Some claims cite what was given and others nothing, or an id the node never saw.
        Partial => "partial",
        /// No claim rests on what the node was given.
        Unverified => "unverified",
    }
);

/// The share of claims that must cite what was given for an output to be
/// verified when no policy says otherwise.
pub const MIN_COVERAGE_DEFAULT: f64 = 0.8;
/// Most ids listed as cited or unknown.
pub const IDS_MAX: usize = 64;
/// Markers that declare a claim the model's own; counted, never cited.
pub const DECLARED_MARKERS: [&str; 3] = ["[assumption]", "[own reasoning]", "[inference]"];
/// Headings (by their first word) under which the ids are listed rather than
/// claimed: `Sources`, `References`, `Sources to consult`, …
const SOURCE_HEADINGS: [&str; 4] = ["sources", "references", "citations", "bibliography"];
/// Words a sentence needs to count as a claim; a bullet or a table row needs one.
const SENTENCE_WORDS: usize = 2;

/// What the verifier found about an output: stored on the node run
/// (`NodeRun.evidence`) and summarised on its last `llm_call` span.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Evidence {
    pub verdict: EvidenceVerdict,
    /// Claims citing what the node was given, over the claims not declared the
    /// model's own; 0 to 1.
    pub coverage: f64,
    /// The sentences, bullets and table rows of the prose (headings, quotes,
    /// code and the sources section aside).
    pub claims: usize,
    /// Claims that cite at least one thing the node was given.
    pub cited_claims: usize,
    /// Claims the model declared its own (`[assumption]`) and cited nothing for.
    pub declared_claims: usize,
    /// Distinct ids cited that the node was given (`doc:…`), first cited first.
    pub cited: Vec<String>,
    /// Distinct ids cited that the node was never given: forged, or another run's.
    pub unknown: Vec<String>,
    /// How many things the node was given.
    pub given: usize,
    /// The coverage a verified output needed under the policy in force.
    pub min_coverage: f64,
}

impl Evidence {
    /// The claims that count toward the coverage: all but the declared ones.
    pub fn counted_claims(&self) -> usize {
        self.claims.saturating_sub(self.declared_claims)
    }

    /// Coverage as a whole percentage.
    pub fn coverage_pct(&self) -> i64 {
        (self.coverage * 100.0).round() as i64
    }

    /// Whether the output is verified.
    pub fn meets(&self) -> bool {
        self.verdict == EvidenceVerdict::Verified
    }

    /// The rule the verdict is a decision of: `evidence.required` when nothing
    /// rests on evidence, `evidence.min_coverage` when not enough does or an id
    /// was forged.
    pub fn rule(&self) -> &'static str {
        match self.verdict {
            EvidenceVerdict::Verified | EvidenceVerdict::Unverified => "evidence.required",
            EvidenceVerdict::Partial => "evidence.min_coverage",
        }
    }

    /// The verdict in words, for a span and a node's error.
    pub fn describe(&self) -> String {
        let counted = self.counted_claims();
        let forged = match self.unknown.len() {
            0 => String::new(),
            1 => format!("; 1 id it was never given: {}", self.unknown[0]),
            n => format!(
                "; {n} ids it was never given: {}",
                self.unknown
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        match self.verdict {
            EvidenceVerdict::Verified => format!(
                "{} of {counted} claims cite what the node was given ({}%)",
                self.cited_claims,
                self.coverage_pct()
            ),
            EvidenceVerdict::Partial => format!(
                "{} of {counted} claims cite what the node was given ({}%, {}% required){forged}",
                self.cited_claims,
                self.coverage_pct(),
                (self.min_coverage * 100.0).round() as i64
            ),
            EvidenceVerdict::Unverified => format!(
                "no claim rests on what the node was given ({} given, {} claims, {} declared){forged}",
                self.given, self.claims, self.declared_claims
            ),
        }
    }

    /// The attributes a span carries about the verdict (counts and ids only).
    pub fn span_attributes(&self) -> Vec<(&'static str, serde_json::Value)> {
        vec![
            ("evidence", serde_json::json!(self.verdict)),
            ("evidence_coverage", serde_json::json!(self.coverage_pct())),
            ("evidence_claims", serde_json::json!(self.claims)),
            (
                "evidence_cited_claims",
                serde_json::json!(self.cited_claims),
            ),
            (
                "evidence_declared_claims",
                serde_json::json!(self.declared_claims),
            ),
            ("evidence_given", serde_json::json!(self.given)),
            ("evidence_cited_ids", serde_json::json!(self.cited)),
            ("evidence_unknown_ids", serde_json::json!(self.unknown)),
        ]
    }
}

/// A claim as the verifier sees it: one sentence, bullet or table row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub text: String,
    /// The citations it carries, as written (known or not).
    pub citations: Vec<Citation>,
    /// Whether it ends with a declared marker.
    pub declared: bool,
}

/// The prose of a Markdown text: fenced code blocks and inline code spans
/// are blanked (an id inside code is not a citation), lines are kept.
fn prose(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_fence = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            out.push('\n');
            continue;
        }
        if !in_fence {
            out.push_str(&without_inline_code(line));
        }
        out.push('\n');
    }
    out
}

/// `line` with every `` `code span` `` replaced by a space; an unmatched
/// backtick keeps the rest of the line.
fn without_inline_code(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(start) = rest.find('`') {
        let after = &rest[start + 1..];
        match after.find('`') {
            Some(end) => {
                out.push_str(&rest[..start]);
                out.push(' ');
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

/// The citations of one bracket's content, when it holds nothing else:
/// `doc:a` or `doc:a, mem:b`.
fn bracket_citations(inner: &str) -> Option<Vec<Citation>> {
    let tokens: Vec<&str> = inner
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        return None;
    }
    tokens.iter().map(|t| Citation::parse(t)).collect()
}

/// Every citation in `text` as written, in order, duplicates kept; ids
/// inside code are not citations, nor is a bracket holding anything but ids.
pub fn citations_in(text: &str) -> Vec<Citation> {
    let mut out = Vec::new();
    let prose = prose(text);
    let mut slice = prose.as_str();
    while let Some(open) = slice.find('[') {
        let after = &slice[open + 1..];
        let Some(stop) = after.find(['[', ']', '\n']) else {
            break;
        };
        if after[stop..].starts_with('[') {
            slice = &after[stop..];
            continue;
        }
        if after[stop..].starts_with(']')
            && let Some(found) = bracket_citations(&after[..stop])
        {
            out.extend(found);
        }
        slice = &after[stop + 1..];
    }
    out
}

fn is_declared(text: &str) -> bool {
    let lower = text.to_lowercase();
    DECLARED_MARKERS.iter().any(|m| lower.contains(m))
}

/// Words outside brackets: what makes a fragment a claim.
fn word_count(text: &str) -> usize {
    let mut words = 0;
    let mut depth = 0u32;
    let mut in_word = false;
    for c in text.chars() {
        match c {
            '[' => {
                depth += 1;
                in_word = false;
            }
            ']' => {
                depth = depth.saturating_sub(1);
                in_word = false;
            }
            c if depth == 0 && c.is_alphanumeric() => {
                if !in_word {
                    words += 1;
                    in_word = true;
                }
            }
            c if depth == 0 && c.is_whitespace() => in_word = false,
            _ => {}
        }
    }
    words
}

fn claim_of(text: &str, min_words: usize) -> Option<Claim> {
    let text = text.trim();
    if word_count(text) < min_words {
        return None;
    }
    Some(Claim {
        text: text.to_owned(),
        citations: citations_in(text),
        declared: is_declared(text),
    })
}

/// How far a leading run of citation brackets reaches into `text` (0 when
/// it does not start with one).
fn leading_citations(text: &str) -> usize {
    let mut end = 0;
    loop {
        let rest = &text[end..];
        let trimmed = rest.trim_start();
        let Some(after) = trimmed.strip_prefix('[') else {
            break;
        };
        let Some(close) = after.find(']') else {
            break;
        };
        if bracket_citations(&after[..close]).is_none() {
            break;
        }
        end += rest.len() - trimmed.len() + close + 2;
    }
    end
}

/// A paragraph as its sentences: a terminator (`.`, `!`, `?`, closing quotes
/// and brackets included) followed by a space ends one; a citation that
/// follows the terminator belongs to the sentence before it.
fn sentences(paragraph: &str) -> Vec<Claim> {
    let chars: Vec<char> = paragraph.chars().collect();
    let mut segments: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut i = 0;
    while i < chars.len() {
        current.push(chars[i]);
        if matches!(chars[i], '.' | '!' | '?') {
            let mut j = i + 1;
            while j < chars.len()
                && matches!(chars[j], '"' | '\'' | ')' | ']' | '\u{201d}' | '\u{2019}')
            {
                current.push(chars[j]);
                j += 1;
            }
            if j >= chars.len() || chars[j].is_whitespace() {
                segments.push(std::mem::take(&mut current));
            }
            i = j;
            continue;
        }
        i += 1;
    }
    if !current.trim().is_empty() {
        segments.push(current);
    }
    let mut merged: Vec<String> = Vec::new();
    for segment in segments {
        let segment = segment.trim();
        let lead = leading_citations(segment);
        if lead > 0
            && let Some(previous) = merged.last_mut()
        {
            previous.push(' ');
            previous.push_str(segment[..lead].trim());
            let rest = segment[lead..].trim();
            if !rest.is_empty() {
                merged.push(rest.to_owned());
            }
        } else if !segment.is_empty() {
            merged.push(segment.to_owned());
        }
    }
    merged
        .iter()
        .filter_map(|s| claim_of(s, SENTENCE_WORDS))
        .collect()
}

/// The text of a Markdown heading line, when `line` is one.
fn heading_text(line: &str) -> Option<&str> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &line[hashes..];
    if rest.is_empty() || rest.starts_with(' ') {
        Some(rest.trim())
    } else {
        None
    }
}

/// The text of a bullet or numbered item, when `line` is one.
fn item_text(line: &str) -> Option<&str> {
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return Some(rest.trim_start_matches("[ ] ").trim_start_matches("[x] "));
        }
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        let rest = &line[digits..];
        if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some(rest);
        }
    }
    None
}

fn is_rule(line: &str) -> bool {
    line.len() >= 3 && line.chars().all(|c| matches!(c, '-' | '*' | '_' | ' '))
}

fn is_table_separator(row: &str) -> bool {
    row.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// The cells of a table row as one line of text.
fn row_text(row: &str) -> String {
    row.trim_matches('|')
        .split('|')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The claims of a Markdown text: its sentences, bullets and table rows
/// (a table's header row aside). Headings, block quotes, horizontal rules,
/// code and everything under a `Sources`-like heading are not claims.
pub fn claims_in(text: &str) -> Vec<Claim> {
    let prose = prose(text);
    let mut claims: Vec<Claim> = Vec::new();
    let mut paragraph = String::new();
    let mut pending_row: Option<String> = None;
    let mut in_sources = false;
    fn flush(paragraph: &mut String, pending_row: &mut Option<String>, claims: &mut Vec<Claim>) {
        if !paragraph.trim().is_empty() {
            claims.extend(sentences(paragraph));
        }
        paragraph.clear();
        if let Some(row) = pending_row.take() {
            claims.extend(claim_of(&row, 1));
        }
    }
    for line in prose.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            flush(&mut paragraph, &mut pending_row, &mut claims);
            continue;
        }
        if let Some(heading) = heading_text(trimmed) {
            flush(&mut paragraph, &mut pending_row, &mut claims);
            let first = heading
                .split(|c: char| c.is_whitespace() || c == ':')
                .next()
                .unwrap_or_default()
                .to_lowercase();
            in_sources = SOURCE_HEADINGS.contains(&first.as_str());
            continue;
        }
        if in_sources || trimmed.starts_with('>') || is_rule(trimmed) {
            flush(&mut paragraph, &mut pending_row, &mut claims);
            continue;
        }
        if trimmed.starts_with('|') {
            if !paragraph.trim().is_empty() {
                claims.extend(sentences(&paragraph));
                paragraph.clear();
            }
            if is_table_separator(trimmed) {
                // The row before a separator is the table's header.
                pending_row = None;
            } else {
                if let Some(row) = pending_row.take() {
                    claims.extend(claim_of(&row, 1));
                }
                pending_row = Some(row_text(trimmed));
            }
            continue;
        }
        if let Some(row) = pending_row.take() {
            claims.extend(claim_of(&row, 1));
        }
        if let Some(item) = item_text(trimmed) {
            if !paragraph.trim().is_empty() {
                claims.extend(sentences(&paragraph));
                paragraph.clear();
            }
            claims.extend(claim_of(item, 1));
            continue;
        }
        if !paragraph.is_empty() {
            paragraph.push(' ');
        }
        paragraph.push_str(trimmed);
    }
    flush(&mut paragraph, &mut pending_row, &mut claims);
    claims
}

/// `text` without its citation brackets and declared markers, for what is
/// kept as a fact rather than quoted (a memory): the ids belonged to the
/// output they were written in, not to the fact.
pub fn strip_markers(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(['[', ']', '\n']) else {
            break;
        };
        if !after[close..].starts_with(']') {
            out.push_str(&rest[..open + 1 + close]);
            rest = &after[close..];
            continue;
        }
        let inner = &after[..close];
        let marker = bracket_citations(inner).is_some()
            || DECLARED_MARKERS.contains(&format!("[{}]", inner.trim().to_lowercase()).as_str());
        if marker {
            out.push_str(&rest[..open]);
            let kept = out.trim_end_matches(' ').len();
            out.truncate(kept);
            rest = &after[close + 1..];
            // A marker before a full stop leaves the stop where it was.
            if !rest.starts_with(['.', ',', ';', ':', '!', '?']) && !out.is_empty() {
                out.push(' ');
            }
        } else {
            out.push_str(&rest[..open + 1 + close + 1]);
            rest = &after[close + 1..];
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn push_unique(into: &mut Vec<String>, key: String) {
    if into.len() < IDS_MAX && !into.contains(&key) {
        into.push(key);
    }
}

/// Verifies `texts` (an output and its Markdown artifacts) against what
/// the node was given: the coverage, the ids it cited and the ones it
/// forged, and the verdict under `min_coverage`.
pub fn assess(texts: &[&str], given: &Given, min_coverage: f64) -> Evidence {
    let min_coverage = if min_coverage.is_finite() {
        min_coverage.clamp(0.0, 1.0)
    } else {
        MIN_COVERAGE_DEFAULT
    };
    let (mut cited, mut unknown) = (Vec::new(), Vec::new());
    for text in texts {
        for citation in citations_in(text) {
            if given.contains(&citation) {
                push_unique(&mut cited, citation.key());
            } else {
                push_unique(&mut unknown, citation.key());
            }
        }
    }
    let claims: Vec<Claim> = texts.iter().flat_map(|t| claims_in(t)).collect();
    let rests = |claim: &Claim| claim.citations.iter().any(|c| given.contains(c));
    let cited_claims = claims.iter().filter(|c| rests(c)).count();
    let declared_claims = claims.iter().filter(|c| c.declared && !rests(c)).count();
    let counted = claims.len() - declared_claims;
    let coverage = if counted == 0 {
        0.0
    } else {
        (cited_claims as f64 / counted as f64 * 1000.0).round() / 1000.0
    };
    let verdict = if cited_claims == 0 {
        EvidenceVerdict::Unverified
    } else if !unknown.is_empty() || coverage < min_coverage {
        EvidenceVerdict::Partial
    } else {
        EvidenceVerdict::Verified
    };
    Evidence {
        verdict,
        coverage,
        claims: claims.len(),
        cited_claims,
        declared_claims,
        cited,
        unknown,
        given: given.len(),
        min_coverage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn given() -> (Given, Uuid, Uuid, Uuid) {
        let (chunk, memory, node) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        let given = Given {
            documents: vec![chunk],
            memories: vec![memory],
            nodes: vec![node],
            facts: Vec::new(),
            seen: Vec::new(),
        };
        (given, chunk, memory, node)
    }

    #[test]
    fn claims_are_the_sentences_bullets_and_rows_of_the_prose() {
        let text = "# Report\n\n\
            Revenue grew twelve percent. Costs stayed flat! Was margin up?\n\
            > A quoted passage is not a claim.\n\n\
            - A bullet is one claim.\n\
            1. So is a numbered item.\n\
            * Short\n\n\
            | Metric | Value |\n|---|---|\n| Revenue | 12.4 m |\n| Margin | 41 % |\n\n\
            ```\nnot a claim [doc:1]\n```\n\
            ---\n\
            Done.";
        let claims = claims_in(text);
        let texts: Vec<&str> = claims.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "Revenue grew twelve percent.",
                "Costs stayed flat!",
                "Was margin up?",
                "A bullet is one claim.",
                "So is a numbered item.",
                "Short",
                "Revenue 12.4 m",
                "Margin 41 %",
            ],
            "headings, quotes, the header row, code, rules and one-word sentences are not claims"
        );
    }

    #[test]
    fn citations_are_read_outside_code_and_attached_to_their_sentence() {
        let (given, chunk, memory, node) = given();
        let text = format!(
            "Revenue grew [doc:{chunk}]. Costs fell. [mem:{memory}] Both matter [doc:{chunk}, node:{node}].\n\
             See `[doc:{chunk}]` in code and [the report](http://x) and [1] and [customs.pdf, p. 17].\n\
             ```\n[node:{node}]\n```\n"
        );
        let found = citations_in(&text);
        assert_eq!(found.len(), 4, "{found:?}");
        assert_eq!(found[0], Citation::of(CitationKind::Doc, chunk));
        assert_eq!(found[1], Citation::of(CitationKind::Mem, memory));
        assert_eq!(found[3], Citation::of(CitationKind::Node, node));
        let claims = claims_in(&text);
        assert_eq!(claims.len(), 4, "{claims:?}");
        assert_eq!(claims[0].citations.len(), 1);
        assert_eq!(
            claims[1].text,
            format!("Costs fell. [mem:{memory}]"),
            "a citation after the full stop belongs to the sentence before it"
        );
        assert_eq!(claims[2].citations.len(), 2);
        assert!(claims[3].citations.is_empty(), "{:?}", claims[3]);
        let evidence = assess(&[&text], &given, 0.8);
        assert_eq!(
            evidence.cited.len(),
            3,
            "distinct ids: {:?}",
            evidence.cited
        );
        assert!(evidence.unknown.is_empty());
        assert_eq!((evidence.claims, evidence.cited_claims), (4, 3));
        assert_eq!(evidence.coverage, 0.75);
        assert_eq!(evidence.verdict, EvidenceVerdict::Partial);
        assert!(
            evidence.describe().contains("3 of 4 claims"),
            "{}",
            evidence.describe()
        );
        assert_eq!(
            assess(&[&text], &given, 0.7).verdict,
            EvidenceVerdict::Verified
        );
    }

    #[test]
    fn duplicates_count_once_and_forged_ids_are_violations() {
        let (given, chunk, _, _) = given();
        let other = Uuid::now_v7();
        let text = format!(
            "Revenue grew [doc:{chunk}]. It grew again [doc:{chunk}] [doc:{chunk}].\n\
             Margin was 41 percent [doc:{other}]. Costs were flat [mem:{other}, doc:{other}]."
        );
        let evidence = assess(&[&text], &given, 0.8);
        assert_eq!(evidence.cited, vec![format!("doc:{chunk}")]);
        assert_eq!(
            evidence.unknown,
            vec![format!("doc:{other}"), format!("mem:{other}")]
        );
        assert_eq!((evidence.claims, evidence.cited_claims), (4, 2));
        assert_eq!(evidence.coverage, 0.5);
        assert_eq!(evidence.verdict, EvidenceVerdict::Partial);
        assert!(evidence.describe().contains("2 ids it was never given"));
        // Even full coverage is not verified while a forged id is on the page.
        let forged = format!("Revenue grew [doc:{chunk}]. Costs fell [doc:{other}, doc:{chunk}].");
        let evidence = assess(&[&forged], &given, 0.8);
        assert_eq!(evidence.coverage, 1.0);
        assert_eq!(evidence.verdict, EvidenceVerdict::Partial);
        assert_eq!(evidence.rule(), "evidence.min_coverage");
        // A malformed id of a known kind is unknown, not ignored.
        let bad = format!("Revenue grew [doc:{chunk}]. Costs fell [doc:nope].");
        assert_eq!(
            assess(&[&bad], &given, 0.8).unknown,
            vec!["doc:nope".to_owned()]
        );
    }

    #[test]
    fn declared_claims_are_counted_apart_and_nothing_cited_is_unverified() {
        let (given, chunk, _, _) = given();
        let text = format!(
            "Revenue grew [doc:{chunk}]. Growth will continue next year [assumption].\n\
             - Margins look healthy [own reasoning]\n\
             - They cite nothing"
        );
        let evidence = assess(&[&text], &given, 0.8);
        assert_eq!(
            (
                evidence.claims,
                evidence.cited_claims,
                evidence.declared_claims
            ),
            (4, 1, 2)
        );
        assert_eq!(evidence.counted_claims(), 2);
        assert_eq!(evidence.coverage, 0.5);
        assert_eq!(evidence.verdict, EvidenceVerdict::Partial);
        let only_declared = "Growth will continue [assumption]. Margins look fine [Inference].";
        let evidence = assess(&[only_declared], &given, 0.8);
        assert_eq!(evidence.verdict, EvidenceVerdict::Unverified);
        assert_eq!((evidence.coverage, evidence.declared_claims), (0.0, 2));
        assert_eq!(evidence.rule(), "evidence.required");
        assert!(evidence.describe().starts_with("no claim rests"));
        let nothing = assess(&[""], &Given::default(), 0.8);
        assert_eq!(nothing.verdict, EvidenceVerdict::Unverified);
        assert_eq!((nothing.claims, nothing.given), (0, 0));
        // A declared claim that also cites counts as cited.
        let both = format!("Revenue grew [doc:{chunk}] [assumption].");
        let evidence = assess(&[&both], &given, 0.8);
        assert_eq!((evidence.cited_claims, evidence.declared_claims), (1, 0));
        assert_eq!(evidence.verdict, EvidenceVerdict::Verified);
    }

    #[test]
    fn the_sources_section_and_artifacts_are_read_right() {
        let (given, chunk, memory, _) = given();
        let output = format!(
            "## Findings\n\nRevenue grew [doc:{chunk}].\n\n## Sources\n\n- [doc:{chunk}] report.pdf, p. 3\n- [mem:{memory}] a memory"
        );
        let evidence = assess(&[&output], &given, 1.0);
        assert_eq!(evidence.claims, 1, "the sources list is not a claim");
        let consult =
            format!("{output}\n\n## Sources to consult\n\n- The annual report\n- The filings");
        assert_eq!(
            assess(&[&consult], &given, 1.0).claims,
            1,
            "nor a list of sources to consult"
        );
        assert_eq!(evidence.cited.len(), 2, "but its ids are on record");
        assert_eq!(evidence.verdict, EvidenceVerdict::Verified);
        // A Markdown artifact is verified with the output; an id it forges counts.
        let artifact = format!(
            "# Notes\n\nCosts fell [mem:{memory}].\nMargin rose [doc:{}].",
            Uuid::now_v7()
        );
        let evidence = assess(&[&output, &artifact], &given, 0.5);
        assert_eq!((evidence.claims, evidence.cited_claims), (3, 2));
        assert_eq!(evidence.unknown.len(), 1);
        assert_eq!(evidence.verdict, EvidenceVerdict::Partial);
        let attributes = evidence.span_attributes();
        assert_eq!(attributes[0].0, "evidence");
        assert_eq!(attributes[0].1, serde_json::json!("partial"));
        assert_eq!(attributes[1].1, serde_json::json!(67));
        // The threshold is clamped to a fraction.
        assert_eq!(assess(&[&output], &given, 7.0).min_coverage, 1.0);
        assert_eq!(
            assess(&[&output], &given, f64::NAN).min_coverage,
            MIN_COVERAGE_DEFAULT
        );
        let json = serde_json::to_value(&evidence).unwrap();
        assert_eq!(json["verdict"], "partial");
        let back: Evidence = serde_json::from_value(json).unwrap();
        assert_eq!(back, evidence);
        // A section named Evidence is where the claims are, not a list of sources.
        let findings = format!("## Evidence\n\n- Revenue grew [doc:{chunk}].\n- Costs fell.");
        let evidence = assess(&[&findings], &given, 0.5);
        assert_eq!((evidence.claims, evidence.cited_claims), (2, 1));
        assert_eq!(evidence.verdict, EvidenceVerdict::Verified);
    }

    #[test]
    fn ids_seen_inside_the_context_are_not_forged_and_memories_keep_none() {
        let (mut given, chunk, _, node) = given();
        let other = Uuid::now_v7();
        // An upstream output cites its own sources; the node quotes it: not forged.
        let upstream = format!(
            "Revenue grew [doc:{other}]. See `[doc:{}]`.",
            Uuid::now_v7()
        );
        given.see(&upstream);
        assert_eq!(given.seen, vec![format!("doc:{other}")], "code is not seen");
        assert_eq!(given.len(), 3, "seen ids are not what was given");
        let text = format!("Revenue grew [doc:{other}] [node:{node}]. Costs fell [doc:{chunk}].");
        let evidence = assess(&[&text], &given, 0.8);
        assert!(evidence.unknown.is_empty(), "{evidence:?}");
        assert_eq!(evidence.verdict, EvidenceVerdict::Verified);
        assert!(evidence.cited.contains(&format!("doc:{other}")));
        // Markers are stripped from what is kept as a fact.
        assert_eq!(
            strip_markers(&format!(
                "Revenue grew [doc:{chunk}] [node:{node}]. Growth continues [assumption]. See [1] and [a, b]."
            )),
            "Revenue grew. Growth continues. See [1] and [a, b]."
        );
        assert_eq!(
            strip_markers("[doc:x] Goods are exempt"),
            "Goods are exempt"
        );
        assert_eq!(strip_markers("plain [unclosed"), "plain [unclosed");
        assert_eq!(strip_markers("  spaced   out  "), "spaced out");
    }

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
            seen: Vec::new(),
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
