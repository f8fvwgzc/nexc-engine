//! What an agent may do: the capabilities document kept on each agent and
//! carried to the runtime in its per-run token.
//!
//! A capability set is a lattice. `null` on a list means "no restriction";
//! [`Capabilities::narrow`] is the meet of two sets, so a sub-agent's set
//! can only ever lose permissions compared with its parent's, never gain any.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::agent::is_valid_role;
use super::memory::MemoryScope;
use super::string_enum;
use super::validation::FieldErrors;

/// Most entries in one capability list.
pub const LIST_MAX: usize = 64;
/// Longest tool, host or model name.
pub const ENTRY_MAX: usize = 100;
/// Deepest delegation tree a policy may allow (the runtime caps it lower).
pub const DEPTH_MAX: u8 = 8;
/// Most sub-agents one agent may be allowed to spawn.
pub const FAN_OUT_MAX: u8 = 32;

string_enum!(
    /// Classification of a document, a topic or a memory, and the clearance
    /// an agent holds: an agent only receives data whose label is in its
    /// clearance.
    DataLabel {
        Public => "public",
        Internal => "internal",
        Confidential => "confidential",
        Restricted => "restricted",
    }
);

impl DataLabel {
    /// Every label, least sensitive first.
    pub const ALL: [DataLabel; 4] = [
        DataLabel::Public,
        DataLabel::Internal,
        DataLabel::Confidential,
        DataLabel::Restricted,
    ];

    /// Position in the order of sensitivity.
    pub fn rank(self) -> u8 {
        match self {
            DataLabel::Public => 0,
            DataLabel::Internal => 1,
            DataLabel::Confidential => 2,
            DataLabel::Restricted => 3,
        }
    }

    /// The more sensitive of two labels (a passage under a `restricted`
    /// topic is restricted whatever its document says).
    pub fn stricter(self, other: DataLabel) -> DataLabel {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }
}

fn default_labels() -> Vec<DataLabel> {
    vec![DataLabel::Public, DataLabel::Internal]
}

/// What an agent may read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DataScopes {
    /// Knowledge topics whose passages the agent may read; `null` allows every topic.
    #[serde(default)]
    #[schema(required = true)]
    pub topics: Option<Vec<Uuid>>,
    /// The classifications the agent may read: its clearance.
    #[serde(default = "default_labels")]
    #[schema(required = true)]
    pub labels: Vec<DataLabel>,
    /// Memory namespaces (the scope a memory was learned in) the agent may
    /// recall from; `null` allows all of them.
    #[serde(default)]
    #[schema(required = true)]
    pub memory_namespaces: Option<Vec<MemoryScope>>,
}

impl Default for DataScopes {
    fn default() -> Self {
        DataScopes {
            topics: None,
            labels: default_labels(),
            memory_namespaces: None,
        }
    }
}

fn default_depth() -> u8 {
    2
}

fn default_fan_out() -> u8 {
    4
}

/// How far an agent may delegate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Delegation {
    /// Levels of sub-agents that may hang below the agent (0: it may not spawn).
    #[serde(default = "default_depth")]
    #[schema(required = true)]
    pub max_depth: u8,
    /// Sub-agents one agent may spawn during a run.
    #[serde(default = "default_fan_out")]
    #[schema(required = true)]
    pub max_fan_out: u8,
}

impl Default for Delegation {
    fn default() -> Self {
        Delegation {
            max_depth: default_depth(),
            max_fan_out: default_fan_out(),
        }
    }
}

/// An agent's capabilities. `{}` is the default: every tool, every topic,
/// `public` and `internal` data, every memory namespace, every host, any
/// sub-agent role, two levels of delegation with four children each, any model.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    /// Tools the agent may call; `null` allows every tool the runtime offers.
    #[serde(default)]
    #[schema(required = true)]
    pub tools: Option<Vec<String>>,
    #[serde(default)]
    #[schema(required = true)]
    pub data: DataScopes,
    /// Hosts the agent may call out to (`api.example.com`, `*.example.com`);
    /// `null` allows any, `[]` none.
    #[serde(default)]
    #[schema(required = true)]
    pub egress_hosts: Option<Vec<String>>,
    /// Roles of the sub-agents the agent may spawn (and, later, the
    /// principals it may address); `null` allows any.
    #[serde(default)]
    #[schema(required = true)]
    pub counterparts: Option<Vec<String>>,
    #[serde(default)]
    #[schema(required = true)]
    pub delegation: Delegation,
    /// Models the agent may run on; `null` allows any.
    #[serde(default)]
    #[schema(required = true)]
    pub models: Option<Vec<String>>,
}

/// The meet of two allow-lists: whatever both allow. `None` restricts nothing.
fn meet<T: Clone + PartialEq>(a: &Option<Vec<T>>, b: &Option<Vec<T>>) -> Option<Vec<T>> {
    match (a, b) {
        (None, None) => None,
        (Some(a), None) => Some(a.clone()),
        (None, Some(b)) => Some(b.clone()),
        (Some(a), Some(b)) => Some(a.iter().filter(|x| b.contains(x)).cloned().collect()),
    }
}

impl Capabilities {
    /// The capabilities `child` ends up with under `self`: whatever both
    /// allow, the smaller of each limit. Nothing is added, whatever `child`
    /// asks for.
    pub fn narrow(&self, child: &Capabilities) -> Capabilities {
        Capabilities {
            tools: meet(&self.tools, &child.tools),
            data: DataScopes {
                topics: meet(&self.data.topics, &child.data.topics),
                labels: self
                    .data
                    .labels
                    .iter()
                    .filter(|l| child.data.labels.contains(l))
                    .copied()
                    .collect(),
                memory_namespaces: meet(
                    &self.data.memory_namespaces,
                    &child.data.memory_namespaces,
                ),
            },
            egress_hosts: meet(&self.egress_hosts, &child.egress_hosts),
            counterparts: meet(&self.counterparts, &child.counterparts),
            delegation: Delegation {
                max_depth: self.delegation.max_depth.min(child.delegation.max_depth),
                max_fan_out: self
                    .delegation
                    .max_fan_out
                    .min(child.delegation.max_fan_out),
            },
            models: meet(&self.models, &child.models),
        }
    }

    /// Whether every permission in `self` is also in `other`.
    pub fn is_within(&self, other: &Capabilities) -> bool {
        other.narrow(self) == *self
    }

    /// Adds every problem with the document to `errors` (`capabilities.<field>`).
    pub fn check(&self, errors: &mut FieldErrors) {
        fn list(errors: &mut FieldErrors, field: &str, items: &[String], ok: fn(&str) -> bool) {
            if items.len() > LIST_MAX {
                errors.add(field, format!("must have at most {LIST_MAX} entries"));
            }
            if items
                .iter()
                .any(|i| i.trim().is_empty() || i.chars().count() > ENTRY_MAX || !ok(i))
            {
                errors.add(field, "has an entry that is not valid");
            }
        }
        fn is_tool(name: &str) -> bool {
            name.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
        }
        fn is_host(host: &str) -> bool {
            let host = host.strip_prefix("*.").unwrap_or(host);
            let (name, port) = host.split_once(':').unwrap_or((host, "1"));
            !name.is_empty()
                && name.len() <= 253
                && port.parse::<u16>().is_ok_and(|p| p > 0)
                && name.split('.').all(|part| {
                    !part.is_empty()
                        && !part.starts_with('-')
                        && !part.ends_with('-')
                        && part
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                })
        }
        fn is_model(_: &str) -> bool {
            true
        }
        if let Some(tools) = &self.tools {
            list(errors, "capabilities.tools", tools, is_tool);
        }
        if self
            .data
            .topics
            .as_ref()
            .is_some_and(|t| t.len() > LIST_MAX)
        {
            errors.add(
                "capabilities.data.topics",
                format!("must have at most {LIST_MAX} entries"),
            );
        }
        if self.data.labels.len() > DataLabel::ALL.len() {
            errors.add("capabilities.data.labels", "repeats a label");
        }
        if let Some(hosts) = &self.egress_hosts {
            list(errors, "capabilities.egress_hosts", hosts, is_host);
        }
        if let Some(roles) = &self.counterparts {
            list(errors, "capabilities.counterparts", roles, is_valid_role);
        }
        if self.delegation.max_depth > DEPTH_MAX {
            errors.add(
                "capabilities.delegation.max_depth",
                format!("must be at most {DEPTH_MAX}"),
            );
        }
        if self.delegation.max_fan_out > FAN_OUT_MAX {
            errors.add(
                "capabilities.delegation.max_fan_out",
                format!("must be at most {FAN_OUT_MAX}"),
            );
        }
        if let Some(models) = &self.models {
            list(errors, "capabilities.models", models, is_model);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn some(items: &[&str]) -> Option<Vec<String>> {
        Some(items.iter().map(|s| (*s).to_owned()).collect())
    }

    #[test]
    fn the_empty_document_is_the_default_policy() {
        let parsed: Capabilities = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, Capabilities::default());
        assert_eq!(parsed.tools, None);
        assert_eq!(
            parsed.data.labels,
            vec![DataLabel::Public, DataLabel::Internal]
        );
        assert_eq!(
            (parsed.delegation.max_depth, parsed.delegation.max_fan_out),
            (2, 4)
        );
        assert!(serde_json::from_value::<Capabilities>(json!({"typo": 1})).is_err());
        assert!(
            serde_json::from_value::<Capabilities>(json!({"data": {"labels": ["secret"]}}))
                .is_err()
        );
        // What is stored comes back the same.
        let again: Capabilities =
            serde_json::from_value(serde_json::to_value(&parsed).unwrap()).unwrap();
        assert_eq!(again, parsed);
    }

    #[test]
    fn narrowing_only_removes() {
        let parent = Capabilities {
            tools: some(&["write_file", "read_file", "finish"]),
            data: DataScopes {
                topics: None,
                labels: vec![
                    DataLabel::Public,
                    DataLabel::Internal,
                    DataLabel::Confidential,
                ],
                memory_namespaces: Some(vec![MemoryScope::Graph]),
            },
            egress_hosts: Some(Vec::new()),
            counterparts: some(&["researcher"]),
            delegation: Delegation {
                max_depth: 2,
                max_fan_out: 3,
            },
            models: None,
        };
        // A child asking for more gets the parent's limits; asking for less, its own.
        let greedy = Capabilities {
            tools: some(&["write_file", "make_docx", "run_python"]),
            data: DataScopes {
                topics: Some(vec![Uuid::nil()]),
                labels: DataLabel::ALL.to_vec(),
                memory_namespaces: None,
            },
            egress_hosts: some(&["evil.example"]),
            counterparts: None,
            delegation: Delegation {
                max_depth: 8,
                max_fan_out: 1,
            },
            models: some(&["claude-opus-5"]),
        };
        let child = parent.narrow(&greedy);
        assert_eq!(child.tools, some(&["write_file"]));
        assert_eq!(child.data.topics, Some(vec![Uuid::nil()]));
        assert_eq!(
            child.data.labels,
            vec![
                DataLabel::Public,
                DataLabel::Internal,
                DataLabel::Confidential
            ]
        );
        assert_eq!(child.data.memory_namespaces, Some(vec![MemoryScope::Graph]));
        assert_eq!(child.egress_hosts, Some(Vec::new()));
        assert_eq!(child.counterparts, some(&["researcher"]));
        assert_eq!(
            (child.delegation.max_depth, child.delegation.max_fan_out),
            (2, 1)
        );
        assert_eq!(child.models, some(&["claude-opus-5"]));
        assert!(child.is_within(&parent));
        assert!(!greedy.is_within(&parent));
        assert!(parent.is_within(&parent));
        // The unrestricted policy narrows to whatever it meets, and never widens it.
        let top = Capabilities {
            data: DataScopes {
                labels: DataLabel::ALL.to_vec(),
                ..DataScopes::default()
            },
            ..Capabilities::default()
        };
        assert_eq!(top.narrow(&parent), parent);
        assert_eq!(parent.narrow(&top), parent);
        assert!(
            !top.is_within(&parent),
            "unrestricted is wider than any list"
        );
        assert_eq!(parent.narrow(&parent), parent, "narrowing is idempotent");
        // The default clearance is `public` and `internal`: a sub-agent born under a
        // confidential-cleared parent with the default document loses `confidential`.
        assert_eq!(
            parent.narrow(&Capabilities::default()).data.labels,
            vec![DataLabel::Public, DataLabel::Internal]
        );
    }

    #[test]
    fn labels_order_and_stricter() {
        assert_eq!(
            DataLabel::Internal.stricter(DataLabel::Restricted),
            DataLabel::Restricted
        );
        assert_eq!(
            DataLabel::Restricted.stricter(DataLabel::Public),
            DataLabel::Restricted
        );
        assert!(DataLabel::ALL.windows(2).all(|w| w[0].rank() < w[1].rank()));
    }

    #[test]
    fn checks_the_shape_of_entries() {
        let mut errors = FieldErrors::default();
        Capabilities::default().check(&mut errors);
        assert!(errors.is_empty());
        let bad = Capabilities {
            tools: some(&["write file"]),
            egress_hosts: some(&["Bad_Host", "api.example.com:0"]),
            counterparts: some(&["QA"]),
            delegation: Delegation {
                max_depth: 9,
                max_fan_out: 33,
            },
            models: some(&[""]),
            ..Capabilities::default()
        };
        let mut errors = FieldErrors::default();
        bad.check(&mut errors);
        let fields: Vec<&String> = errors.as_map().keys().collect();
        assert_eq!(
            fields,
            [
                "capabilities.counterparts",
                "capabilities.delegation.max_depth",
                "capabilities.delegation.max_fan_out",
                "capabilities.egress_hosts",
                "capabilities.models",
                "capabilities.tools",
            ]
        );
        let good = Capabilities {
            egress_hosts: some(&["api.example.com", "*.example.com", "localhost:8443"]),
            ..Capabilities::default()
        };
        let mut errors = FieldErrors::default();
        good.check(&mut errors);
        assert!(errors.is_empty(), "{:?}", errors.as_map());
    }
}
