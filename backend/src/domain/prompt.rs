//! Prompt layouts shared by the executors and the offline demo provider
//! (which reads the same structure back to produce plausible output).

use super::ontology::NodeType;

/// First line of every node prompt, followed by the node title.
pub const TASK_HEADING: &str = "# Task: ";
/// Heading of the upstream results section; each result is a `### title` block.
pub const UPSTREAM_HEADING: &str = "## Upstream results";
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
    pub memories: &'a [String],
    /// Passages of the workspace's documents, each headed by its citation.
    pub documents: &'a [String],
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
            out.push_str(&format!("\n### {}\n", u.title));
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
        out.push_str("\n## Relevant memories\n");
        for m in p.memories {
            out.push_str(&format!("- {m}\n"));
        }
    }
    if !p.documents.is_empty() {
        out.push_str(
            "\n## Relevant passages from the workspace's documents\n\
             Use them where they bear on the task and name the source in brackets when you do.\n",
        );
        for d in p.documents {
            out.push_str(&format!("\n{d}\n"));
        }
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
                .map(|t| t.trim().to_owned())
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
}
