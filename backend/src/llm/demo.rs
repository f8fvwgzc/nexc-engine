//! Deterministic, offline "demo" provider. It lets anyone try planning and
//! execution without an API key: plans are a rule-based refinement of the
//! user's real nodes and node outputs are clearly labelled templates,
//! streamed with realistic pacing and token counts.

use std::time::Duration;

use async_stream::stream;
use serde_json::json;

use super::{LlmEvent, LlmProvider, LlmRequest, LlmStream, StopReason, Usage, estimate_tokens};
use crate::domain::graph::{EdgeKind, Executor, NodeKind, default_role_for_kind};
use crate::domain::memory::MEMORY_SCHEMA_NAME;
use crate::domain::plan::{
    PLAN_SCHEMA_NAME, PlanContext, PlanProposal, ProposedEdge, ProposedNode,
};
use crate::domain::prompt::{OUTPUT_MARKER, parse_node_prompt};
use crate::kernel;

/// Label that starts every demo text output.
pub const DEMO_LABEL: &str = "[demo output]";

/// The offline provider. `pace` scales the artificial streaming delay
/// (zero in tests).
#[derive(Debug, Clone)]
pub struct DemoProvider {
    pace: Duration,
}

impl DemoProvider {
    /// Provider with the given base delay between streamed chunks.
    pub fn new(pace: Duration) -> Self {
        DemoProvider { pace }
    }
}

impl LlmProvider for DemoProvider {
    fn stream(&self, req: LlmRequest) -> LlmStream {
        let prompt = req
            .messages
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_default();
        let (text, chunk_chars) = match req.json_schema.as_ref().map(|s| s.name) {
            Some(PLAN_SCHEMA_NAME) => (plan_json(&prompt), 48),
            Some(MEMORY_SCHEMA_NAME) => (memories_json(&prompt), 64),
            _ => (text_output(&prompt), 18),
        };
        let input_tokens = estimate_tokens(&req.system) + estimate_tokens(&prompt);
        let pace = self.pace;
        Box::pin(stream! {
            let mut usage = Usage { input_tokens, output_tokens: 0 };
            yield Ok(LlmEvent::Usage(usage));
            for (i, chunk) in chunks(&text, chunk_chars).into_iter().enumerate() {
                if !pace.is_zero() {
                    // Deterministic jitter: 50–150 % of the base pace.
                    let jitter = 50 + kernel::hash64(chunk.as_bytes(), i as u64) % 100;
                    tokio::time::sleep(pace * jitter as u32 / 100).await;
                }
                usage.output_tokens += estimate_tokens(&chunk);
                yield Ok(LlmEvent::Text(chunk));
            }
            yield Ok(LlmEvent::Usage(usage));
            yield Ok(LlmEvent::Done(StopReason::EndTurn));
        })
    }
}

/// Splits `text` into chunks of about `size` characters on char boundaries.
fn chunks(text: &str, size: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    chars
        .chunks(size.max(1))
        .map(|c| c.iter().collect())
        .collect()
}

fn text_output(prompt: &str) -> String {
    let (title, upstream) =
        parse_node_prompt(prompt).unwrap_or_else(|| ("Untitled task".into(), Vec::new()));
    let mut out = format!(
        "{DEMO_LABEL} This text was generated offline by the demo provider; configure an API key \
         in Settings for real results.\n\n# {title}\n\n## Summary\n\nA concise, structured result for \
         **{title}**. In a real run the model would complete this step using the graph goal, the \
         node instructions and the results of its dependencies.\n"
    );
    if upstream.is_empty() {
        out.push_str("\n## Approach\n\n1. Clarify the scope and success criteria.\n2. Gather the key facts.\n3. Produce the deliverable and check it against the goal.\n");
    } else {
        out.push_str("\n## Built on\n\n");
        for u in &upstream {
            out.push_str(&format!(
                "- **{u}** — its findings feed directly into this step.\n"
            ));
        }
    }
    out.push_str("\n## Key points\n\n- Every claim is tied to an upstream result or marked as an assumption.\n- Open questions are listed explicitly so a reviewer can resolve them.\n- The output is ready to be consumed by downstream nodes.\n");
    out
}

/// Extracts up to two bullet-like facts. Demo boilerplate is never remembered.
fn memories_json(prompt: &str) -> String {
    let output = prompt
        .split_once(OUTPUT_MARKER)
        .map(|(_, o)| o)
        .unwrap_or_default();
    let source = if output.contains(DEMO_LABEL) {
        ""
    } else {
        output
    };
    // Only clean prose lines: no banners, quotes, tables or bold label fragments.
    let facts: Vec<_> = source
        .lines()
        .map(|l| l.trim().trim_start_matches(['-', '*', '#', ' ']).trim())
        .filter(|l| !l.starts_with(['>', '|']) && !l.contains("**"))
        .filter(|l| !l.to_lowercase().contains("demo"))
        .filter(|l| (30..=200).contains(&l.chars().count()))
        .take(2)
        .map(|l| json!({ "kind": "observation", "content": format!("[demo] {l}"), "importance": 0.3 }))
        .collect();
    json!({ "memories": facts }).to_string()
}

fn plan_json(prompt: &str) -> String {
    let proposal = PlanContext::extract(prompt)
        .map(|ctx| refine(&ctx))
        .unwrap_or_default();
    serde_json::to_string(&proposal).expect("proposal serialises")
}

/// Bullet items of a node's content (`- `, `* ` or `1. ` lines).
fn bullets(content: &str) -> Vec<String> {
    content
        .lines()
        .map(str::trim)
        .filter_map(|l| {
            l.strip_prefix("- ")
                .or_else(|| l.strip_prefix("* "))
                .or_else(|| {
                    l.split_once(". ")
                        .filter(|(n, _)| n.parse::<u32>().is_ok())
                        .map(|(_, rest)| rest)
                })
        })
        .map(|b| b.trim().to_owned())
        .filter(|b| b.len() >= 3)
        .collect()
}

fn proposed(reference: String, title: String, content: String, kind: NodeKind) -> ProposedNode {
    ProposedNode {
        reference,
        existing_id: None,
        title,
        content,
        kind,
        agent_role: default_role_for_kind(kind).into(),
        executor: executor_for_kind(kind, Executor::Llm),
        tags: vec!["demo".into()],
    }
}

/// Steps that produce files (documents, final deliverables) go to the agent
/// runtime, which has file tools; everything else keeps its executor.
fn executor_for_kind(kind: NodeKind, current: Executor) -> Executor {
    match kind {
        NodeKind::Document | NodeKind::Output => Executor::Agent,
        _ => current,
    }
}

/// Short title for a sub-task: the first few words of its bullet.
fn short_title(item: &str) -> String {
    let words: Vec<&str> = item.split_whitespace().take(6).collect();
    let mut title = words.join(" ");
    title = title.trim_end_matches([',', ';', ':', '.']).to_owned();
    if item.split_whitespace().count() > words.len() {
        title.push('…');
    }
    title
}

/// Most sub-tasks a demo plan adds, so refined graphs stay readable.
const MAX_SPLIT_ITEMS: usize = 3;

/// Rule-based plan refinement, standing in for the model: keeps every node,
/// routes file-producing steps to the agent runtime, splits the single
/// broadest research/topic node into a few parallel sub-tasks that feed it,
/// preserves dependencies, links nodes whose content mentions another node's
/// title, and adds a final output node when there is none.
pub fn refine(ctx: &PlanContext) -> PlanProposal {
    if ctx.nodes.is_empty() {
        return starter_plan(ctx);
    }
    let refs: Vec<String> = (0..ctx.nodes.len())
        .map(|i| format!("n{}", i + 1))
        .collect();
    let mut nodes: Vec<ProposedNode> = ctx
        .nodes
        .iter()
        .zip(&refs)
        .map(|(n, reference)| ProposedNode {
            reference: reference.clone(),
            existing_id: Some(n.id),
            title: n.title.clone(),
            content: if n.content.trim().is_empty() {
                format!(
                    "Complete \"{}\" and summarise the result for the next steps.",
                    n.title
                )
            } else {
                n.content.clone()
            },
            kind: n.kind,
            agent_role: n
                .agent_role
                .clone()
                .unwrap_or_else(|| default_role_for_kind(n.kind).into()),
            executor: executor_for_kind(n.kind, n.executor),
            tags: n.tags.clone(),
        })
        .collect();

    let index = |id| ctx.nodes.iter().position(|n| n.id == id);
    let mut edges: Vec<ProposedEdge> = Vec::new();
    let link = |s: usize, t: usize, edges: &mut Vec<ProposedEdge>| {
        let edge = ProposedEdge {
            source_ref: refs[s].clone(),
            target_ref: refs[t].clone(),
        };
        if s != t && !edges.contains(&edge) {
            edges.push(edge);
        }
    };
    for e in ctx.edges.iter().filter(|e| e.kind == EdgeKind::DependsOn) {
        if let (Some(s), Some(t)) = (index(e.source), index(e.target)) {
            link(s, t, &mut edges);
        }
    }
    for (t, target) in ctx.nodes.iter().enumerate() {
        let haystack = target.content.to_lowercase();
        for (s, source) in ctx.nodes.iter().enumerate() {
            if source.title.len() >= 4 && haystack.contains(&source.title.to_lowercase()) {
                link(s, t, &mut edges);
            }
        }
    }

    // Split the broadest research/topic node: its sub-tasks run in parallel,
    // inherit its upstream dependencies, and all feed back into it.
    let broadest = ctx
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| matches!(n.kind, NodeKind::Research | NodeKind::Topic))
        .map(|(i, n)| (i, bullets(&n.content)))
        .filter(|(_, items)| items.len() > MAX_SPLIT_ITEMS)
        .max_by_key(|(_, items)| items.len());
    let split = broadest.is_some();
    if let Some((i, items)) = broadest {
        let parent = &ctx.nodes[i];
        let upstream: Vec<String> = edges
            .iter()
            .filter(|e| e.target_ref == refs[i])
            .map(|e| e.source_ref.clone())
            .collect();
        for (j, item) in items.iter().take(MAX_SPLIT_ITEMS).enumerate() {
            let sub = format!("{}-{}", refs[i], j + 1);
            nodes.push(proposed(
                sub.clone(),
                short_title(item),
                format!("Part of \"{}\": {item}", parent.title),
                NodeKind::Task,
            ));
            edges.extend(upstream.iter().map(|u| ProposedEdge {
                source_ref: u.clone(),
                target_ref: sub.clone(),
            }));
            edges.push(ProposedEdge {
                source_ref: sub,
                target_ref: refs[i].clone(),
            });
        }
    }

    let has_output = ctx.nodes.iter().any(|n| n.kind == NodeKind::Output);
    if !has_output {
        let sinks: Vec<String> = nodes
            .iter()
            .map(|n| n.reference.clone())
            .filter(|r| !edges.iter().any(|e| &e.source_ref == r))
            .collect();
        nodes.push(proposed(
            "final".into(),
            "Final deliverable".into(),
            "Combine all upstream results into one polished, well-structured deliverable that fulfils the goal."
                .into(),
            NodeKind::Output,
        ));
        edges.extend(sinks.into_iter().map(|s| ProposedEdge {
            source_ref: s,
            target_ref: "final".into(),
        }));
    }
    let summary = format!(
        "[demo] Refined {} node(s){}{}; file-producing steps run on the agent runtime. Configure an API key for model-generated plans.",
        ctx.nodes.len(),
        if split {
            ", split the broadest research step into parallel sub-tasks"
        } else {
            ""
        },
        if has_output {
            ""
        } else {
            " and added a final output node"
        },
    );
    PlanProposal {
        summary,
        nodes,
        edges,
    }
}

fn starter_plan(ctx: &PlanContext) -> PlanProposal {
    let goal = if ctx.goal.trim().is_empty() {
        "the goal"
    } else {
        ctx.goal.trim()
    };
    let steps = [
        (
            "scope",
            "Clarify scope",
            NodeKind::Topic,
            format!("Define the questions, audience and success criteria for: {goal}"),
        ),
        (
            "research",
            "Research",
            NodeKind::Research,
            "Collect the facts, sources and constraints needed.".to_owned(),
        ),
        (
            "draft",
            "Draft",
            NodeKind::Document,
            "Write a complete first draft from the research.".to_owned(),
        ),
        (
            "final",
            "Final deliverable",
            NodeKind::Output,
            "Review the draft and produce the final deliverable.".to_owned(),
        ),
    ];
    let nodes = steps
        .iter()
        .map(|(r, t, k, c)| proposed((*r).into(), (*t).into(), c.clone(), *k))
        .collect();
    let edges = steps
        .windows(2)
        .map(|w| ProposedEdge {
            source_ref: w[0].0.into(),
            target_ref: w[1].0.into(),
        })
        .collect();
    PlanProposal {
        summary: "[demo] Created a starter plan for an empty graph.".into(),
        nodes,
        edges,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use uuid::Uuid;

    use super::*;
    use crate::domain::plan::{ContextEdge, ContextNode, sanitize_proposal};

    fn node(title: &str, content: &str, kind: NodeKind) -> ContextNode {
        ContextNode {
            id: Uuid::now_v7(),
            title: title.into(),
            kind,
            content: content.into(),
            tags: vec![],
            agent_role: None,
            executor: Executor::Llm,
        }
    }

    fn ctx(nodes: Vec<ContextNode>, edges: Vec<ContextEdge>) -> PlanContext {
        PlanContext {
            goal: "Write a report".into(),
            instructions: String::new(),
            nodes,
            edges,
            suggestions: vec![],
            memories: vec![],
            agent_roles: vec![],
        }
    }

    #[test]
    fn refines_real_nodes() {
        let a = node(
            "Literature review",
            "- history\n- current tools\n- open problems\n- adoption",
            NodeKind::Research,
        );
        let b = node(
            "Write draft",
            "Use the Literature review findings.",
            NodeKind::Document,
        );
        let plan = refine(&ctx(vec![a.clone(), b.clone()], vec![]));
        let existing: HashSet<_> = [a.id, b.id].into();
        let (clean, notes) = sanitize_proposal(plan.clone(), &existing, 500);
        assert_eq!(clean, plan, "demo plans need no correction: {notes:?}");
        assert_eq!(
            plan.nodes.len(),
            2 + 3 + 1,
            "kept 2, split 3 bullets, added final"
        );
        assert!(plan.edges.contains(&ProposedEdge {
            source_ref: "n1".into(),
            target_ref: "n2".into()
        }));
        assert!(
            plan.edges.contains(&ProposedEdge {
                source_ref: "n1-1".into(),
                target_ref: "n1".into()
            }),
            "sub-tasks feed the node they were split from"
        );
        let draft = plan.nodes.iter().find(|n| n.reference == "n2").unwrap();
        assert_eq!(
            draft.executor,
            Executor::Agent,
            "documents run on the agent runtime"
        );
        assert!(plan.nodes.iter().any(|n| n.kind == NodeKind::Output));
        assert!(plan.summary.starts_with("[demo]"));
    }

    #[test]
    fn empty_graph_gets_a_starter_plan() {
        let plan = refine(&ctx(vec![], vec![]));
        assert_eq!(plan.nodes.len(), 4);
        assert_eq!(plan.edges.len(), 3);
    }

    #[tokio::test]
    async fn streams_labelled_text_with_usage() {
        use crate::domain::settings::LlmProviderKind;
        use crate::llm::{LlmTarget, Message, collect};
        let req = LlmRequest {
            target: LlmTarget {
                provider: LlmProviderKind::Demo,
                model: "demo".into(),
                base_url: None,
                api_key: None,
            },
            system: String::new(),
            messages: vec![Message::user("# Task: Draft\nKind: document\n")],
            max_tokens: 100,
            json_schema: None,
            effort: None,
            cacheable: false,
        };
        let c = collect(DemoProvider::new(Duration::ZERO).stream(req), |_| {})
            .await
            .unwrap();
        assert!(c.text.starts_with(DEMO_LABEL));
        assert!(c.text.contains("# Draft"));
        assert!(c.usage.input_tokens > 0 && c.usage.output_tokens > 0);
    }

    #[test]
    fn extracts_demo_memories() {
        let json = memories_json(&format!(
            "{OUTPUT_MARKER}\n- The report targets engineering managers in 2026.\n- ok"
        ));
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["memories"].as_array().unwrap().len(), 1);
        let boilerplate = memories_json(&format!(
            "{OUTPUT_MARKER}\n{DEMO_LABEL} text\n- a long enough line to count as a fact"
        ));
        assert_eq!(boilerplate, r#"{"memories":[]}"#);
        let runtime_markup = memories_json(&format!(
            "{OUTPUT_MARKER}\n> **Demo mode** - generated offline by the runtime\n\
             **Role:** writer · **Kind:** output, padded to be long enough\n\
             | a | table row that is long enough to be picked up |"
        ));
        assert_eq!(
            runtime_markup, r#"{"memories":[]}"#,
            "markup is never remembered"
        );
    }
}
