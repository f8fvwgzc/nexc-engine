//! The ontology of a graph: which types of node exist and which typed
//! relations may connect them. Every graph owns its ontology as data; nothing
//! about node or relation types is compiled in except the starter set a new
//! graph begins with, which the user and the planner are free to rewrite.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::graph::Executor;
use super::validation::{FieldErrors, Validate};

/// Maximum number of node types and of relation types in one ontology.
pub const TYPES_MAX: usize = 40;
/// Maximum type key length.
pub const KEY_MAX: usize = 40;
/// Maximum type label length.
pub const LABEL_MAX: usize = 60;
/// Maximum type description length in characters.
pub const DESCRIPTION_MAX: usize = 500;
/// Maximum length of an edge's reason in characters.
pub const REASON_MAX: usize = 500;

/// Colours handed to types that do not choose one, picked by key hash.
const PALETTE: [&str; 8] = [
    "#6366f1", "#0ea5e9", "#10b981", "#f59e0b", "#ef4444", "#a855f7", "#14b8a6", "#ec4899",
];
/// Icon names the UI can draw for a node type (`frontend/src/components/custom-ui/node-kind-meta.ts`);
/// any other name is shown as a dot.
pub const ICONS: [&str; 25] = [
    "bar-chart",
    "book-open",
    "brain",
    "calendar",
    "chart",
    "circle",
    "code",
    "database",
    "file-text",
    "flag",
    "flask",
    "globe",
    "hash",
    "lightbulb",
    "list-todo",
    "message",
    "puzzle",
    "scale",
    "search",
    "shield",
    "target",
    "trending-up",
    "users",
    "wrench",
    "zap",
];
const FALLBACK_ROLE: &str = "engineer";
const FALLBACK_ICON: &str = "circle";

/// A type of node. Its attributes drive everything the engine used to derive
/// from a fixed kind: default agent role and executor, ordering of suggested
/// dependencies, artifact production and code execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NodeType {
    /// Stable identifier stored on nodes (`[a-z][a-z0-9_]*`).
    pub key: String,
    pub label: String,
    /// What a node of this type means; shown to the planner and to agents.
    #[serde(default)]
    #[schema(required = true)]
    pub description: String,
    /// `#rrggbb` colour of the type on the canvas.
    #[serde(default)]
    #[schema(required = true)]
    pub color: String,
    /// Icon name from the UI's icon vocabulary (unknown names fall back to a dot).
    #[serde(default)]
    #[schema(required = true)]
    pub icon: String,
    /// Agent role used when a node of this type sets none.
    #[serde(default)]
    #[schema(required = true)]
    pub default_role: String,
    #[serde(default = "default_executor")]
    #[schema(required = true)]
    pub default_executor: Executor,
    /// Position in the usual flow of work: suggested dependencies point from
    /// lower to higher stages.
    #[serde(default)]
    #[schema(required = true)]
    pub stage: i32,
    /// Nodes of this type are expected to deliver a file.
    #[serde(default)]
    #[schema(required = true)]
    pub produces_artifact: bool,
    /// Agents running nodes of this type may execute code.
    #[serde(default)]
    #[schema(required = true)]
    pub allow_code_exec: bool,
}

fn default_executor() -> Executor {
    Executor::Llm
}

/// A type of relation between two nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RelationType {
    /// Stable identifier stored on edges (`[a-z][a-z0-9_]*`).
    pub key: String,
    pub label: String,
    /// What the relation asserts about source and target.
    #[serde(default)]
    #[schema(required = true)]
    pub description: String,
    /// The source must finish before the target runs. Blocking relations form
    /// the execution DAG and may not close a cycle.
    #[serde(default)]
    #[schema(required = true)]
    pub blocking: bool,
}

/// The node types and relation types of one graph.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Ontology {
    #[serde(default)]
    #[schema(required = true)]
    pub node_types: Vec<NodeType>,
    #[serde(default)]
    #[schema(required = true)]
    pub relation_types: Vec<RelationType>,
}

impl Ontology {
    /// The ontology a new graph starts with.
    pub fn starter() -> Ontology {
        let node = |key: &str,
                    label: &str,
                    description: &str,
                    color: &str,
                    icon: &str,
                    role: &str,
                    stage: i32| NodeType {
            key: key.into(),
            label: label.into(),
            description: description.into(),
            color: color.into(),
            icon: icon.into(),
            default_role: role.into(),
            default_executor: Executor::Llm,
            stage,
            produces_artifact: false,
            allow_code_exec: false,
        };
        let relation = |key: &str, label: &str, description: &str, blocking: bool| RelationType {
            key: key.into(),
            label: label.into(),
            description: description.into(),
            blocking,
        };
        Ontology {
            node_types: vec![
                node(
                    "topic",
                    "Topic",
                    "A subject or idea that frames the work.",
                    "#6366f1",
                    "hash",
                    "planner",
                    0,
                ),
                node(
                    "research",
                    "Research",
                    "Finding and summarising information.",
                    "#0ea5e9",
                    "book-open",
                    "researcher",
                    1,
                ),
                node(
                    "task",
                    "Task",
                    "A concrete step to carry out.",
                    "#10b981",
                    "list-todo",
                    "engineer",
                    2,
                ),
                NodeType {
                    allow_code_exec: true,
                    ..node(
                        "code",
                        "Code",
                        "Writing or changing source code.",
                        "#f59e0b",
                        "code",
                        "engineer",
                        2,
                    )
                },
                NodeType {
                    produces_artifact: true,
                    ..node(
                        "document",
                        "Document",
                        "A written deliverable.",
                        "#a855f7",
                        "file-text",
                        "writer",
                        3,
                    )
                },
                NodeType {
                    produces_artifact: true,
                    ..node(
                        "output",
                        "Output",
                        "The final deliverable of the graph.",
                        "#ef4444",
                        "flag",
                        "writer",
                        4,
                    )
                },
            ],
            relation_types: vec![
                relation(
                    "depends_on",
                    "Depends on",
                    "The source must finish before the target runs; its output is the target's context.",
                    true,
                ),
                relation(
                    "relates_to",
                    "Relates to",
                    "The two nodes are about the same thing; no ordering.",
                    false,
                ),
            ],
        }
    }

    /// The node type with `key`.
    pub fn node_type(&self, key: &str) -> Option<&NodeType> {
        self.node_types.iter().find(|t| t.key == key)
    }

    /// The relation type with `key`.
    pub fn relation_type(&self, key: &str) -> Option<&RelationType> {
        self.relation_types.iter().find(|t| t.key == key)
    }

    /// Key of the type a node gets when the caller names none: the first
    /// type whose nodes neither frame the work nor deliver it, else the first.
    pub fn default_node_kind(&self) -> Option<&str> {
        self.node_types
            .iter()
            .find(|t| !t.produces_artifact && t.default_role == FALLBACK_ROLE)
            .or(self.node_types.first())
            .map(|t| t.key.as_str())
    }

    /// Key of the relation an edge gets when the caller names none: the first
    /// blocking relation, else the first.
    pub fn default_relation(&self) -> Option<&RelationType> {
        self.relation_types
            .iter()
            .find(|t| t.blocking)
            .or(self.relation_types.first())
    }

    /// The first blocking relation, i.e. the one that expresses a dependency.
    pub fn dependency_relation(&self) -> Option<&RelationType> {
        self.relation_types.iter().find(|t| t.blocking)
    }

    /// Agent role for a node of type `kind` that sets none.
    pub fn role_for(&self, kind: &str) -> &str {
        self.node_type(kind)
            .map(|t| t.default_role.as_str())
            .filter(|r| !r.is_empty())
            .unwrap_or(FALLBACK_ROLE)
    }

    /// Stage of type `kind` (unknown types sort in the middle of the starter flow).
    pub fn stage_of(&self, kind: &str) -> i32 {
        self.node_type(kind).map_or(2, |t| t.stage)
    }

    /// Whether nodes of type `kind` deliver a file.
    pub fn produces_artifact(&self, kind: &str) -> bool {
        self.node_type(kind).is_some_and(|t| t.produces_artifact)
    }

    /// Whether agents running nodes of type `kind` may execute code.
    pub fn allows_code_exec(&self, kind: &str) -> bool {
        self.node_type(kind).is_some_and(|t| t.allow_code_exec)
    }

    /// Brings every type into canonical form: slug keys, trimmed and bounded
    /// text, a colour and an icon for every node type. Types whose key is
    /// empty after slugging, or repeats an earlier one, are dropped.
    pub fn normalize(&mut self) {
        let mut seen = Vec::new();
        self.node_types.retain_mut(|t| {
            t.key = slug(&t.key);
            t.label = bounded(&t.label, LABEL_MAX);
            if t.label.is_empty() {
                t.label = label_from_key(&t.key);
            }
            t.description = bounded(&t.description, DESCRIPTION_MAX);
            t.default_role = bounded(&t.default_role, super::graph::ROLE_MAX);
            if !is_hex_color(&t.color) {
                t.color = palette_color(&t.key).into();
            }
            t.icon = slug(&t.icon).replace('_', "-");
            if t.icon.is_empty() {
                t.icon = FALLBACK_ICON.into();
            }
            !t.key.is_empty() && !seen.contains(&t.key) && {
                seen.push(t.key.clone());
                true
            }
        });
        seen.clear();
        self.relation_types.retain_mut(|t| {
            t.key = slug(&t.key);
            t.label = bounded(&t.label, LABEL_MAX);
            if t.label.is_empty() {
                t.label = label_from_key(&t.key);
            }
            t.description = bounded(&t.description, DESCRIPTION_MAX);
            !t.key.is_empty() && !seen.contains(&t.key) && {
                seen.push(t.key.clone());
                true
            }
        });
    }

    /// Adds the types of `other` whose keys are not defined yet (bounded by
    /// [`TYPES_MAX`]). Returns true when anything was added.
    pub fn merge(&mut self, other: &Ontology) -> bool {
        let before = (self.node_types.len(), self.relation_types.len());
        for t in &other.node_types {
            if self.node_type(&t.key).is_none() && self.node_types.len() < TYPES_MAX {
                self.node_types.push(t.clone());
            }
        }
        for t in &other.relation_types {
            if self.relation_type(&t.key).is_none() && self.relation_types.len() < TYPES_MAX {
                self.relation_types.push(t.clone());
            }
        }
        before != (self.node_types.len(), self.relation_types.len())
    }

    /// Declares a node type for `key` if none exists, deriving its label and
    /// colour from the key. Returns false when the ontology is full.
    pub fn ensure_node_type(&mut self, key: &str) -> bool {
        if self.node_type(key).is_some() {
            return true;
        }
        if self.node_types.len() >= TYPES_MAX || key.is_empty() {
            return false;
        }
        self.node_types.push(NodeType {
            key: key.to_owned(),
            label: label_from_key(key),
            description: String::new(),
            color: palette_color(key).into(),
            icon: FALLBACK_ICON.into(),
            default_role: String::new(),
            default_executor: Executor::Llm,
            stage: 2,
            produces_artifact: false,
            allow_code_exec: false,
        });
        true
    }

    /// Declares a relation type for `key` if none exists. Returns false when
    /// the ontology is full.
    pub fn ensure_relation_type(&mut self, key: &str, blocking: bool) -> bool {
        if self.relation_type(key).is_some() {
            return true;
        }
        if self.relation_types.len() >= TYPES_MAX || key.is_empty() {
            return false;
        }
        self.relation_types.push(RelationType {
            key: key.to_owned(),
            label: label_from_key(key),
            description: String::new(),
            blocking,
        });
        true
    }
}

/// A submitted ontology needs at least one type of each sort and at most
/// [`TYPES_MAX`]; text bounds are enforced by [`Ontology::normalize`].
impl Validate for Ontology {
    fn validate(&self, errors: &mut FieldErrors) {
        if self.node_types.is_empty() {
            errors.add("node_types", "an ontology needs at least one node type");
        }
        if self.relation_types.is_empty() {
            errors.add(
                "relation_types",
                "an ontology needs at least one relation type",
            );
        }
        if self.node_types.len() > TYPES_MAX {
            errors.add("node_types", format!("at most {TYPES_MAX} node types"));
        }
        if self.relation_types.len() > TYPES_MAX {
            errors.add(
                "relation_types",
                format!("at most {TYPES_MAX} relation types"),
            );
        }
    }
}

/// Canonical type key: lowercase ASCII letters, digits and `_`, starting with
/// a letter, at most [`KEY_MAX`] characters. Other characters become `_`.
pub fn slug(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.trim().chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_start_matches(|c: char| c.is_ascii_digit() || c == '_');
    out.trim_end_matches('_').chars().take(KEY_MAX).collect()
}

/// `market_signal` -> `Market signal`.
fn label_from_key(key: &str) -> String {
    let text = key.replace('_', " ");
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn bounded(text: &str, max: usize) -> String {
    text.trim().chars().take(max).collect()
}

fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

fn palette_color(key: &str) -> &'static str {
    let hash = key
        .bytes()
        .fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize));
    PALETTE[hash % PALETTE.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_is_valid_and_canonical() {
        let starter = Ontology::starter();
        let mut normalized = starter.clone();
        normalized.normalize();
        assert_eq!(starter, normalized);
        let mut errors = FieldErrors::default();
        starter.validate(&mut errors);
        assert!(errors.is_empty());
        assert_eq!(starter.default_node_kind(), Some("task"));
        assert_eq!(starter.default_relation().unwrap().key, "depends_on");
        assert!(starter.produces_artifact("output"));
        assert!(starter.allows_code_exec("code"));
        assert_eq!(starter.role_for("research"), "researcher");
        assert_eq!(starter.role_for("unknown"), "engineer");
    }

    #[test]
    fn slugs_keys() {
        assert_eq!(slug("  Market Signal! "), "market_signal");
        assert_eq!(slug("9 lives"), "lives");
        assert_eq!(slug("risk--review_"), "risk_review");
        assert_eq!(slug("***"), "");
    }

    #[test]
    fn normalizes_and_merges() {
        let mut o: Ontology = serde_json::from_value(serde_json::json!({
            "node_types": [
                { "key": "Market Signal", "label": "" },
                { "key": "market_signal", "label": "dup" },
                { "key": "!!", "label": "nameless" }
            ],
            "relation_types": [{ "key": "Validates", "label": "Validates", "blocking": true }]
        }))
        .unwrap();
        o.normalize();
        assert_eq!(o.node_types.len(), 1);
        assert_eq!(o.node_types[0].key, "market_signal");
        assert_eq!(o.node_types[0].label, "Market signal");
        assert!(is_hex_color(&o.node_types[0].color));
        assert_eq!(o.node_types[0].icon, "circle");

        let mut base = Ontology::starter();
        assert!(base.merge(&o));
        assert!(!base.merge(&o), "merging twice adds nothing");
        assert!(base.relation_type("validates").unwrap().blocking);
        assert!(base.ensure_node_type("hypothesis"));
        assert_eq!(base.node_type("hypothesis").unwrap().label, "Hypothesis");
    }
}
