//! The policy document: rules as data, kept as versions per workspace, team
//! and agent (`policies`). A document only says what it sets; everything it
//! leaves out is inherited from the level above, and resolution
//! (`engine::policy`) may only narrow: a child removes tools, hosts and
//! labels, lowers limits, adds approvals and alerts, never the reverse.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::capabilities::{DEPTH_MAX, DataLabel, ENTRY_MAX, FAN_OUT_MAX, LIST_MAX};
use super::string_enum;
use super::validation::FieldErrors;

/// Most time windows in one document.
pub const WINDOWS_MAX: usize = 16;
/// Most keywords the PII rule may name.
pub const KEYWORDS_MAX: usize = 64;
/// Longest keyword.
pub const KEYWORD_MAX: usize = 100;
/// Most approvers one rule may name.
pub const APPROVERS_MAX: usize = 32;
/// Longest wait for an approval a policy may ask for (24 hours).
pub const APPROVAL_TIMEOUT_MAX_S: u32 = 24 * 3600;
/// How long a tool call waits for its approval when no policy says.
pub const APPROVAL_TIMEOUT_DEFAULT_S: u32 = 600;

string_enum!(
    /// The level a policy is set at.
    PolicyScope {
        Workspace => "workspace",
        Team => "team",
        Agent => "agent",
    }
);

string_enum!(
    /// What a finding leads to: a word in the inbox, or a refusal.
    RuleAction {
        Alert => "alert",
        Deny => "deny",
    }
);

impl RuleAction {
    /// The stricter of two actions.
    pub fn stricter(self, other: RuleAction) -> RuleAction {
        if self == RuleAction::Deny || other == RuleAction::Deny {
            RuleAction::Deny
        } else {
            RuleAction::Alert
        }
    }
}

string_enum!(
    /// How bad an alert is.
    Severity {
        Low => "low",
        Medium => "medium",
        High => "high",
        Critical => "critical",
    }
);

string_enum!(
    /// A shape of personal data the DLP classifier looks for.
    PiiKind {
        Email => "email",
        Phone => "phone",
        Card => "card",
        NationalId => "national_id",
        /// A Japanese corporate number (houjin bangou): public, but it names
        /// the business a document is about; looked for only when asked.
        CorporateNumber => "corporate_number",
    }
);

impl PiiKind {
    /// Every kind.
    pub const ALL: [PiiKind; 5] = [
        PiiKind::Email,
        PiiKind::Phone,
        PiiKind::Card,
        PiiKind::NationalId,
        PiiKind::CorporateNumber,
    ];

    /// The kinds looked for when a policy names none: personal data.
    /// (Corporate numbers are published, and on every Japanese invoice.)
    pub const DEFAULT: [PiiKind; 4] = [
        PiiKind::Email,
        PiiKind::Phone,
        PiiKind::Card,
        PiiKind::NationalId,
    ];
}

string_enum!(
    /// What the prompt-injection screen does with a finding.
    InjectionAction {
        /// Record it as a policy span and let the text through.
        Flag => "flag",
        /// Record it and keep the text from the model.
        Block => "block",
    }
);

string_enum!(
    /// What the evidence verifier does with an output that is not verified.
    EvidenceAction {
        /// Record the verdict on the node run and the trace; the node succeeds.
        Mark => "mark",
        /// Fail the node, with the verdict as the reason.
        Reject => "reject",
    }
);

impl EvidenceAction {
    /// The stricter of two actions.
    pub fn stricter(self, other: EvidenceAction) -> EvidenceAction {
        if self == EvidenceAction::Reject || other == EvidenceAction::Reject {
            EvidenceAction::Reject
        } else {
            EvidenceAction::Mark
        }
    }
}

/// An allow-list and a deny-list of names (tools, hosts, models). `allow:
/// null` inherits the level above; `deny` is always added to it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ListRule {
    #[serde(default)]
    #[schema(required = true)]
    pub allow: Option<Vec<String>>,
    #[serde(default)]
    #[schema(required = true)]
    pub deny: Vec<String>,
}

/// Ceilings. `null` inherits; a child may only lower one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// Tokens (in + out) one run of the agent may spend.
    #[serde(default)]
    #[schema(required = true)]
    pub tokens_per_run: Option<i64>,
    /// Tokens the agent may spend per UTC day, across runs.
    #[serde(default)]
    #[schema(required = true)]
    pub tokens_per_day: Option<i64>,
    /// Dollars the agent may spend per UTC day.
    #[serde(default)]
    #[schema(required = true)]
    pub cost_usd_per_day: Option<f64>,
    /// Tool calls per minute within one run (the runtime refuses the rest).
    #[serde(default)]
    #[schema(required = true)]
    pub tool_calls_per_minute: Option<u32>,
    /// Nodes the agent may start per hour, across runs.
    #[serde(default)]
    #[schema(required = true)]
    pub runs_per_hour: Option<u32>,
    #[serde(default)]
    #[schema(required = true)]
    pub max_depth: Option<u8>,
    #[serde(default)]
    #[schema(required = true)]
    pub max_fan_out: Option<u8>,
}

/// When the agent may start work (UTC). `days` are ISO weekdays, 1 = Monday.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TimeWindow {
    pub days: Vec<u8>,
    /// `HH:MM`.
    pub start: String,
    /// `HH:MM`; a window ending before it starts wraps past midnight.
    pub end: String,
}

impl TimeWindow {
    fn minutes(text: &str) -> Option<u32> {
        let (h, m) = text.split_once(':')?;
        let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
        (h < 24 && m < 60).then_some(h * 60 + m)
    }

    /// Whether `weekday` (1 = Monday) at `minute_of_day` is inside the window.
    pub fn admits(&self, weekday: u8, minute_of_day: u32) -> bool {
        let (Some(start), Some(end)) = (Self::minutes(&self.start), Self::minutes(&self.end))
        else {
            return false;
        };
        if !self.days.contains(&weekday) {
            return false;
        }
        if start <= end {
            (start..end).contains(&minute_of_day)
        } else {
            minute_of_day >= start || minute_of_day < end
        }
    }
}

/// Which actions need a human to say yes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ApprovalRule {
    /// Tools that wait for an approval before they run.
    #[serde(default)]
    #[schema(required = true)]
    pub tools: Vec<String>,
    /// A node that was given data of one of these labels needs an approval
    /// before anything leaves it (`write_file`, `make_docx`).
    #[serde(default)]
    #[schema(required = true)]
    pub labels: Vec<DataLabel>,
    /// Members who may decide; empty means every workspace admin.
    #[serde(default)]
    #[schema(required = true)]
    pub approvers: Vec<Uuid>,
    /// How long a call waits before the node fails; `null` inherits (default 600).
    #[serde(default)]
    #[schema(required = true)]
    pub timeout_s: Option<u32>,
}

/// Thresholds that raise an alert, or quarantine the agent.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AlertRule {
    /// Risk score at and above which an alert is raised.
    #[serde(default)]
    #[schema(required = true)]
    pub risk_score: Option<f64>,
    /// Risk score at and above which the agent is quarantined.
    #[serde(default)]
    #[schema(required = true)]
    pub quarantine_risk_score: Option<f64>,
    /// Policy denials in one run at and above which an alert is raised.
    #[serde(default)]
    #[schema(required = true)]
    pub denials_per_run: Option<u32>,
    /// Novelty (1 - similarity to every earlier run) at and above which an alert is raised.
    #[serde(default)]
    #[schema(required = true)]
    pub novelty: Option<f64>,
}

/// Where content of one label may go.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Outbound {
    /// Whether a node that read data of the label may deliver artifacts.
    #[serde(default)]
    #[schema(required = true)]
    pub artifacts: Option<bool>,
    /// Hosts such content may be sent to; `[]` none, `null` inherits.
    #[serde(default)]
    #[schema(required = true)]
    pub egress_hosts: Option<Vec<String>>,
}

/// The PII rule of the DLP classifier.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PiiRule {
    #[serde(default)]
    #[schema(required = true)]
    pub enabled: Option<bool>,
    /// The shapes looked for; `null` inherits (default: all of them).
    #[serde(default)]
    #[schema(required = true)]
    pub kinds: Option<Vec<PiiKind>>,
    /// Words that count as personal data in this workspace (code names, project names).
    #[serde(default)]
    #[schema(required = true)]
    pub keywords: Vec<String>,
    #[serde(default)]
    #[schema(required = true)]
    pub action: Option<RuleAction>,
}

/// Data loss prevention on the way out: artifacts and external calls.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DlpRule {
    /// What a credential found in an artifact leads to (default: deny).
    #[serde(default)]
    #[schema(required = true)]
    pub secrets: Option<RuleAction>,
    #[serde(default)]
    #[schema(required = true)]
    pub pii: PiiRule,
    /// Per label, where content derived from it may go.
    #[serde(default)]
    #[schema(required = true)]
    pub labels: BTreeMap<DataLabel, Outbound>,
}

/// The prompt-injection screen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InjectionRule {
    #[serde(default)]
    #[schema(required = true)]
    pub action: Option<InjectionAction>,
    /// Whether a small model is asked to confirm a finding (behind a runtime setting).
    #[serde(default)]
    #[schema(required = true)]
    pub model_check: Option<bool>,
}

/// Evidence-bound outputs (`docs/AGENTSECOPS.md` §2): whether a node's
/// output must cite what the node was given, how much of it, and what
/// happens when it does not.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRule {
    /// Whether the verifier acts on the verdict; `null` inherits (default: off,
    /// the verdict is only recorded).
    #[serde(default)]
    #[schema(required = true)]
    pub required: Option<bool>,
    /// The share of claims that must cite what was given for an output to be
    /// verified, 0 to 1; `null` inherits (default 0.8).
    #[serde(default)]
    #[schema(required = true)]
    pub min_coverage: Option<f64>,
    /// `mark` records the verdict, `reject` fails the node; `null` inherits (default mark).
    #[serde(default)]
    #[schema(required = true)]
    pub action: Option<EvidenceAction>,
}

/// One version of a policy. `{}` sets nothing and inherits everything.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicyDocument {
    #[serde(default)]
    #[schema(required = true)]
    pub tools: ListRule,
    #[serde(default)]
    #[schema(required = true)]
    pub egress: ListRule,
    #[serde(default)]
    #[schema(required = true)]
    pub models: ListRule,
    /// The classifications agents under this policy may read; `null` inherits.
    #[serde(default)]
    #[schema(required = true)]
    pub labels: Option<Vec<DataLabel>>,
    #[serde(default)]
    #[schema(required = true)]
    pub limits: Limits,
    /// When agents may start work; `null` inherits, `[]` never.
    #[serde(default)]
    #[schema(required = true)]
    pub time_windows: Option<Vec<TimeWindow>>,
    #[serde(default)]
    #[schema(required = true)]
    pub require_approval: ApprovalRule,
    #[serde(default)]
    #[schema(required = true)]
    pub alerts: AlertRule,
    #[serde(default)]
    #[schema(required = true)]
    pub dlp: DlpRule,
    #[serde(default)]
    #[schema(required = true)]
    pub injection: InjectionRule,
    #[serde(default)]
    #[schema(required = true)]
    pub evidence: EvidenceRule,
}

fn is_tool(name: &str) -> bool {
    super::mcp::is_tool_entry(name)
}

/// Whether `host` is a host name, optionally `*.`-prefixed, optionally with a port.
pub fn is_host(host: &str) -> bool {
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

fn check_list(errors: &mut FieldErrors, field: &str, items: &[String], ok: fn(&str) -> bool) {
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

fn check_list_rule(errors: &mut FieldErrors, field: &str, rule: &ListRule, ok: fn(&str) -> bool) {
    if let Some(allow) = &rule.allow {
        check_list(errors, &format!("{field}.allow"), allow, ok);
    }
    check_list(errors, &format!("{field}.deny"), &rule.deny, ok);
}

fn check_positive<T: PartialOrd + Default>(
    errors: &mut FieldErrors,
    field: &str,
    value: Option<T>,
) {
    if value.is_some_and(|v| v <= T::default()) {
        errors.add(field, "must be positive");
    }
}

impl PolicyDocument {
    /// Adds every problem with the document to `errors` (`document.<field>`).
    pub fn check(&self, errors: &mut FieldErrors) {
        check_list_rule(errors, "document.tools", &self.tools, is_tool);
        check_list_rule(errors, "document.egress", &self.egress, is_host);
        check_list_rule(errors, "document.models", &self.models, |_| true);
        if self
            .labels
            .as_ref()
            .is_some_and(|l| l.len() > DataLabel::ALL.len())
        {
            errors.add("document.labels", "repeats a label");
        }
        let limits = &self.limits;
        check_positive(
            errors,
            "document.limits.tokens_per_run",
            limits.tokens_per_run,
        );
        check_positive(
            errors,
            "document.limits.tokens_per_day",
            limits.tokens_per_day,
        );
        if limits
            .cost_usd_per_day
            .is_some_and(|c| !c.is_finite() || c <= 0.0)
        {
            errors.add("document.limits.cost_usd_per_day", "must be positive");
        }
        check_positive(
            errors,
            "document.limits.tool_calls_per_minute",
            limits.tool_calls_per_minute,
        );
        check_positive(
            errors,
            "document.limits.runs_per_hour",
            limits.runs_per_hour,
        );
        if limits.max_depth.is_some_and(|d| d > DEPTH_MAX) {
            errors.add(
                "document.limits.max_depth",
                format!("must be at most {DEPTH_MAX}"),
            );
        }
        if limits.max_fan_out.is_some_and(|f| f > FAN_OUT_MAX) {
            errors.add(
                "document.limits.max_fan_out",
                format!("must be at most {FAN_OUT_MAX}"),
            );
        }
        if let Some(windows) = &self.time_windows {
            if windows.len() > WINDOWS_MAX {
                errors.add(
                    "document.time_windows",
                    format!("must have at most {WINDOWS_MAX} windows"),
                );
            }
            let valid = windows.iter().all(|w| {
                !w.days.is_empty()
                    && w.days.iter().all(|d| (1..=7).contains(d))
                    && TimeWindow::minutes(&w.start).is_some()
                    && TimeWindow::minutes(&w.end).is_some()
            });
            if !valid {
                errors.add(
                    "document.time_windows",
                    "each window needs ISO weekdays (1-7) and HH:MM start and end",
                );
            }
        }
        let approval = &self.require_approval;
        check_list(
            errors,
            "document.require_approval.tools",
            &approval.tools,
            is_tool,
        );
        if approval.labels.len() > DataLabel::ALL.len() {
            errors.add("document.require_approval.labels", "repeats a label");
        }
        if approval.approvers.len() > APPROVERS_MAX {
            errors.add(
                "document.require_approval.approvers",
                format!("must have at most {APPROVERS_MAX} entries"),
            );
        }
        if approval
            .timeout_s
            .is_some_and(|t| t == 0 || t > APPROVAL_TIMEOUT_MAX_S)
        {
            errors.add(
                "document.require_approval.timeout_s",
                format!("must be between 1 and {APPROVAL_TIMEOUT_MAX_S}"),
            );
        }
        let alerts = &self.alerts;
        for (field, value) in [
            ("risk_score", alerts.risk_score),
            ("quarantine_risk_score", alerts.quarantine_risk_score),
            ("novelty", alerts.novelty),
        ] {
            if value.is_some_and(|v| !v.is_finite() || v < 0.0) {
                errors.add(&format!("document.alerts.{field}"), "must be zero or more");
            }
        }
        if alerts.novelty.is_some_and(|n| n > 1.0) {
            errors.add("document.alerts.novelty", "is a fraction between 0 and 1");
        }
        check_positive(
            errors,
            "document.alerts.denials_per_run",
            alerts.denials_per_run,
        );
        let pii = &self.dlp.pii;
        if pii.keywords.len() > KEYWORDS_MAX {
            errors.add(
                "document.dlp.pii.keywords",
                format!("must have at most {KEYWORDS_MAX} entries"),
            );
        }
        if pii
            .keywords
            .iter()
            .any(|k| k.trim().is_empty() || k.chars().count() > KEYWORD_MAX)
        {
            errors.add(
                "document.dlp.pii.keywords",
                "has an entry that is not valid",
            );
        }
        if pii
            .kinds
            .as_ref()
            .is_some_and(|k| k.len() > PiiKind::ALL.len())
        {
            errors.add("document.dlp.pii.kinds", "repeats a kind");
        }
        for (label, outbound) in &self.dlp.labels {
            if let Some(hosts) = &outbound.egress_hosts {
                check_list(
                    errors,
                    &format!("document.dlp.labels.{label}.egress_hosts"),
                    hosts,
                    is_host,
                );
            }
        }
        if self
            .evidence
            .min_coverage
            .is_some_and(|m| !m.is_finite() || !(0.0..=1.0).contains(&m))
        {
            errors.add(
                "document.evidence.min_coverage",
                "is a fraction between 0 and 1",
            );
        }
    }
}

/// One stored version of a policy.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Policy {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub scope: PolicyScope,
    /// The team or agent; `null` at the workspace level.
    #[schema(required = true)]
    pub scope_id: Option<Uuid>,
    pub version: i32,
    pub document: PolicyDocument,
    #[schema(required = true)]
    pub author_id: Option<Uuid>,
    pub author_name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Flattens a JSON value into `path -> leaf` pairs (`a.b[2]`).
fn flatten(value: &serde_json::Value, prefix: &str, out: &mut BTreeMap<String, String>) {
    match value {
        serde_json::Value::Object(map) => {
            if map.is_empty() {
                out.insert(prefix.to_owned(), "{}".into());
            }
            for (key, inner) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(inner, &path, out);
            }
        }
        serde_json::Value::Array(items) => {
            out.insert(prefix.to_owned(), value.to_string());
            let _ = items;
        }
        other => {
            out.insert(prefix.to_owned(), other.to_string());
        }
    }
}

/// What changed between two documents, in words for the audit log:
/// `tools.allow: null -> ["finish"]; limits.tokens_per_run: 50000 -> 20000`.
pub fn diff_summary(before: &PolicyDocument, after: &PolicyDocument) -> String {
    let (mut old, mut new) = (BTreeMap::new(), BTreeMap::new());
    flatten(
        &serde_json::to_value(before).unwrap_or_default(),
        "",
        &mut old,
    );
    flatten(
        &serde_json::to_value(after).unwrap_or_default(),
        "",
        &mut new,
    );
    let mut changes = Vec::new();
    for (path, value) in &new {
        match old.get(path) {
            Some(previous) if previous == value => {}
            Some(previous) => changes.push(format!("{path}: {previous} -> {value}")),
            None => changes.push(format!("{path}: {value}")),
        }
    }
    for path in old.keys().filter(|p| !new.contains_key(*p)) {
        changes.push(format!("{path}: removed"));
    }
    if changes.is_empty() {
        "no change".to_owned()
    } else {
        let mut text = changes.join("; ");
        if text.chars().count() > 1_000 {
            text = text.chars().take(1_000).collect::<String>() + "…";
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_empty_document_sets_nothing() {
        let doc: PolicyDocument = serde_json::from_str("{}").unwrap();
        assert_eq!(doc, PolicyDocument::default());
        assert_eq!(doc.tools.allow, None);
        assert!(doc.tools.deny.is_empty());
        assert_eq!(doc.labels, None);
        assert_eq!(doc.limits.tokens_per_run, None);
        let mut errors = FieldErrors::default();
        doc.check(&mut errors);
        assert!(errors.is_empty());
        assert!(serde_json::from_value::<PolicyDocument>(json!({"typo": 1})).is_err());
        let again: PolicyDocument =
            serde_json::from_value(serde_json::to_value(&doc).unwrap()).unwrap();
        assert_eq!(again, doc);
    }

    #[test]
    fn checks_every_rule() {
        let doc: PolicyDocument = serde_json::from_value(json!({
            "tools": {"allow": ["write file"], "deny": []},
            "egress": {"deny": ["Bad Host"]},
            "limits": {"tokens_per_run": 0, "cost_usd_per_day": -1.0, "max_depth": 9, "max_fan_out": 40},
            "time_windows": [{"days": [8], "start": "25:00", "end": "17:00"}],
            "require_approval": {"tools": ["ok"], "timeout_s": 0},
            "alerts": {"risk_score": -1.0, "novelty": 2.0, "denials_per_run": 0},
            "dlp": {"pii": {"keywords": [""]}, "labels": {"restricted": {"egress_hosts": ["x y"]}}},
        }))
        .unwrap();
        let mut errors = FieldErrors::default();
        doc.check(&mut errors);
        let fields: Vec<&String> = errors.as_map().keys().collect();
        assert_eq!(
            fields,
            [
                "document.alerts.denials_per_run",
                "document.alerts.novelty",
                "document.alerts.risk_score",
                "document.dlp.labels.restricted.egress_hosts",
                "document.dlp.pii.keywords",
                "document.egress.deny",
                "document.limits.cost_usd_per_day",
                "document.limits.max_depth",
                "document.limits.max_fan_out",
                "document.limits.tokens_per_run",
                "document.require_approval.timeout_s",
                "document.time_windows",
                "document.tools.allow",
            ]
        );
    }

    #[test]
    fn time_windows_admit_by_weekday_and_minute_and_wrap_midnight() {
        let office = TimeWindow {
            days: vec![1, 2, 3, 4, 5],
            start: "09:00".into(),
            end: "17:30".into(),
        };
        assert!(office.admits(1, 9 * 60));
        assert!(office.admits(5, 17 * 60 + 29));
        assert!(!office.admits(5, 17 * 60 + 30));
        assert!(!office.admits(6, 12 * 60));
        let night = TimeWindow {
            days: vec![7],
            start: "22:00".into(),
            end: "02:00".into(),
        };
        assert!(night.admits(7, 23 * 60) && night.admits(7, 60));
        assert!(!night.admits(7, 12 * 60));
        let broken = TimeWindow {
            days: vec![1],
            start: "9".into(),
            end: "17:00".into(),
        };
        assert!(!broken.admits(1, 600));
    }

    #[test]
    fn the_diff_names_what_changed() {
        let before = PolicyDocument::default();
        let after: PolicyDocument = serde_json::from_value(json!({
            "tools": {"allow": ["finish"]},
            "limits": {"tokens_per_run": 20000},
        }))
        .unwrap();
        let summary = diff_summary(&before, &after);
        assert!(
            summary.contains("tools.allow: null -> [\"finish\"]"),
            "{summary}"
        );
        assert!(
            summary.contains("limits.tokens_per_run: null -> 20000"),
            "{summary}"
        );
        assert_eq!(diff_summary(&after, &after), "no change");
        assert!(RuleAction::Alert.stricter(RuleAction::Deny) == RuleAction::Deny);
        assert!(is_host("*.example.com:8443") && !is_host("Example.com"));
    }

    #[test]
    fn the_evidence_rule_is_a_fraction_and_reject_is_the_stricter_action() {
        let doc: PolicyDocument = serde_json::from_value(json!({
            "evidence": {"required": true, "min_coverage": 0.9, "action": "reject"},
        }))
        .unwrap();
        assert_eq!(doc.evidence.required, Some(true));
        assert_eq!(doc.evidence.action, Some(EvidenceAction::Reject));
        let mut errors = FieldErrors::default();
        doc.check(&mut errors);
        assert!(errors.is_empty());
        for bad in [1.5, -0.1, f64::NAN] {
            let doc = PolicyDocument {
                evidence: EvidenceRule {
                    min_coverage: Some(bad),
                    ..EvidenceRule::default()
                },
                ..PolicyDocument::default()
            };
            let mut errors = FieldErrors::default();
            doc.check(&mut errors);
            assert!(
                errors
                    .as_map()
                    .contains_key("document.evidence.min_coverage")
            );
        }
        assert_eq!(
            EvidenceAction::Mark.stricter(EvidenceAction::Reject),
            EvidenceAction::Reject
        );
        assert_eq!(
            EvidenceAction::Mark.stricter(EvidenceAction::Mark),
            EvidenceAction::Mark
        );
        assert!(
            serde_json::from_value::<PolicyDocument>(json!({"evidence": {"typo": 1}})).is_err()
        );
    }
}
