//! Prompt layouts shared by the executors and the offline demo provider
//! (which reads the same structure back to produce plausible output).
//!
//! Every piece of context carries the citation id the model is told to cite
//! it by (`domain::evidence`): an upstream result's heading ends with
//! `[node:<id>]`, a memory line starts with `[mem:<id>]`, a passage's first
//! line with `[doc:<chunk id>]`.

use super::evidence::{CITATION_INSTRUCTION, Citation, CitationKind};
use super::ontology::NodeType;

/// First line of every node prompt, followed by the node title.
pub const TASK_HEADING: &str = "# Task: ";
/// Heading of the upstream results section; each result is a `### title [node:<id>]` block.
pub const UPSTREAM_HEADING: &str = "## Upstream results";
/// Heading of the memories section; each memory is a `- [mem:<id>] text` line.
pub const MEMORIES_HEADING: &str = "## Relevant memories";
/// Heading of the passages section; each passage starts with `[doc:<chunk id>] <source>`.
pub const DOCUMENTS_HEADING: &str = "## Relevant passages from the workspace's documents";
/// Precedes the node output in memory-extraction prompts.
pub const OUTPUT_MARKER: &str = "NODE_OUTPUT:";
/// Characters of each upstream output included in the prompt of a single LLM call.
pub const UPSTREAM_CHARS: usize = 12_000;
/// Characters of each upstream output handed to an agent, which works over
/// several turns and has room for more (the runtime's own limit).
pub const AGENT_UPSTREAM_CHARS: usize = 24_000;

/// One upstream result passed to a node.
#[derive(Debug, Clone)]
pub struct UpstreamOutput {
    pub node_id: uuid::Uuid,
    pub title: String,
    pub output: String,
    /// Label of the relation that makes this node an upstream (may be empty).
    pub relation: String,
    /// Why the edge exists (may be empty).
    pub reason: String,
}

/// Inputs of [`node_prompt`].
#[derive(Debug, Clone, Copy)]
pub struct NodePrompt<'a> {
    pub goal: &'a str,
    pub title: &'a str,
    pub kind: &'a str,
    /// The node's type in the graph's ontology, when it still exists.
    pub node_type: Option<&'a NodeType>,
    pub content: &'a str,
    pub upstream: &'a [UpstreamOutput],
    /// Memories, each headed by its citation id (`[mem:<id>] …`).
    pub memories: &'a [String],
    /// Passages of the workspace's documents, each headed by its citation id
    /// and its source (`[doc:<chunk id>] <source>` on the first line).
    pub documents: &'a [String],
}

/// Heading of the citation rules in a node prompt.
pub const CITING_HEADING: &str = "## Citing what you were given";

/// The heading of one upstream result: its title and its citation id.
pub fn upstream_heading(upstream: &UpstreamOutput) -> String {
    format!(
        "### {} {}",
        upstream.title,
        Citation::of(CitationKind::Node, upstream.node_id).render()
    )
}

/// The title of an upstream result without its citation id, as a heading
/// reads back (`parse_node_prompt`).
fn strip_node_id(heading: &str) -> &str {
    let trimmed = heading.trim();
    match trimmed.rsplit_once(" [node:") {
        Some((title, rest)) if rest.ends_with(']') => title.trim_end(),
        _ => trimmed,
    }
}

/// Renders the user message for executing one node.
pub fn node_prompt(p: NodePrompt<'_>) -> String {
    let mut out = format!("{TASK_HEADING}{}\nKind: {}\n", p.title, p.kind);
    if let Some(t) = p.node_type.filter(|t| !t.description.trim().is_empty()) {
        out.push_str(&format!("Meaning of this kind: {}\n", t.description.trim()));
    }
    if !p.goal.trim().is_empty() {
        out.push_str(&format!("\n## Overall goal\n{}\n", p.goal.trim()));
    }
    if !p.content.trim().is_empty() {
        out.push_str(&format!("\n## Instructions\n{}\n", p.content.trim()));
    }
    if !p.upstream.is_empty() {
        out.push_str(&format!("\n{UPSTREAM_HEADING}\n"));
        for u in p.upstream {
            let text: String = u.output.chars().take(UPSTREAM_CHARS).collect();
            out.push_str(&format!("\n{}\n", upstream_heading(u)));
            if !u.reason.trim().is_empty() {
                let relation = Some(u.relation.trim()).filter(|r| !r.is_empty());
                out.push_str(&format!(
                    "Why it matters here ({}): {}\n",
                    relation.unwrap_or("upstream"),
                    u.reason.trim()
                ));
            }
            out.push_str(&format!("{}\n", text.trim()));
        }
    }
    if !p.memories.is_empty() {
        out.push_str(&format!("\n{MEMORIES_HEADING}\n"));
        for m in p.memories {
            out.push_str(&format!("- {m}\n"));
        }
    }
    if !p.documents.is_empty() {
        out.push_str(&format!(
            "\n{DOCUMENTS_HEADING}\n\
             Each passage starts with its id and its source; use them where they bear on the \
             task and cite the id when you do.\n"
        ));
        for d in p.documents {
            out.push_str(&format!("\n{d}\n"));
        }
    }
    if !p.upstream.is_empty() || !p.memories.is_empty() || !p.documents.is_empty() {
        out.push_str(&format!("\n{CITING_HEADING}\n{CITATION_INSTRUCTION}\n"));
    }
    out.push_str("\nComplete the task. Answer in Markdown and build on the upstream results.");
    out
}

/// Parses the task title and upstream titles back out of a node prompt.
pub fn parse_node_prompt(prompt: &str) -> Option<(String, Vec<String>)> {
    let title = prompt
        .lines()
        .find_map(|l| l.strip_prefix(TASK_HEADING))?
        .trim()
        .to_owned();
    let upstream = prompt
        .split_once(UPSTREAM_HEADING)
        .map(|(_, rest)| {
            rest.lines()
                .take_while(|l| !l.starts_with("## "))
                .filter_map(|l| l.strip_prefix("### "))
                .map(|t| strip_node_id(t).to_owned())
                .collect()
        })
        .unwrap_or_default();
    Some((title, upstream))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_round_trips() {
        let upstream = vec![UpstreamOutput {
            node_id: uuid::Uuid::nil(),
            title: "Sources".into(),
            output: "a\nb".into(),
            relation: "Depends on".into(),
            reason: "the report cites them".into(),
        }];
        let prompt = node_prompt(NodePrompt {
            goal: "Write a report",
            title: "Draft",
            kind: "document",
            node_type: None,
            content: "Write it.",
            upstream: &upstream,
            memories: &["User prefers APA".into()],
            documents: &["[style.pdf, p. 2]\nCite sources in APA.".into()],
        });
        assert!(prompt.starts_with("# Task: Draft\nKind: document"));
        assert!(prompt.contains("- User prefers APA"));
        assert!(prompt.contains("Why it matters here (Depends on): the report cites them"));
        assert_eq!(
            parse_node_prompt(&prompt),
            Some(("Draft".into(), vec!["Sources".into()]))
        );
        assert_eq!(parse_node_prompt("nothing"), None);
    }

    #[test]
    fn every_piece_of_context_carries_its_citation_id_and_the_model_is_told_to_cite() {
        let node_id = uuid::Uuid::now_v7();
        let upstream = vec![UpstreamOutput {
            node_id,
            title: "Sources".into(),
            output: "a".into(),
            relation: String::new(),
            reason: String::new(),
        }];
        let memory = format!("[mem:{}] User prefers APA", uuid::Uuid::now_v7());
        let passage = format!(
            "[doc:{}] style.pdf, p. 2\nCite sources in APA.",
            uuid::Uuid::now_v7()
        );
        let prompt = node_prompt(NodePrompt {
            goal: "",
            title: "Draft",
            kind: "document",
            node_type: None,
            content: "",
            upstream: &upstream,
            memories: std::slice::from_ref(&memory),
            documents: std::slice::from_ref(&passage),
        });
        assert!(prompt.contains(&format!("### Sources [node:{node_id}]")));
        assert!(prompt.contains(&format!("- {memory}")));
        assert!(prompt.contains(&passage));
        assert!(prompt.contains(CITING_HEADING) && prompt.contains("[assumption]"));
        // The ids never leak into the titles the demo provider reads back.
        assert_eq!(
            parse_node_prompt(&prompt).unwrap().1,
            vec!["Sources".to_owned()]
        );
        let bare = node_prompt(NodePrompt {
            goal: "",
            title: "Draft",
            kind: "task",
            node_type: None,
            content: "",
            upstream: &[],
            memories: &[],
            documents: &[],
        });
        assert!(!bare.contains(CITING_HEADING), "nothing to cite, no rule");
    }
}
