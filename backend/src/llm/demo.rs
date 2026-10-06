//! Deterministic, offline "demo" provider. It lets anyone try planning and
//! execution without an API key: plans are a rule-based refinement of the
//! user's real nodes and node outputs are clearly labelled templates,
//! streamed with realistic pacing and token counts.

use std::time::Duration;

use async_stream::stream;
use serde_json::json;

use super::{LlmEvent, LlmProvider, LlmRequest, LlmStream, StopReason, Usage, estimate_tokens};
use crate::domain::assistant::{ASSISTANT_SCHEMA_NAME, TEAMS_MARKER, USER_MESSAGE_MARKER};
use crate::domain::graph::Executor;
use crate::domain::insight::DAY_SUMMARY_SCHEMA_NAME;
use crate::domain::memory::MEMORY_SCHEMA_NAME;
use crate::domain::ontology::Ontology;
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
            Some(ASSISTANT_SCHEMA_NAME) => (assistant_json(&prompt), 48),
            Some(DAY_SUMMARY_SCHEMA_NAME) => (day_summary_json(&prompt), 48),
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
/// The offline assistant: it cannot reason, so it says what it was given and
/// files an issue only for a message of the form `create issue: <title>`.
fn assistant_json(prompt: &str) -> String {
    let message = prompt
        .split_once(USER_MESSAGE_MARKER)
        .map_or(prompt, |(_, m)| m)
        .trim();
    let team = prompt
        .lines()
        .find_map(|l| l.strip_prefix(TEAMS_MARKER))
        .and_then(|teams| teams.split_whitespace().next())
        .unwrap_or_default();
    let title = message
        .to_lowercase()
        .strip_prefix("create issue:")
        .map(|_| message["create issue:".len()..].trim().to_owned())
        .filter(|t| !t.is_empty() && !team.is_empty());
    let memories = prompt.matches("\n- ").count();
    let reply = match &title {
        Some(title) => format!("{DEMO_LABEL} Filed \"{title}\" with {team}."),
        None => format!(
            "{DEMO_LABEL} The demo provider cannot answer questions. It was given {memories} \
             line(s) of workspace context. Write `create issue: <title>` to file an issue, or \
             configure a real model in Settings."
        ),
    };
    let issues: Vec<serde_json::Value> = title
        .iter()
        .map(|title| {
            serde_json::json!({
                "team_key": team, "title": title, "description": "", "priority": 0
            })
        })
        .collect();
    serde_json::json!({ "reply": reply, "issues": issues }).to_string()
}

/// A day summary without a model: the counts of the digest it was given,
/// read back as sentences, and the entries that name a failure.
fn day_summary_json(digest: &str) -> String {
    let field = |name: &str| {
        digest
            .lines()
            .find_map(|l| l.strip_prefix(name))
            .unwrap_or_default()
            .trim()
    };
    let attention: Vec<String> = digest
        .lines()
        .filter(|l| l.contains("· run_failed ·") || l.contains("· failed:"))
        .take(5)
        .map(|l| l.split_once(" · ").map_or(l, |(_, rest)| rest).to_owned())
        .collect();
    serde_json::json!({
        "headline": format!(
            "{DEMO_LABEL} {} entries were recorded on {}.", field("Entries:"), field("Day:")
        ),
        "highlights": [
            format!("By kind of event: {}.", field("By kind:")),
            format!("By person: {}.", field("By person:")),
            "The demo provider only counts; configure a real model in Settings for a summary \
             in words.",
        ],
        "attention": attention,
    })
    .to_string()
}

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

/// The parts of a graph's ontology the rule-based planner works with.
struct Vocabulary {
    ontology: Ontology,
    /// Type of an ordinary step.
    work: String,
    /// Type of the node that delivers the result: produces a file, latest stage.
    deliverable: String,
    /// Relation that orders execution.
    dependency: String,
}

impl Vocabulary {
    fn of(ctx: &PlanContext) -> Vocabulary {
        // Prompts written before graphs had ontologies carry none.
        let ontology = if ctx.ontology.node_types.is_empty() {
            Ontology::starter()
        } else {
            ctx.ontology.clone()
        };
        let work = ontology.default_node_kind().unwrap_or_default().to_owned();
        let deliverable = ontology
            .node_types
            .iter()
            .filter(|t| t.produces_artifact)
            .max_by_key(|t| t.stage)
            .or(ontology.node_types.iter().max_by_key(|t| t.stage))
            .map(|t| t.key.clone())
            .unwrap_or_default();
        let dependency = ontology
            .default_relation()
            .map(|r| r.key.clone())
            .unwrap_or_default();
        Vocabulary {
            ontology,
            work,
            deliverable,
            dependency,
        }
    }

    /// Whether `kind` frames or explores the work, i.e. comes before ordinary steps.
    fn is_exploratory(&self, kind: &str) -> bool {
        self.ontology.stage_of(kind) < self.ontology.stage_of(&self.work)
    }

    /// Steps that produce files (documents, final deliverables) go to the agent
    /// runtime, which has file tools; everything else keeps its executor.
    fn executor(&self, kind: &str, current: Executor) -> Executor {
        if self.ontology.produces_artifact(kind) {
            Executor::Agent
        } else {
            current
        }
    }

    fn node(&self, reference: String, title: String, content: String, kind: &str) -> ProposedNode {
        ProposedNode {
            reference,
            existing_id: None,
            title,
            content,
            kind: kind.to_owned(),
            agent_role: self.ontology.role_for(kind).into(),
            executor: self.executor(kind, Executor::Llm),
            tags: vec!["demo".into()],
        }
    }

    fn edge(&self, source_ref: &str, target_ref: &str, reason: String) -> ProposedEdge {
        ProposedEdge {
            source_ref: source_ref.to_owned(),
            target_ref: target_ref.to_owned(),
            kind: self.dependency.clone(),
            reason,
        }
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
/// broadest exploratory node into a few parallel sub-tasks that feed it,
/// preserves edges with their reasons, links nodes whose content mentions
/// another node's title, and adds a final deliverable node when there is
/// none. Node and relation types come from the graph's ontology.
pub fn refine(ctx: &PlanContext) -> PlanProposal {
    let vocab = Vocabulary::of(ctx);
    if ctx.nodes.is_empty() {
        return starter_plan(ctx, &vocab);
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
            kind: n.kind.clone(),
            agent_role: n
                .agent_role
                .clone()
                .unwrap_or_else(|| vocab.ontology.role_for(&n.kind).into()),
            executor: vocab.executor(&n.kind, n.executor),
            tags: n.tags.clone(),
        })
        .collect();

    let index = |id| ctx.nodes.iter().position(|n| n.id == id);
    let mut edges: Vec<ProposedEdge> = Vec::new();
    let link = |edge: ProposedEdge, edges: &mut Vec<ProposedEdge>| {
        let same = |e: &ProposedEdge| {
            e.source_ref == edge.source_ref
                && e.target_ref == edge.target_ref
                && e.kind == edge.kind
        };
        if edge.source_ref != edge.target_ref && !edges.iter().any(same) {
            edges.push(edge);
        }
    };
    for e in &ctx.edges {
        if let (Some(s), Some(t)) = (index(e.source), index(e.target)) {
            let edge = ProposedEdge {
                source_ref: refs[s].clone(),
                target_ref: refs[t].clone(),
                kind: e.kind.clone(),
                reason: e.reason.clone(),
            };
            link(edge, &mut edges);
        }
    }
    for (t, target) in ctx.nodes.iter().enumerate() {
        let haystack = target.content.to_lowercase();
        for (s, source) in ctx.nodes.iter().enumerate() {
            if source.title.len() >= 4 && haystack.contains(&source.title.to_lowercase()) {
                let reason = format!(
                    "\"{}\" mentions \"{}\" in its instructions",
                    target.title, source.title
                );
                link(vocab.edge(&refs[s], &refs[t], reason), &mut edges);
            }
        }
    }

    // Split the broadest exploratory node: its sub-tasks run in parallel,
    // inherit its upstream dependencies, and all feed back into it.
    let broadest = ctx
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| vocab.is_exploratory(&n.kind))
        .map(|(i, n)| (i, bullets(&n.content)))
        .filter(|(_, items)| items.len() > MAX_SPLIT_ITEMS)
        .max_by_key(|(_, items)| items.len());
    let split = broadest.is_some();
    if let Some((i, items)) = broadest {
        let parent = &ctx.nodes[i];
        let upstream: Vec<ProposedEdge> = edges
            .iter()
            .filter(|e| e.target_ref == refs[i] && e.kind == vocab.dependency)
            .cloned()
            .collect();
        for (j, item) in items.iter().take(MAX_SPLIT_ITEMS).enumerate() {
            let sub = format!("{}-{}", refs[i], j + 1);
            nodes.push(vocab.node(
                sub.clone(),
                short_title(item),
                format!("Part of \"{}\": {item}", parent.title),
                &vocab.work,
            ));
            edges.extend(upstream.iter().map(|u| ProposedEdge {
                target_ref: sub.clone(),
                ..u.clone()
            }));
            edges.push(vocab.edge(
                &sub,
                &refs[i],
                format!("covers one part of \"{}\"", parent.title),
            ));
        }
    }

    let has_deliverable = ctx.nodes.iter().any(|n| n.kind == vocab.deliverable);
    if !has_deliverable {
        let sinks: Vec<String> = nodes
            .iter()
            .map(|n| n.reference.clone())
            .filter(|r| {
                !edges
                    .iter()
                    .any(|e| &e.source_ref == r && e.kind == vocab.dependency)
            })
            .collect();
        nodes.push(vocab.node(
            "final".into(),
            "Final deliverable".into(),
            "Combine all upstream results into one polished, well-structured deliverable that fulfils the goal."
                .into(),
            &vocab.deliverable,
        ));
        edges.extend(sinks.iter().map(|s| {
            vocab.edge(
                s,
                "final",
                "its result is combined into the final deliverable".into(),
            )
        }));
    }
    let summary = format!(
        "[demo] Refined {} node(s){}{}; file-producing steps run on the agent runtime. Configure an API key for model-generated plans.",
        ctx.nodes.len(),
        if split {
            ", split the broadest exploratory step into parallel sub-tasks"
        } else {
            ""
        },
        if has_deliverable {
            ""
        } else {
            " and added a final deliverable node"
        },
    );
    PlanProposal {
        summary,
        ontology: Ontology::default(),
        nodes,
        edges,
    }
}

/// Four steps for an empty graph, typed by walking the ontology's stages:
/// the earliest type frames the work, the next explores it, the first type
/// that delivers a file drafts it and the deliverable type finishes it.
fn starter_plan(ctx: &PlanContext, vocab: &Vocabulary) -> PlanProposal {
    let goal = if ctx.goal.trim().is_empty() {
        "the goal"
    } else {
        ctx.goal.trim()
    };
    let mut by_stage: Vec<_> = vocab.ontology.node_types.iter().collect();
    by_stage.sort_by_key(|t| t.stage);
    let key_at = |i: usize| {
        by_stage
            .get(i)
            .or(by_stage.last())
            .map_or(vocab.work.as_str(), |t| t.key.as_str())
    };
    let draft_kind = by_stage
        .iter()
        .find(|t| t.produces_artifact)
        .map_or(vocab.work.as_str(), |t| t.key.as_str());
    let steps = [
        (
            "scope",
            "Clarify scope",
            key_at(0),
            format!("Define the questions, audience and success criteria for: {goal}"),
        ),
        (
            "research",
            "Research",
            key_at(1),
            "Collect the facts, sources and constraints needed.".to_owned(),
        ),
        (
            "draft",
            "Draft",
            draft_kind,
            "Write a complete first draft from the research.".to_owned(),
        ),
        (
            "final",
            "Final deliverable",
            vocab.deliverable.as_str(),
            "Review the draft and produce the final deliverable.".to_owned(),
        ),
    ];
    let nodes = steps
        .iter()
        .map(|(r, t, k, c)| vocab.node((*r).into(), (*t).into(), c.clone(), k))
        .collect();
    let edges = steps
        .windows(2)
        .map(|w| {
            vocab.edge(
                w[0].0,
                w[1].0,
                format!("\"{}\" builds on the result of \"{}\"", w[1].1, w[0].1),
            )
        })
        .collect();
    PlanProposal {
        summary: "[demo] Created a starter plan for an empty graph.".into(),
        ontology: Ontology::default(),
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

    fn node(title: &str, content: &str, kind: &str) -> ContextNode {
        ContextNode {
            id: Uuid::now_v7(),
            title: title.into(),
            kind: kind.into(),
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
            ontology: Ontology::starter(),
            nodes,
            edges,
            suggestions: vec![],
            memories: vec![],
            documents: vec![],
            agent_roles: vec![],
        }
    }

    #[test]
    fn refines_real_nodes() {
        let a = node(
            "Literature review",
            "- history\n- current tools\n- open problems\n- adoption",
            "research",
        );
        let b = node(
            "Write draft",
            "Use the Literature review findings.",
            "document",
        );
        let plan = refine(&ctx(vec![a.clone(), b.clone()], vec![]));
        let existing: HashSet<_> = [a.id, b.id].into();
        let (clean, notes) = sanitize_proposal(plan.clone(), &existing, &Ontology::starter(), 500);
        assert_eq!(clean, plan, "demo plans need no correction: {notes:?}");
        assert_eq!(
            plan.nodes.len(),
            2 + 3 + 1,
            "kept 2, split 3 bullets, added final"
        );
        let linked = |s: &str, t: &str| {
            plan.edges.iter().any(|e| {
                e.source_ref == s
                    && e.target_ref == t
                    && e.kind == "depends_on"
                    && !e.reason.is_empty()
            })
        };
        assert!(linked("n1", "n2"));
        assert!(
            linked("n1-1", "n1"),
            "sub-tasks feed the node they were split from"
        );
        let draft = plan.nodes.iter().find(|n| n.reference == "n2").unwrap();
        assert_eq!(
            draft.executor,
            Executor::Agent,
            "documents run on the agent runtime"
        );
        assert!(plan.nodes.iter().any(|n| n.kind == "output"));
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

    #[test]
    fn a_day_is_counted_back_without_a_model() {
        let digest = "Day: 2026-10-06 (UTC)\nEntries: 3\nBy kind: issue_created 2, run_failed 1\n\
            By person: Bob 2, the system 1\n\n## Entries, oldest first\n\
            09:10 · Bob · issue_created · ENG-1 Login\n\
            09:30 · the system · run_failed · Launch graph · timeout\n";
        let out: crate::domain::insight::DaySummaryOutput =
            serde_json::from_str(&day_summary_json(digest)).unwrap();
        assert!(
            out.headline
                .contains("3 entries were recorded on 2026-10-06 (UTC).")
        );
        assert!(out.highlights[0].contains("issue_created 2, run_failed 1"));
        assert_eq!(
            out.attention,
            ["the system · run_failed · Launch graph · timeout"]
        );
    }
}
