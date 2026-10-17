//! Policy decisions: pure functions from an agent's capabilities and an
//! action to allow or deny, with the rule that decided. They run in-process
//! at every enforcement point (the runtime's tool loop, the backend's
//! context builder and dispatch, the re-check of every reported span), so
//! they take microseconds and never do IO.

use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::capabilities::{Capabilities, DataLabel};
use super::memory::MemoryScope;

/// Something an agent is about to do, or did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action<'a> {
    /// Call the tool.
    Tool(&'a str),
    /// Spawn a sub-agent of `role` at `depth` (the child's depth; the root
    /// is 0), having spawned `children` already.
    Spawn {
        role: &'a str,
        depth: u8,
        children: u8,
    },
    /// Run on the model.
    Model(&'a str),
    /// Call out to the host.
    Egress(&'a str),
    /// Be given a passage or a memory.
    Read {
        label: DataLabel,
        topic: Option<Uuid>,
        namespace: Option<MemoryScope>,
    },
}

impl Action<'_> {
    /// The kind of action, as recorded on a `policy` span.
    pub fn kind(&self) -> &'static str {
        match self {
            Action::Tool(_) => "tool_call",
            Action::Spawn { .. } => "spawn",
            Action::Model(_) => "model",
            Action::Egress(_) => "egress",
            Action::Read { .. } => "data_read",
        }
    }

    /// What the action was about: the tool, the role, the model, the host.
    pub fn subject(&self) -> String {
        match self {
            Action::Tool(name) | Action::Model(name) | Action::Egress(name) => (*name).to_owned(),
            Action::Spawn { role, .. } => (*role).to_owned(),
            Action::Read { label, .. } => label.as_str().to_owned(),
        }
    }
}

/// The outcome of a check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Decision {
    pub allowed: bool,
    /// The rule that decided (`tools.allow`, `delegation.max_depth`, ...).
    pub rule: &'static str,
    /// Why, in words.
    pub reason: String,
}

impl Decision {
    fn allow(rule: &'static str, reason: impl Into<String>) -> Self {
        Decision {
            allowed: true,
            rule,
            reason: reason.into(),
        }
    }

    fn deny(rule: &'static str, reason: impl Into<String>) -> Self {
        Decision {
            allowed: false,
            rule,
            reason: reason.into(),
        }
    }

    /// `allow` or `deny`, as recorded on a span.
    pub fn verdict(&self) -> &'static str {
        if self.allowed { "allow" } else { "deny" }
    }
}

/// Whether `host` is one of `allowed`: exact, or under a `*.` pattern.
fn host_allowed(allowed: &[String], host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    allowed.iter().any(|entry| {
        let entry = entry.to_ascii_lowercase();
        match entry.strip_prefix("*.") {
            Some(suffix) => {
                host.ends_with(suffix)
                    && host.len() > suffix.len() + 1
                    && host[..host.len() - suffix.len()].ends_with('.')
            }
            None => entry == host,
        }
    })
}

/// Decides `action` under `caps`.
pub fn decide(caps: &Capabilities, action: &Action<'_>) -> Decision {
    match action {
        Action::Tool(name) => match &caps.tools {
            None => Decision::allow("tools.allow", "no tool restriction"),
            Some(tools) if tools.iter().any(|t| t == name) => {
                Decision::allow("tools.allow", format!("{name} is in the allow-list"))
            }
            Some(_) => Decision::deny("tools.allow", format!("tool {name} is not allowed")),
        },
        Action::Spawn {
            role,
            depth,
            children,
        } => {
            let limits = caps.delegation;
            if *depth > limits.max_depth {
                return Decision::deny(
                    "delegation.max_depth",
                    format!("depth {depth} exceeds the limit of {}", limits.max_depth),
                );
            }
            if *children >= limits.max_fan_out {
                return Decision::deny(
                    "delegation.max_fan_out",
                    format!("already spawned {} sub-agents", limits.max_fan_out),
                );
            }
            match &caps.counterparts {
                Some(roles) if !roles.iter().any(|r| r == role) => {
                    Decision::deny("counterparts", format!("role {role} is not a counterpart"))
                }
                _ => Decision::allow("delegation", format!("may spawn a {role} at depth {depth}")),
            }
        }
        Action::Model(model) => match &caps.models {
            None => Decision::allow("models.allow", "no model restriction"),
            Some(models) if models.iter().any(|m| m == model) => {
                Decision::allow("models.allow", format!("{model} is in the allow-list"))
            }
            Some(_) => Decision::deny("models.allow", format!("model {model} is not allowed")),
        },
        Action::Egress(host) => match &caps.egress_hosts {
            None => Decision::allow("egress.hosts", "no host restriction"),
            Some(hosts) if host_allowed(hosts, host) => {
                Decision::allow("egress.hosts", format!("{host} is in the allow-list"))
            }
            Some(_) => Decision::deny("egress.hosts", format!("host {host} is not allowed")),
        },
        Action::Read {
            label,
            topic,
            namespace,
        } => {
            if !caps.data.labels.contains(label) {
                return Decision::deny("data.labels", format!("no clearance for {label} data"));
            }
            if let (Some(allowed), Some(topic)) = (&caps.data.topics, topic)
                && !allowed.contains(topic)
            {
                return Decision::deny("data.topics", format!("topic {topic} is not in scope"));
            }
            if let (Some(allowed), Some(namespace)) = (&caps.data.memory_namespaces, namespace)
                && !allowed.contains(namespace)
            {
                return Decision::deny(
                    "data.memory_namespaces",
                    format!("memory namespace {namespace} is not in scope"),
                );
            }
            Decision::allow("data", format!("{label} data is within clearance"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::capabilities::{DataScopes, Delegation};

    fn some(items: &[&str]) -> Option<Vec<String>> {
        Some(items.iter().map(|s| (*s).to_owned()).collect())
    }

    #[test]
    fn the_default_policy_allows_tools_models_and_internal_data() {
        let caps = Capabilities::default();
        assert!(decide(&caps, &Action::Tool("make_docx")).allowed);
        assert!(decide(&caps, &Action::Model("anything")).allowed);
        assert!(decide(&caps, &Action::Egress("api.example.com")).allowed);
        assert!(
            decide(
                &caps,
                &Action::Spawn {
                    role: "researcher",
                    depth: 2,
                    children: 3
                }
            )
            .allowed
        );
        let read = |label| Action::Read {
            label,
            topic: None,
            namespace: Some(MemoryScope::User),
        };
        assert!(decide(&caps, &read(DataLabel::Internal)).allowed);
        let denied = decide(&caps, &read(DataLabel::Restricted));
        assert!(!denied.allowed);
        assert_eq!(denied.rule, "data.labels");
        assert_eq!(denied.verdict(), "deny");
    }

    #[test]
    fn every_rule_denies_with_its_name() {
        let topic = Uuid::now_v7();
        let caps = Capabilities {
            tools: some(&["write_file", "finish"]),
            data: DataScopes {
                topics: Some(vec![topic]),
                labels: vec![DataLabel::Public],
                memory_namespaces: Some(vec![MemoryScope::Graph]),
            },
            egress_hosts: some(&["api.example.com", "*.internal.example"]),
            counterparts: some(&["researcher"]),
            delegation: Delegation {
                max_depth: 1,
                max_fan_out: 2,
            },
            models: some(&["claude-opus-5"]),
        };
        let deny = |action: Action<'_>| {
            let d = decide(&caps, &action);
            assert!(!d.allowed, "{action:?} should be denied");
            d.rule
        };
        assert_eq!(deny(Action::Tool("make_docx")), "tools.allow");
        assert!(decide(&caps, &Action::Tool("finish")).allowed);
        assert_eq!(deny(Action::Model("gpt-x")), "models.allow");
        assert_eq!(deny(Action::Egress("evil.example")), "egress.hosts");
        assert_eq!(deny(Action::Egress("internal.example")), "egress.hosts");
        assert!(decide(&caps, &Action::Egress("Db.Internal.Example")).allowed);
        assert!(decide(&caps, &Action::Egress("API.example.com")).allowed);
        assert_eq!(
            deny(Action::Spawn {
                role: "researcher",
                depth: 2,
                children: 0
            }),
            "delegation.max_depth"
        );
        assert_eq!(
            deny(Action::Spawn {
                role: "researcher",
                depth: 1,
                children: 2
            }),
            "delegation.max_fan_out"
        );
        assert_eq!(
            deny(Action::Spawn {
                role: "editor",
                depth: 1,
                children: 0
            }),
            "counterparts"
        );
        let ok = decide(
            &caps,
            &Action::Spawn {
                role: "researcher",
                depth: 1,
                children: 1,
            },
        );
        assert!(ok.allowed && ok.rule == "delegation");
        assert_eq!(
            deny(Action::Read {
                label: DataLabel::Internal,
                topic: None,
                namespace: None
            }),
            "data.labels"
        );
        assert_eq!(
            deny(Action::Read {
                label: DataLabel::Public,
                topic: Some(Uuid::now_v7()),
                namespace: None
            }),
            "data.topics"
        );
        assert_eq!(
            deny(Action::Read {
                label: DataLabel::Public,
                topic: Some(topic),
                namespace: Some(MemoryScope::User)
            }),
            "data.memory_namespaces"
        );
        assert!(
            decide(
                &caps,
                &Action::Read {
                    label: DataLabel::Public,
                    topic: None,
                    namespace: Some(MemoryScope::Graph)
                }
            )
            .allowed,
            "a passage without a topic is in every topic scope"
        );
        assert_eq!(Action::Tool("x").kind(), "tool_call");
        assert_eq!(
            Action::Spawn {
                role: "qa",
                depth: 1,
                children: 0
            }
            .subject(),
            "qa"
        );
    }

    #[test]
    fn empty_lists_allow_nothing() {
        let caps = Capabilities {
            tools: Some(Vec::new()),
            egress_hosts: Some(Vec::new()),
            counterparts: Some(Vec::new()),
            models: Some(Vec::new()),
            delegation: Delegation {
                max_depth: 0,
                max_fan_out: 0,
            },
            ..Capabilities::default()
        };
        assert!(!decide(&caps, &Action::Tool("finish")).allowed);
        assert!(!decide(&caps, &Action::Egress("a.b")).allowed);
        assert!(!decide(&caps, &Action::Model("m")).allowed);
        assert!(
            !decide(
                &caps,
                &Action::Spawn {
                    role: "r",
                    depth: 1,
                    children: 0
                }
            )
            .allowed
        );
    }
}
