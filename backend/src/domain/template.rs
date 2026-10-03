//! Built-in starter graphs, embedded from `backend/templates/*.json`.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::graph::{Executor, NodeKind};

/// A template as listed by `GET /templates`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct GraphTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub node_count: i64,
    pub tags: Vec<String>,
}

/// A node of a template; `key` is referenced by the template's edges.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateNode {
    pub key: String,
    pub title: String,
    pub content: String,
    pub kind: NodeKind,
    #[serde(default)]
    pub tags: Vec<String>,
    pub x: f64,
    pub y: f64,
    #[serde(default = "default_executor")]
    pub executor: Executor,
    #[serde(default)]
    pub agent_role: Option<String>,
}

fn default_executor() -> Executor {
    Executor::Llm
}

/// A `depends_on` edge between template node keys.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateEdge {
    pub source: String,
    pub target: String,
}

/// A complete starter graph.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateSpec {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub tags: Vec<String>,
    pub goal: String,
    pub nodes: Vec<TemplateNode>,
    pub edges: Vec<TemplateEdge>,
}

impl TemplateSpec {
    /// The list view of this template.
    pub fn summary(&self) -> GraphTemplate {
        GraphTemplate {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            category: self.category.clone(),
            node_count: self.nodes.len() as i64,
            tags: self.tags.clone(),
        }
    }

    /// Edges as `(source index, target index)` into [`TemplateSpec::nodes`].
    pub fn edge_indices(&self) -> Vec<(usize, usize)> {
        let index: HashMap<&str, usize> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.key.as_str(), i))
            .collect();
        self.edges
            .iter()
            .filter_map(|e| {
                Some((
                    *index.get(e.source.as_str())?,
                    *index.get(e.target.as_str())?,
                ))
            })
            .collect()
    }
}

const SOURCES: [&str; 6] = [
    include_str!("../../templates/research-report-docx.json"),
    include_str!("../../templates/rest-api-service.json"),
    include_str!("../../templates/market-analysis.json"),
    include_str!("../../templates/blog-series.json"),
    include_str!("../../templates/data-pipeline.json"),
    include_str!("../../templates/product-launch-plan.json"),
];

/// All built-in templates (parsed once; the files are validated by tests).
pub fn builtin() -> &'static [TemplateSpec] {
    static TEMPLATES: OnceLock<Vec<TemplateSpec>> = OnceLock::new();
    TEMPLATES.get_or_init(|| {
        SOURCES
            .iter()
            .map(|src| {
                serde_json::from_str(src).expect("built-in template JSON is validated by tests")
            })
            .collect()
    })
}

/// Looks up a built-in template by id.
pub fn find(id: &str) -> Option<&'static TemplateSpec> {
    builtin().iter().find(|t| t.id == id)
}

/// Maximum length of the topic given when instantiating a template.
pub const TEMPLATE_TOPIC_MAX: usize = 2000;

/// Goal of a template instance: the user's topic first (every prompt includes the goal), then
/// the template's generic goal.
pub fn instance_goal(template_goal: &str, topic: Option<&str>) -> String {
    match topic {
        Some(topic) => format!("Topic: {topic}\n\n{template_goal}"),
        None => template_goal.to_owned(),
    }
}

/// Default graph name of a template instance: `<template> · <topic>` (topic shortened).
pub fn instance_name(template_name: &str, topic: Option<&str>) -> String {
    const TOPIC_CHARS: usize = 60;
    match topic {
        Some(topic) if topic.chars().count() > TOPIC_CHARS => {
            let short: String = topic.chars().take(TOPIC_CHARS - 1).collect();
            format!("{template_name} · {}…", short.trim_end())
        }
        Some(topic) => format!("{template_name} · {topic}"),
        None => template_name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn topic_leads_the_goal_and_names_the_graph() {
        assert_eq!(instance_goal("Write it.", None), "Write it.");
        assert_eq!(
            instance_goal("Write it.", Some("Solid-state batteries")),
            "Topic: Solid-state batteries\n\nWrite it."
        );
        assert_eq!(instance_name("Research report", None), "Research report");
        assert_eq!(
            instance_name("Research report", Some("Solid-state batteries")),
            "Research report · Solid-state batteries"
        );
        let long = "x".repeat(100);
        assert_eq!(
            instance_name("R", Some(&long)).chars().count(),
            "R · ".chars().count() + 60
        );
    }
    use crate::domain::graph::{CONTENT_MAX_BYTES, TITLE_MAX, wikilinks};
    use crate::dsa::graph::DiGraph;

    #[test]
    fn templates_are_valid_dags() {
        let all = builtin();
        assert!(all.len() >= 6);
        let ids: HashSet<_> = all.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids.len(), all.len(), "template ids are unique");
        for t in all {
            assert!(
                (5..=10).contains(&t.nodes.len()),
                "{} has {} nodes",
                t.id,
                t.nodes.len()
            );
            let keys: HashSet<_> = t.nodes.iter().map(|n| n.key.as_str()).collect();
            assert_eq!(keys.len(), t.nodes.len(), "{}: duplicate keys", t.id);
            assert_eq!(
                t.edge_indices().len(),
                t.edges.len(),
                "{}: dangling edge",
                t.id
            );
            let mut g = DiGraph::new(t.nodes.len());
            for (s, d) in t.edge_indices() {
                assert!(!g.would_create_cycle(s, d), "{}: cycle", t.id);
                g.add_edge(s, d);
            }
            let titles: HashSet<_> = t.nodes.iter().map(|n| n.title.to_lowercase()).collect();
            for n in &t.nodes {
                assert!(
                    n.title.chars().count() <= TITLE_MAX && n.content.len() <= CONTENT_MAX_BYTES
                );
                for link in wikilinks(&n.content) {
                    assert!(titles.contains(&link), "{}: [[{link}]] has no node", t.id);
                }
            }
        }
        assert!(find("research-report-docx").is_some());
        assert!(find("nope").is_none());
    }
}
