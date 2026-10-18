//! Policy decisions: pure functions from an agent's capabilities, the rules
//! resolved for it and an action to a decision (allow, deny, require an
//! approval, allow with an alert), with the rule that decided. They run
//! in-process at every enforcement point (the runtime's tool loop, the
//! backend's context builder and dispatch, the re-check of every reported
//! span), so they take microseconds and never do IO.

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Timelike, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::capabilities::{Capabilities, DataLabel};
use super::memory::MemoryScope;
use super::policy_document::{
    APPROVAL_TIMEOUT_DEFAULT_S, InjectionAction, PiiKind, RuleAction, Severity, TimeWindow,
};

/// Tools whose output leaves the run as an artifact: the ones a label-based
/// approval rule holds back.
pub const OUTBOUND_TOOLS: [&str; 2] = ["write_file", "make_docx"];

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

/// What a decision does with the action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Effect {
    Allow,
    Deny,
    /// The action waits for one of `approvers` (every workspace admin when empty).
    RequireApproval {
        approvers: Vec<Uuid>,
    },
    /// The action goes ahead and someone is told.
    Alert {
        severity: Severity,
    },
}

/// The outcome of a check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Decision {
    /// Whether the action may go ahead now (`allow` and `alert`).
    pub allowed: bool,
    pub effect: Effect,
    /// The rule that decided (`tools.allow`, `delegation.max_depth`, ...).
    pub rule: &'static str,
    /// Why, in words.
    pub reason: String,
}

impl Decision {
    /// The action goes ahead.
    pub fn allow(rule: &'static str, reason: impl Into<String>) -> Self {
        Decision {
            allowed: true,
            effect: Effect::Allow,
            rule,
            reason: reason.into(),
        }
    }

    /// The action is refused.
    pub fn deny(rule: &'static str, reason: impl Into<String>) -> Self {
        Decision {
            allowed: false,
            effect: Effect::Deny,
            rule,
            reason: reason.into(),
        }
    }

    /// The action waits for a human.
    pub fn require_approval(
        rule: &'static str,
        approvers: Vec<Uuid>,
        reason: impl Into<String>,
    ) -> Self {
        Decision {
            allowed: false,
            effect: Effect::RequireApproval { approvers },
            rule,
            reason: reason.into(),
        }
    }

    /// The action goes ahead and an alert is raised.
    pub fn alert(rule: &'static str, severity: Severity, reason: impl Into<String>) -> Self {
        Decision {
            allowed: true,
            effect: Effect::Alert { severity },
            rule,
            reason: reason.into(),
        }
    }

    /// `allow`, `deny`, `require_approval` or `alert`, as recorded on a span.
    pub fn verdict(&self) -> &'static str {
        match self.effect {
            Effect::Allow => "allow",
            Effect::Deny => "deny",
            Effect::RequireApproval { .. } => "require_approval",
            Effect::Alert { .. } => "alert",
        }
    }
}

/// Whether `host` is one of `allowed`: exact, or under a `*.` pattern.
pub fn host_matches(allowed: &[String], host: &str) -> bool {
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

/// Decides `action` under `caps` alone: allow or deny.
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
            Some(hosts) if host_matches(hosts, host) => {
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

// --- the resolved rules carried beside the capabilities -----------------------------------------

/// Which actions wait for a human, once resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct ApprovalRules {
    pub tools: Vec<String>,
    pub labels: Vec<DataLabel>,
    /// Members who may decide; empty: every workspace admin.
    pub approvers: Vec<Uuid>,
    /// Seconds a call waits before the node fails.
    pub timeout_s: u32,
}

impl Default for ApprovalRules {
    fn default() -> Self {
        ApprovalRules {
            tools: Vec::new(),
            labels: Vec::new(),
            approvers: Vec::new(),
            timeout_s: APPROVAL_TIMEOUT_DEFAULT_S,
        }
    }
}

/// Ceilings, once resolved (the lowest of every level).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct LimitRules {
    pub tokens_per_run: Option<i64>,
    pub tokens_per_day: Option<i64>,
    pub cost_usd_per_day: Option<f64>,
    pub tool_calls_per_minute: Option<u32>,
    pub runs_per_hour: Option<u32>,
}

/// Alert thresholds, once resolved (the lowest of every level).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct AlertRules {
    pub risk_score: Option<f64>,
    pub quarantine_risk_score: Option<f64>,
    pub denials_per_run: Option<u32>,
    pub novelty: Option<f64>,
}

/// Where content of one label may go, once resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct OutboundRules {
    pub artifacts: bool,
    /// `null`: any host the egress allow-list permits; `[]`: none.
    pub egress_hosts: Option<Vec<String>>,
}

impl Default for OutboundRules {
    fn default() -> Self {
        OutboundRules {
            artifacts: true,
            egress_hosts: None,
        }
    }
}

/// The PII rule, once resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct PiiRules {
    pub enabled: bool,
    pub kinds: Vec<PiiKind>,
    pub keywords: Vec<String>,
    pub action: RuleAction,
}

impl Default for PiiRules {
    fn default() -> Self {
        PiiRules {
            enabled: true,
            kinds: PiiKind::ALL.to_vec(),
            keywords: Vec::new(),
            action: RuleAction::Alert,
        }
    }
}

/// DLP on the way out, once resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct DlpRules {
    pub secrets: RuleAction,
    pub pii: PiiRules,
    pub labels: BTreeMap<DataLabel, OutboundRules>,
}

impl Default for DlpRules {
    /// Public and internal content goes anywhere; confidential content may
    /// be delivered as an artifact but not sent out; restricted content
    /// leaves the run in no form.
    fn default() -> Self {
        let mut labels = BTreeMap::new();
        labels.insert(DataLabel::Public, OutboundRules::default());
        labels.insert(DataLabel::Internal, OutboundRules::default());
        labels.insert(
            DataLabel::Confidential,
            OutboundRules {
                artifacts: true,
                egress_hosts: Some(Vec::new()),
            },
        );
        labels.insert(
            DataLabel::Restricted,
            OutboundRules {
                artifacts: false,
                egress_hosts: Some(Vec::new()),
            },
        );
        DlpRules {
            secrets: RuleAction::Deny,
            pii: PiiRules::default(),
            labels,
        }
    }
}

impl DlpRules {
    /// Where content of `label` may go.
    pub fn outbound(&self, label: DataLabel) -> OutboundRules {
        self.labels.get(&label).cloned().unwrap_or_default()
    }
}

/// The prompt-injection screen, once resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct InjectionRules {
    pub action: InjectionAction,
    pub model_check: bool,
}

impl Default for InjectionRules {
    fn default() -> Self {
        InjectionRules {
            action: InjectionAction::Flag,
            model_check: false,
        }
    }
}

/// The versions of the policies that governed a node, by level; `null`
/// where no policy is set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct PolicyVersions {
    #[schema(required = true)]
    pub workspace: Option<i32>,
    #[schema(required = true)]
    pub team: Option<i32>,
    #[schema(required = true)]
    pub agent: Option<i32>,
}

/// Everything the resolved policy says beyond the capabilities: carried in
/// the per-run token next to them and evaluated by the same functions on
/// both sides. `Rules::default()` is what an agent gets when no policy is set.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct Rules {
    /// Tools refused whatever the allow-list says.
    pub tools_denied: Vec<String>,
    pub egress_denied: Vec<String>,
    pub models_denied: Vec<String>,
    pub require_approval: ApprovalRules,
    pub limits: LimitRules,
    /// One set of windows per level that set some; a time must be in a window of every set.
    pub time_windows: Vec<Vec<TimeWindow>>,
    pub alerts: AlertRules,
    pub dlp: DlpRules,
    pub injection: InjectionRules,
    pub versions: PolicyVersions,
}

impl Rules {
    /// Whether work may start at `now` under the time windows.
    pub fn within_time_windows(&self, now: DateTime<Utc>) -> Decision {
        let weekday = now.weekday().number_from_monday() as u8;
        let minute = now.hour() * 60 + now.minute();
        let closed = self
            .time_windows
            .iter()
            .find(|set| !set.iter().any(|w| w.admits(weekday, minute)));
        match closed {
            None => Decision::allow("time_windows", "within the agent's working hours"),
            Some(_) => Decision::deny(
                "time_windows",
                format!(
                    "outside the agent's working hours ({} UTC)",
                    now.format("%a %H:%M")
                ),
            ),
        }
    }
}

/// Decides `action` under `caps` and `rules`, for a node that was given data
/// of `labels_given`: a deny-list refuses first, then the capabilities, then
/// an approval rule holds the action for a human.
pub fn decide_with(
    caps: &Capabilities,
    rules: &Rules,
    labels_given: &[DataLabel],
    action: &Action<'_>,
) -> Decision {
    match action {
        Action::Tool(name) => {
            if rules.tools_denied.iter().any(|t| t == name) {
                return Decision::deny("tools.deny", format!("tool {name} is denied by policy"));
            }
            let base = decide(caps, action);
            if !base.allowed {
                return base;
            }
            let approval = &rules.require_approval;
            if approval.tools.iter().any(|t| t == name) {
                return Decision::require_approval(
                    "approvals.tools",
                    approval.approvers.clone(),
                    format!("tool {name} needs an approval"),
                );
            }
            if OUTBOUND_TOOLS.contains(name)
                && let Some(label) = labels_given.iter().find(|l| approval.labels.contains(l))
            {
                return Decision::require_approval(
                    "approvals.labels",
                    approval.approvers.clone(),
                    format!("the node read {label} data; {name} needs an approval"),
                );
            }
            base
        }
        Action::Egress(host) => {
            if host_matches(&rules.egress_denied, host) {
                return Decision::deny("egress.deny", format!("host {host} is denied by policy"));
            }
            decide(caps, action)
        }
        Action::Model(model) => {
            if rules.models_denied.iter().any(|m| m == model) {
                return Decision::deny("models.deny", format!("model {model} is denied by policy"));
            }
            decide(caps, action)
        }
        Action::Spawn { .. } | Action::Read { .. } => decide(caps, action),
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
        assert_eq!(denied.effect, Effect::Deny);
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

    #[test]
    fn rules_deny_first_then_hold_for_approval() {
        let caps = Capabilities::default();
        let approver = Uuid::now_v7();
        let rules = Rules {
            tools_denied: vec!["run_python".into()],
            egress_denied: vec!["*.evil.example".into()],
            models_denied: vec!["gpt-x".into()],
            require_approval: ApprovalRules {
                tools: vec!["make_docx".into()],
                labels: vec![DataLabel::Confidential],
                approvers: vec![approver],
                timeout_s: 30,
            },
            ..Rules::default()
        };
        let denied = decide_with(&caps, &rules, &[], &Action::Tool("run_python"));
        assert_eq!((denied.rule, denied.verdict()), ("tools.deny", "deny"));
        let held = decide_with(&caps, &rules, &[], &Action::Tool("make_docx"));
        assert!(!held.allowed);
        assert_eq!(held.rule, "approvals.tools");
        assert_eq!(held.verdict(), "require_approval");
        assert_eq!(
            held.effect,
            Effect::RequireApproval {
                approvers: vec![approver]
            }
        );
        // A plain tool is allowed; an outbound tool after confidential data is held.
        assert!(decide_with(&caps, &rules, &[], &Action::Tool("write_file")).allowed);
        let labelled = decide_with(
            &caps,
            &rules,
            &[DataLabel::Internal, DataLabel::Confidential],
            &Action::Tool("write_file"),
        );
        assert_eq!(labelled.rule, "approvals.labels");
        assert!(
            decide_with(
                &caps,
                &rules,
                &[DataLabel::Confidential],
                &Action::Tool("read_file")
            )
            .allowed,
            "reading is not leaving"
        );
        assert_eq!(
            decide_with(&caps, &rules, &[], &Action::Egress("a.evil.example")).rule,
            "egress.deny"
        );
        assert!(decide_with(&caps, &rules, &[], &Action::Egress("api.example.com")).allowed);
        assert_eq!(
            decide_with(&caps, &rules, &[], &Action::Model("gpt-x")).rule,
            "models.deny"
        );
        // The capabilities still decide underneath the rules.
        let narrow = Capabilities {
            tools: some(&["finish"]),
            ..Capabilities::default()
        };
        assert_eq!(
            decide_with(&narrow, &rules, &[], &Action::Tool("make_docx")).rule,
            "tools.allow"
        );
        let alert = Decision::alert("dlp.pii", Severity::Medium, "2 e-mail addresses");
        assert!(alert.allowed && alert.verdict() == "alert");
    }

    #[test]
    fn time_windows_are_the_intersection_of_every_level() {
        let office = TimeWindow {
            days: vec![1, 2, 3, 4, 5],
            start: "09:00".into(),
            end: "17:00".into(),
        };
        let mornings = TimeWindow {
            days: vec![1, 2, 3, 4, 5, 6, 7],
            start: "06:00".into(),
            end: "12:00".into(),
        };
        let rules = Rules {
            time_windows: vec![vec![office], vec![mornings]],
            ..Rules::default()
        };
        let at = |text: &str| text.parse::<DateTime<Utc>>().unwrap();
        // Monday 10:00 is in both; Monday 14:00 only in the office; Sunday 10:00 only in mornings.
        assert!(
            rules
                .within_time_windows(at("2026-10-19T10:00:00Z"))
                .allowed
        );
        let afternoon = rules.within_time_windows(at("2026-10-19T14:00:00Z"));
        assert!(!afternoon.allowed && afternoon.rule == "time_windows");
        assert!(
            !rules
                .within_time_windows(at("2026-10-18T10:00:00Z"))
                .allowed
        );
        assert!(Rules::default().within_time_windows(Utc::now()).allowed);
        let never = Rules {
            time_windows: vec![Vec::new()],
            ..Rules::default()
        };
        assert!(!never.within_time_windows(Utc::now()).allowed);
    }

    #[test]
    fn the_default_rules_keep_restricted_content_in() {
        let rules = Rules::default();
        assert!(rules.dlp.outbound(DataLabel::Internal).artifacts);
        assert_eq!(rules.dlp.outbound(DataLabel::Internal).egress_hosts, None);
        assert!(rules.dlp.outbound(DataLabel::Confidential).artifacts);
        assert_eq!(
            rules.dlp.outbound(DataLabel::Confidential).egress_hosts,
            Some(Vec::new())
        );
        assert!(!rules.dlp.outbound(DataLabel::Restricted).artifacts);
        assert_eq!(rules.dlp.secrets, RuleAction::Deny);
        assert_eq!(rules.dlp.pii.action, RuleAction::Alert);
        assert_eq!(rules.injection.action, InjectionAction::Flag);
        assert_eq!(rules.require_approval.timeout_s, 600);
        // Old tokens without rules parse to the defaults.
        let parsed: Rules = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, rules);
    }
}
