//! The policy engine: resolves the policy documents set at the workspace,
//! team and agent levels into one effective policy (the capabilities the
//! per-run token carries and the rules beside them), remembering which
//! level set each rule. Resolution only narrows: a lower level removes
//! tools, hosts and labels, lowers limits and adds approvals, never the
//! reverse, and a document that tries to widen its parent is refused
//! before it is stored.

use std::collections::BTreeMap;

use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::domain::capabilities::{Capabilities, DataLabel, Delegation};
use crate::domain::policy::{PolicyVersions, Rules};
use crate::domain::policy_document::{
    EvidenceAction, InjectionAction, ListRule, Policy, PolicyDocument, PolicyScope, RuleAction,
};
use crate::domain::validation::FieldErrors;

/// One level of the hierarchy, as stored.
#[derive(Debug, Clone)]
pub struct Level {
    pub scope: PolicyScope,
    pub scope_id: Option<Uuid>,
    pub version: i32,
    pub document: PolicyDocument,
}

/// A level of the hierarchy, as shown in the resolve view.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LevelRef {
    pub scope: PolicyScope,
    #[schema(required = true)]
    pub scope_id: Option<Uuid>,
    pub version: i32,
}

/// The effective policy of an agent and where each rule came from.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Resolved {
    /// What the per-run token carries: the agent's capabilities narrowed by every level.
    pub capabilities: Capabilities,
    pub rules: Rules,
    /// Rule path (`tools.allow`, `limits.tokens_per_run`, ...) → the level that set it
    /// (`workspace`, `team`, `agent`, `capabilities` for the agent's own document, `default`).
    pub origins: BTreeMap<String, String>,
    /// The policy versions in force, outermost first.
    pub levels: Vec<LevelRef>,
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

fn extend_unique(into: &mut Vec<String>, more: &[String]) {
    for item in more {
        if !into.contains(item) {
            into.push(item.clone());
        }
    }
}

fn lower<T: PartialOrd + Copy>(current: &mut Option<T>, new: Option<T>) -> bool {
    match (current.as_mut(), new) {
        (_, None) => false,
        (None, Some(n)) => {
            *current = Some(n);
            true
        }
        (Some(c), Some(n)) => {
            if n < *c {
                *c = n;
            }
            true
        }
    }
}

struct Folder {
    caps: Capabilities,
    rules: Rules,
    origins: BTreeMap<String, String>,
    pii_kinds_set: bool,
    pii_forced_on: bool,
}

impl Folder {
    fn set(&mut self, path: &str, level: &str) {
        self.origins.insert(path.to_owned(), level.to_owned());
    }

    fn list_rule(
        &mut self,
        level: &str,
        path: &str,
        rule: &ListRule,
        allow: fn(&mut Capabilities) -> &mut Option<Vec<String>>,
        deny: fn(&mut Rules) -> &mut Vec<String>,
    ) {
        if rule.allow.is_some() {
            let current = allow(&mut self.caps);
            *current = meet(current, &rule.allow);
            self.set(&format!("{path}.allow"), level);
        }
        if !rule.deny.is_empty() {
            extend_unique(deny(&mut self.rules), &rule.deny);
            self.set(&format!("{path}.deny"), level);
        }
    }

    fn fold(&mut self, level: &str, doc: &PolicyDocument) {
        self.list_rule(
            level,
            "tools",
            &doc.tools,
            |c| &mut c.tools,
            |r| &mut r.tools_denied,
        );
        self.list_rule(
            level,
            "egress",
            &doc.egress,
            |c| &mut c.egress_hosts,
            |r| &mut r.egress_denied,
        );
        self.list_rule(
            level,
            "models",
            &doc.models,
            |c| &mut c.models,
            |r| &mut r.models_denied,
        );
        if let Some(labels) = &doc.labels {
            self.caps.data.labels.retain(|l| labels.contains(l));
            self.set("labels", level);
        }
        let limits = &doc.limits;
        if let Some(depth) = limits.max_depth {
            self.caps.delegation.max_depth = self.caps.delegation.max_depth.min(depth);
            self.set("limits.max_depth", level);
        }
        if let Some(fan_out) = limits.max_fan_out {
            self.caps.delegation.max_fan_out = self.caps.delegation.max_fan_out.min(fan_out);
            self.set("limits.max_fan_out", level);
        }
        let mine = &mut self.rules.limits;
        for (path, changed) in [
            (
                "limits.tokens_per_run",
                lower(&mut mine.tokens_per_run, limits.tokens_per_run),
            ),
            (
                "limits.tokens_per_day",
                lower(&mut mine.tokens_per_day, limits.tokens_per_day),
            ),
            (
                "limits.cost_usd_per_day",
                lower(&mut mine.cost_usd_per_day, limits.cost_usd_per_day),
            ),
            (
                "limits.tool_calls_per_minute",
                lower(
                    &mut mine.tool_calls_per_minute,
                    limits.tool_calls_per_minute,
                ),
            ),
            (
                "limits.runs_per_hour",
                lower(&mut mine.runs_per_hour, limits.runs_per_hour),
            ),
        ] {
            if changed {
                self.set(path, level);
            }
        }
        if let Some(windows) = &doc.time_windows {
            self.rules.time_windows.push(windows.clone());
            self.set("time_windows", level);
        }
        let approval = &doc.require_approval;
        if !approval.tools.is_empty() {
            extend_unique(&mut self.rules.require_approval.tools, &approval.tools);
            self.set("require_approval.tools", level);
        }
        if !approval.labels.is_empty() {
            for label in &approval.labels {
                if !self.rules.require_approval.labels.contains(label) {
                    self.rules.require_approval.labels.push(*label);
                }
            }
            self.set("require_approval.labels", level);
        }
        if !approval.approvers.is_empty() {
            for approver in &approval.approvers {
                if !self.rules.require_approval.approvers.contains(approver) {
                    self.rules.require_approval.approvers.push(*approver);
                }
            }
            self.set("require_approval.approvers", level);
        }
        if let Some(timeout) = approval.timeout_s {
            let current = &mut self.rules.require_approval.timeout_s;
            if self.origins.contains_key("require_approval.timeout_s") {
                *current = (*current).min(timeout);
            } else {
                *current = timeout;
            }
            self.set("require_approval.timeout_s", level);
        }
        let alerts = &doc.alerts;
        let mine = &mut self.rules.alerts;
        for (path, changed) in [
            (
                "alerts.risk_score",
                lower(&mut mine.risk_score, alerts.risk_score),
            ),
            (
                "alerts.quarantine_risk_score",
                lower(
                    &mut mine.quarantine_risk_score,
                    alerts.quarantine_risk_score,
                ),
            ),
            (
                "alerts.denials_per_run",
                lower(&mut mine.denials_per_run, alerts.denials_per_run),
            ),
            ("alerts.novelty", lower(&mut mine.novelty, alerts.novelty)),
        ] {
            if changed {
                self.set(path, level);
            }
        }
        let dlp = &doc.dlp;
        if let Some(action) = dlp.secrets {
            self.rules.dlp.secrets = if self.origins.contains_key("dlp.secrets") {
                self.rules.dlp.secrets.stricter(action)
            } else {
                action
            };
            self.set("dlp.secrets", level);
        }
        if let Some(enabled) = dlp.pii.enabled {
            // A level above that turned the rule on keeps it on.
            if enabled || !self.pii_forced_on {
                self.rules.dlp.pii.enabled = enabled;
            }
            self.pii_forced_on |= enabled;
            self.set("dlp.pii.enabled", level);
        }
        if let Some(kinds) = &dlp.pii.kinds {
            if self.pii_kinds_set {
                for kind in kinds {
                    if !self.rules.dlp.pii.kinds.contains(kind) {
                        self.rules.dlp.pii.kinds.push(*kind);
                    }
                }
            } else {
                self.rules.dlp.pii.kinds = kinds.clone();
                self.pii_kinds_set = true;
            }
            self.set("dlp.pii.kinds", level);
        }
        if !dlp.pii.keywords.is_empty() {
            extend_unique(&mut self.rules.dlp.pii.keywords, &dlp.pii.keywords);
            self.set("dlp.pii.keywords", level);
        }
        if let Some(action) = dlp.pii.action {
            self.rules.dlp.pii.action = if self.origins.contains_key("dlp.pii.action") {
                self.rules.dlp.pii.action.stricter(action)
            } else {
                action
            };
            self.set("dlp.pii.action", level);
        }
        // The defaults are a starting point: the first level that speaks about a label
        // replaces them, every level below it only tightens.
        for (label, outbound) in &dlp.labels {
            let mut current = self.rules.dlp.outbound(*label);
            if let Some(artifacts) = outbound.artifacts {
                let path = format!("dlp.labels.{label}.artifacts");
                current.artifacts = if self.origins.contains_key(&path) {
                    current.artifacts && artifacts
                } else {
                    artifacts
                };
                self.set(&path, level);
            }
            if outbound.egress_hosts.is_some() {
                let path = format!("dlp.labels.{label}.egress_hosts");
                current.egress_hosts = if self.origins.contains_key(&path) {
                    meet(&current.egress_hosts, &outbound.egress_hosts)
                } else {
                    outbound.egress_hosts.clone()
                };
                self.set(&path, level);
            }
            self.rules.dlp.labels.insert(*label, current);
        }
        if let Some(action) = doc.injection.action {
            if action == InjectionAction::Block {
                self.rules.injection.action = InjectionAction::Block;
            } else if !self.origins.contains_key("injection.action") {
                self.rules.injection.action = action;
            }
            self.set("injection.action", level);
        }
        if let Some(check) = doc.injection.model_check {
            self.rules.injection.model_check |= check;
            self.set("injection.model_check", level);
        }
        // Evidence: once a level requires it, the levels below keep it; the coverage
        // only rises and reject is never undone.
        let evidence = &doc.evidence;
        if let Some(required) = evidence.required {
            self.rules.evidence.required |= required;
            self.set("evidence.required", level);
        }
        if let Some(min) = evidence.min_coverage {
            let current = &mut self.rules.evidence.min_coverage;
            *current = if self.origins.contains_key("evidence.min_coverage") {
                current.max(min)
            } else {
                min
            };
            self.set("evidence.min_coverage", level);
        }
        if let Some(action) = evidence.action {
            self.rules.evidence.action = if self.origins.contains_key("evidence.action") {
                self.rules.evidence.action.stricter(action)
            } else {
                action
            };
            self.set("evidence.action", level);
        }
    }
}

/// Resolves `levels` (outermost first: workspace, team, agent) over the
/// agent's own capabilities document (`None` for a node without an agent,
/// which reads as the system does).
pub fn resolve(levels: &[Level], agent: Option<&Capabilities>) -> Resolved {
    let mut folder = Folder {
        caps: agent.cloned().unwrap_or_default(),
        rules: Rules::default(),
        origins: BTreeMap::new(),
        pii_kinds_set: false,
        pii_forced_on: false,
    };
    if let Some(caps) = agent {
        let default = Capabilities::default();
        for (path, set) in [
            ("tools.allow", caps.tools.is_some()),
            ("egress.allow", caps.egress_hosts.is_some()),
            ("models.allow", caps.models.is_some()),
            ("labels", caps.data.labels != default.data.labels),
            (
                "limits.max_depth",
                caps.delegation.max_depth != default.delegation.max_depth,
            ),
            (
                "limits.max_fan_out",
                caps.delegation.max_fan_out != default.delegation.max_fan_out,
            ),
        ] {
            if set {
                folder.set(path, "capabilities");
            }
        }
    }
    let mut refs = Vec::with_capacity(levels.len());
    for level in levels {
        folder.fold(level.scope.as_str(), &level.document);
        match level.scope {
            PolicyScope::Workspace => folder.rules.versions.workspace = Some(level.version),
            PolicyScope::Team => folder.rules.versions.team = Some(level.version),
            PolicyScope::Agent => folder.rules.versions.agent = Some(level.version),
        }
        refs.push(LevelRef {
            scope: level.scope,
            scope_id: level.scope_id,
            version: level.version,
        });
    }
    Resolved {
        capabilities: folder.caps,
        rules: folder.rules,
        origins: folder.origins,
        levels: refs,
    }
}

/// The levels that apply to an agent (or to a team, or to the workspace
/// alone), outermost first, from the latest version at each.
pub fn levels_for(latest: &[Policy], team_id: Option<Uuid>, agent_id: Option<Uuid>) -> Vec<Level> {
    let pick = |scope: PolicyScope, scope_id: Option<Uuid>| {
        latest
            .iter()
            .find(|p| p.scope == scope && p.scope_id == scope_id)
            .map(|p| Level {
                scope: p.scope,
                scope_id: p.scope_id,
                version: p.version,
                document: p.document.clone(),
            })
    };
    let mut levels = Vec::with_capacity(3);
    levels.extend(pick(PolicyScope::Workspace, None));
    if let Some(team) = team_id {
        levels.extend(pick(PolicyScope::Team, Some(team)));
    }
    if let Some(agent) = agent_id {
        levels.extend(pick(PolicyScope::Agent, Some(agent)));
    }
    levels
}

/// The versions a resolution recorded.
pub fn versions_of(levels: &[Level]) -> PolicyVersions {
    let mut versions = PolicyVersions::default();
    for level in levels {
        match level.scope {
            PolicyScope::Workspace => versions.workspace = Some(level.version),
            PolicyScope::Team => versions.team = Some(level.version),
            PolicyScope::Agent => versions.agent = Some(level.version),
        }
    }
    versions
}

fn subset<T: PartialEq>(child: &[T], parent: &[T]) -> bool {
    child.iter().all(|c| parent.contains(c))
}

/// Adds to `errors` every way `child` would widen the policy the levels
/// above it resolve to (`parent`). A child may remove and lower, add
/// approvals, alerts and deny-lists; it may not allow a tool, host, label or
/// model its parent does not, raise a ceiling, or weaken a DLP or injection action.
pub fn check_narrowing(parent: &Resolved, child: &PolicyDocument, errors: &mut FieldErrors) {
    let widen = |errors: &mut FieldErrors, field: &str, what: &str| {
        errors.add(
            &format!("document.{field}"),
            format!("would widen the policy of the level above: {what}"),
        );
    };
    let lists = [
        (
            "tools.allow",
            &child.tools.allow,
            &parent.capabilities.tools,
        ),
        (
            "egress.allow",
            &child.egress.allow,
            &parent.capabilities.egress_hosts,
        ),
        (
            "models.allow",
            &child.models.allow,
            &parent.capabilities.models,
        ),
    ];
    for (field, mine, theirs) in lists {
        if let (Some(mine), Some(theirs)) = (mine, theirs) {
            let extra: Vec<&String> = mine.iter().filter(|m| !theirs.contains(m)).collect();
            if !extra.is_empty() {
                widen(errors, field, &format!("{} is not allowed above", extra[0]));
            }
        }
    }
    if let Some(labels) = &child.labels
        && !subset(labels, &parent.capabilities.labels())
    {
        widen(errors, "labels", "a label the level above does not clear");
    }
    let limits = &child.limits;
    let theirs = &parent.rules.limits;
    let delegation: Delegation = parent.capabilities.delegation;
    if limits.max_depth.is_some_and(|d| d > delegation.max_depth) {
        widen(errors, "limits.max_depth", "deeper delegation");
    }
    if limits
        .max_fan_out
        .is_some_and(|f| f > delegation.max_fan_out)
    {
        widen(errors, "limits.max_fan_out", "more sub-agents");
    }
    fn higher<T: PartialOrd>(mine: Option<T>, theirs: Option<T>) -> bool {
        matches!((mine, theirs), (Some(m), Some(t)) if m > t)
    }
    for (field, over) in [
        (
            "limits.tokens_per_run",
            higher(limits.tokens_per_run, theirs.tokens_per_run),
        ),
        (
            "limits.tokens_per_day",
            higher(limits.tokens_per_day, theirs.tokens_per_day),
        ),
        (
            "limits.cost_usd_per_day",
            higher(limits.cost_usd_per_day, theirs.cost_usd_per_day),
        ),
        (
            "limits.tool_calls_per_minute",
            higher(limits.tool_calls_per_minute, theirs.tool_calls_per_minute),
        ),
        (
            "limits.runs_per_hour",
            higher(limits.runs_per_hour, theirs.runs_per_hour),
        ),
        (
            "alerts.risk_score",
            higher(child.alerts.risk_score, parent.rules.alerts.risk_score),
        ),
        (
            "alerts.quarantine_risk_score",
            higher(
                child.alerts.quarantine_risk_score,
                parent.rules.alerts.quarantine_risk_score,
            ),
        ),
        (
            "alerts.denials_per_run",
            higher(
                child.alerts.denials_per_run,
                parent.rules.alerts.denials_per_run,
            ),
        ),
        (
            "alerts.novelty",
            higher(child.alerts.novelty, parent.rules.alerts.novelty),
        ),
    ] {
        if over {
            widen(errors, field, "a higher ceiling");
        }
    }
    if child.require_approval.timeout_s.is_some_and(|t| {
        parent.origins.contains_key("require_approval.timeout_s")
            && t > parent.rules.require_approval.timeout_s
    }) {
        widen(errors, "require_approval.timeout_s", "a longer wait");
    }
    let spoke = |path: &str| parent.origins.contains_key(path);
    if child.dlp.secrets == Some(RuleAction::Alert)
        && spoke("dlp.secrets")
        && parent.rules.dlp.secrets == RuleAction::Deny
    {
        widen(errors, "dlp.secrets", "a weaker action than deny");
    }
    if child.dlp.pii.action == Some(RuleAction::Alert)
        && spoke("dlp.pii.action")
        && parent.rules.dlp.pii.action == RuleAction::Deny
    {
        widen(errors, "dlp.pii.action", "a weaker action than deny");
    }
    if child.dlp.pii.enabled == Some(false)
        && parent.rules.dlp.pii.enabled
        && parent.origins.contains_key("dlp.pii.enabled")
    {
        widen(
            errors,
            "dlp.pii.enabled",
            "turning off a check the level above requires",
        );
    }
    for (label, outbound) in &child.dlp.labels {
        let theirs = parent.rules.dlp.outbound(*label);
        let (artifacts, hosts) = (
            format!("dlp.labels.{label}.artifacts"),
            format!("dlp.labels.{label}.egress_hosts"),
        );
        if outbound.artifacts == Some(true) && spoke(&artifacts) && !theirs.artifacts {
            widen(errors, &artifacts, "artifacts the level above keeps in");
        }
        if let (Some(mine), Some(theirs)) = (&outbound.egress_hosts, &theirs.egress_hosts)
            && spoke(&hosts)
            && !subset(mine, theirs)
        {
            widen(errors, &hosts, "a host the level above does not allow");
        }
    }
    if child.injection.action == Some(InjectionAction::Flag)
        && spoke("injection.action")
        && parent.rules.injection.action == InjectionAction::Block
    {
        widen(
            errors,
            "injection.action",
            "flagging where the level above blocks",
        );
    }
    if child.injection.model_check == Some(false) && parent.rules.injection.model_check {
        widen(
            errors,
            "injection.model_check",
            "dropping a check the level above requires",
        );
    }
    let evidence = &child.evidence;
    if evidence.required == Some(false)
        && spoke("evidence.required")
        && parent.rules.evidence.required
    {
        widen(
            errors,
            "evidence.required",
            "dropping the evidence the level above requires",
        );
    }
    if let Some(min) = evidence.min_coverage
        && spoke("evidence.min_coverage")
        && min < parent.rules.evidence.min_coverage
    {
        widen(
            errors,
            "evidence.min_coverage",
            "less coverage than the level above requires",
        );
    }
    if evidence.action == Some(EvidenceAction::Mark)
        && spoke("evidence.action")
        && parent.rules.evidence.action == EvidenceAction::Reject
    {
        widen(errors, "evidence.action", "a weaker action than reject");
    }
}

impl Capabilities {
    /// The clearance, for narrowing checks.
    fn labels(&self) -> Vec<DataLabel> {
        self.data.labels.clone()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn level(scope: PolicyScope, version: i32, doc: serde_json::Value) -> Level {
        Level {
            scope,
            scope_id: (scope != PolicyScope::Workspace).then(Uuid::now_v7),
            version,
            document: serde_json::from_value(doc).unwrap(),
        }
    }

    #[test]
    fn nothing_set_resolves_to_the_capabilities_and_the_default_rules() {
        let caps = Capabilities {
            tools: Some(vec!["finish".into()]),
            ..Capabilities::default()
        };
        let resolved = resolve(&[], Some(&caps));
        assert_eq!(resolved.capabilities, caps);
        assert_eq!(resolved.rules, Rules::default());
        assert_eq!(resolved.origins.get("tools.allow").unwrap(), "capabilities");
        assert!(resolved.levels.is_empty());
        let system = resolve(&[], None);
        assert_eq!(system.capabilities, Capabilities::default());
        assert!(system.origins.is_empty());
    }

    #[test]
    fn levels_only_narrow_and_remember_who_set_what() {
        let workspace = level(
            PolicyScope::Workspace,
            3,
            json!({
                "tools": {"allow": ["write_file", "read_file", "make_docx", "finish"], "deny": ["run_python"]},
                "labels": ["public", "internal", "confidential"],
                "limits": {"tokens_per_run": 50000, "max_depth": 2},
                "require_approval": {"tools": ["make_docx"], "timeout_s": 300},
                "alerts": {"risk_score": 6.0},
                "dlp": {"secrets": "alert", "labels": {"confidential": {"artifacts": true, "egress_hosts": ["api.example.com", "cdn.example.com"]}}},
                "injection": {"action": "flag"},
            }),
        );
        let team = level(
            PolicyScope::Team,
            1,
            json!({
                "tools": {"allow": ["write_file", "make_docx", "finish", "spawn_subagent"]},
                "limits": {"tokens_per_run": 80000, "tokens_per_day": 1000000, "max_depth": 1},
                "require_approval": {"labels": ["confidential"], "timeout_s": 900},
                "alerts": {"risk_score": 4.0, "quarantine_risk_score": 8.0},
                "dlp": {"secrets": "deny", "pii": {"keywords": ["Project Zeus"]},
                        "labels": {"confidential": {"egress_hosts": ["cdn.example.com", "evil.example"]}}},
                "injection": {"action": "block"},
                "time_windows": [{"days": [1,2,3,4,5], "start": "08:00", "end": "18:00"}],
            }),
        );
        let agent = level(
            PolicyScope::Agent,
            2,
            json!({
                "tools": {"deny": ["spawn_subagent"]},
                "labels": ["public", "internal", "confidential", "restricted"],
                "limits": {"tokens_per_run": 20000},
                "dlp": {"labels": {"confidential": {"artifacts": false}}},
                "injection": {"action": "flag"},
            }),
        );
        let caps = Capabilities::default();
        let resolved = resolve(&[workspace, team, agent], Some(&caps));
        let c = &resolved.capabilities;
        // The meet of every allow-list, in the outermost order.
        assert_eq!(
            c.tools,
            Some(vec![
                "write_file".into(),
                "make_docx".into(),
                "finish".into()
            ])
        );
        // The agent asked for restricted; it only ever gets what every level above allows.
        assert_eq!(c.data.labels, vec![DataLabel::Public, DataLabel::Internal]);
        assert_eq!(c.delegation.max_depth, 1);
        let r = &resolved.rules;
        assert_eq!(r.tools_denied, vec!["run_python", "spawn_subagent"]);
        assert_eq!(r.limits.tokens_per_run, Some(20000));
        assert_eq!(r.limits.tokens_per_day, Some(1_000_000));
        assert_eq!(r.require_approval.tools, vec!["make_docx"]);
        assert_eq!(r.require_approval.labels, vec![DataLabel::Confidential]);
        assert_eq!(r.require_approval.timeout_s, 300, "the shorter wait wins");
        assert_eq!(r.alerts.risk_score, Some(4.0));
        assert_eq!(r.alerts.quarantine_risk_score, Some(8.0));
        assert_eq!(
            r.dlp.secrets,
            RuleAction::Deny,
            "deny is stricter than alert"
        );
        assert_eq!(r.dlp.pii.keywords, vec!["Project Zeus"]);
        let confidential = r.dlp.outbound(DataLabel::Confidential);
        assert!(!confidential.artifacts);
        assert_eq!(
            confidential.egress_hosts,
            Some(vec!["cdn.example.com".into()]),
            "the workspace opened two hosts, the team kept one, evil.example never got in"
        );
        // A workspace may open what the defaults keep in; the levels below only tighten it.
        let opened = resolve(
            &[level(
                PolicyScope::Workspace,
                1,
                json!({"dlp": {"secrets": "alert", "labels": {"restricted": {"artifacts": true}}}}),
            )],
            None,
        );
        assert!(opened.rules.dlp.outbound(DataLabel::Restricted).artifacts);
        assert_eq!(opened.rules.dlp.secrets, RuleAction::Alert);
        let mut errors = FieldErrors::default();
        check_narrowing(
            &resolve(&[], None),
            &serde_json::from_value(json!({"dlp": {"secrets": "alert"}})).unwrap(),
            &mut errors,
        );
        assert!(errors.is_empty(), "the defaults are not a floor");
        assert_eq!(
            r.injection.action,
            InjectionAction::Block,
            "block cannot be undone below"
        );
        assert_eq!(r.time_windows.len(), 1);
        assert_eq!(
            (r.versions.workspace, r.versions.team, r.versions.agent),
            (Some(3), Some(1), Some(2))
        );
        let o = &resolved.origins;
        assert_eq!(o["tools.allow"], "team");
        assert_eq!(o["tools.deny"], "agent");
        assert_eq!(o["labels"], "agent");
        assert_eq!(o["limits.tokens_per_run"], "agent");
        assert_eq!(o["limits.tokens_per_day"], "team");
        assert_eq!(o["require_approval.tools"], "workspace");
        assert_eq!(o["injection.action"], "agent");
        assert_eq!(o["dlp.labels.confidential.artifacts"], "agent");
        assert_eq!(resolved.levels.len(), 3);
        assert_eq!(resolved.levels[0].scope, PolicyScope::Workspace);
        assert!(!o.contains_key("alerts.novelty"));
        let versions = versions_of(
            &resolved
                .levels
                .iter()
                .map(|l| Level {
                    scope: l.scope,
                    scope_id: l.scope_id,
                    version: l.version,
                    document: PolicyDocument::default(),
                })
                .collect::<Vec<_>>(),
        );
        assert_eq!(versions.team, Some(1));
    }

    #[test]
    fn evidence_rules_only_tighten_from_level_to_level() {
        let workspace = level(
            PolicyScope::Workspace,
            1,
            json!({"evidence": {"required": true, "min_coverage": 0.9, "action": "reject"}}),
        );
        let agent = level(
            PolicyScope::Agent,
            1,
            json!({"evidence": {"required": false, "min_coverage": 0.5, "action": "mark"}}),
        );
        let resolved = resolve(&[workspace.clone(), agent.clone()], None);
        let e = &resolved.rules.evidence;
        assert!(e.required, "required stays required");
        assert_eq!(e.min_coverage, 0.9, "the coverage only rises");
        assert_eq!(e.action, EvidenceAction::Reject, "reject is never undone");
        assert_eq!(resolved.origins["evidence.required"], "agent");
        // Alone, the agent's document is what it says.
        let alone = resolve(std::slice::from_ref(&agent), None);
        assert!(!alone.rules.evidence.required);
        assert_eq!(alone.rules.evidence.min_coverage, 0.5);
        assert_eq!(alone.rules.evidence.action, EvidenceAction::Mark);
        // Nothing set: off, 0.8, mark, with no origin.
        let none = resolve(&[], None);
        assert_eq!(
            none.rules.evidence,
            crate::domain::policy::EvidenceRules::default()
        );
        assert!(!none.origins.contains_key("evidence.required"));
        // Widening is named rule by rule.
        let parent = resolve(&[workspace], None);
        let mut errors = FieldErrors::default();
        check_narrowing(&parent, &agent.document, &mut errors);
        let fields: Vec<&String> = errors.as_map().keys().collect();
        assert_eq!(
            fields,
            [
                "document.evidence.action",
                "document.evidence.min_coverage",
                "document.evidence.required",
            ]
        );
        let tighter: PolicyDocument = serde_json::from_value(
            json!({"evidence": {"required": true, "min_coverage": 1.0, "action": "reject"}}),
        )
        .unwrap();
        let mut errors = FieldErrors::default();
        check_narrowing(&parent, &tighter, &mut errors);
        assert!(errors.is_empty(), "{:?}", errors.as_map());
    }

    #[test]
    fn a_child_that_widens_its_parent_is_named_rule_by_rule() {
        let parent = resolve(
            &[level(
                PolicyScope::Workspace,
                1,
                json!({
                    "tools": {"allow": ["write_file", "finish"]},
                    "labels": ["public", "internal"],
                    "limits": {"tokens_per_run": 50000, "max_depth": 1, "runs_per_hour": 10},
                    "require_approval": {"timeout_s": 300},
                    "alerts": {"risk_score": 5.0},
                    "dlp": {"secrets": "deny", "pii": {"enabled": true, "action": "deny"},
                            "labels": {"restricted": {"artifacts": false, "egress_hosts": []}}},
                    "injection": {"action": "block", "model_check": true},
                }),
            )],
            None,
        );
        let child: PolicyDocument = serde_json::from_value(json!({
            "tools": {"allow": ["write_file", "make_docx"]},
            "labels": ["public", "internal", "restricted"],
            "limits": {"tokens_per_run": 60000, "max_depth": 2, "runs_per_hour": 11},
            "require_approval": {"timeout_s": 600},
            "alerts": {"risk_score": 7.0},
            "dlp": {"secrets": "alert", "pii": {"enabled": false, "action": "alert"},
                    "labels": {"restricted": {"artifacts": true, "egress_hosts": ["x.example"]}}},
            "injection": {"action": "flag", "model_check": false},
        }))
        .unwrap();
        let mut errors = FieldErrors::default();
        check_narrowing(&parent, &child, &mut errors);
        let fields: Vec<&String> = errors.as_map().keys().collect();
        assert_eq!(
            fields,
            [
                "document.alerts.risk_score",
                "document.dlp.labels.restricted.artifacts",
                "document.dlp.labels.restricted.egress_hosts",
                "document.dlp.pii.action",
                "document.dlp.pii.enabled",
                "document.dlp.secrets",
                "document.injection.action",
                "document.injection.model_check",
                "document.labels",
                "document.limits.max_depth",
                "document.limits.runs_per_hour",
                "document.limits.tokens_per_run",
                "document.require_approval.timeout_s",
                "document.tools.allow",
            ]
        );
        // Narrowing is welcome.
        let narrower: PolicyDocument = serde_json::from_value(json!({
            "tools": {"allow": ["finish"], "deny": ["write_file"]},
            "labels": ["public"],
            "limits": {"tokens_per_run": 1000},
            "require_approval": {"tools": ["finish"], "timeout_s": 60},
            "alerts": {"risk_score": 2.0, "novelty": 0.5},
        }))
        .unwrap();
        let mut errors = FieldErrors::default();
        check_narrowing(&parent, &narrower, &mut errors);
        assert!(errors.is_empty(), "{:?}", errors.as_map());
        // And the meet never widens even if a wider child were stored.
        let resolved = resolve(
            &[
                level(
                    PolicyScope::Workspace,
                    1,
                    json!({"tools": {"allow": ["finish"]}}),
                ),
                level(
                    PolicyScope::Agent,
                    1,
                    json!({"tools": {"allow": ["finish", "make_docx"]}}),
                ),
            ],
            None,
        );
        assert_eq!(resolved.capabilities.tools, Some(vec!["finish".into()]));
    }
}
