//! Issues, workflow states and projects.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::cycle::CycleRef;
use super::string_enum;

/// Maximum issue and project title length.
pub const TITLE_MAX: usize = 200;
/// Maximum issue and project description length in bytes (64 KiB).
pub const DESCRIPTION_MAX_BYTES: usize = 64 * 1024;
/// Maximum workflow state name length.
pub const STATE_NAME_MAX: usize = 40;
/// Most workflow states a team may have.
pub const STATES_MAX: usize = 30;
/// Lowest priority value (`0` none, `1` urgent, `2` high, `3` medium, `4` low).
pub const PRIORITY_MAX: i16 = 4;
pub const COMMENT_MAX_BYTES: usize = 16 * 1024;
pub const LABEL_NAME_MAX: usize = 40;
pub const LABELS_MAX: i64 = 200;
pub const ISSUE_LABELS_MAX: usize = 20;
/// Priority names by value, as shown in an issue's history.
pub const PRIORITY_NAMES: [&str; 5] = ["No priority", "Urgent", "High", "Medium", "Low"];

string_enum!(
    /// What a workflow state means, whatever a team calls it. Reports and
    /// "is this done?" questions use the category, never the name.
    StateCategory {
        Backlog => "backlog",
        Unstarted => "unstarted",
        Started => "started",
        Completed => "completed",
        Canceled => "canceled",
    }
);

impl StateCategory {
    /// An issue in a state of this category needs no more work.
    pub fn is_closed(self) -> bool {
        matches!(self, StateCategory::Completed | StateCategory::Canceled)
    }
}

string_enum!(
    /// Where a project stands.
    ProjectStatus {
        Planned => "planned",
        Started => "started",
        Paused => "paused",
        Completed => "completed",
        Canceled => "canceled",
    }
);

/// One state of a team's workflow.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct IssueState {
    pub id: Uuid,
    pub team_id: Uuid,
    pub name: String,
    pub category: StateCategory,
    /// `#rrggbb`.
    pub color: String,
    /// Order within the workflow, lowest first.
    pub position: i32,
}

/// The workflow a new team starts with: `(name, category, color)`.
pub const STARTER_STATES: [(&str, StateCategory, &str); 6] = [
    ("Backlog", StateCategory::Backlog, "#94a3b8"),
    ("Todo", StateCategory::Unstarted, "#64748b"),
    ("In Progress", StateCategory::Started, "#f59e0b"),
    ("In Review", StateCategory::Started, "#0ea5e9"),
    ("Done", StateCategory::Completed, "#10b981"),
    ("Canceled", StateCategory::Canceled, "#ef4444"),
];

/// Someone an issue refers to.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct IssuePerson {
    pub user_id: Uuid,
    pub name: String,
}

/// An issue.
/// A label of a workspace, as carried by an issue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Label {
    pub id: Uuid,
    pub name: String,
    /// `#rrggbb`.
    pub color: String,
}

/// Another issue, as much as a link to it shows.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct IssueRef {
    pub id: Uuid,
    pub identifier: String,
    pub title: String,
}

/// How many sub-issues an issue has, and how many of them are closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
pub struct SubIssueCount {
    pub total: i64,
    pub closed: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Issue {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub team_id: Uuid,
    /// The team key and the issue number, e.g. `ENG-12`.
    pub identifier: String,
    pub number: i32,
    pub title: String,
    pub description: String,
    pub state: IssueState,
    /// `0` none, `1` urgent, `2` high, `3` medium, `4` low.
    pub priority: i16,
    /// The issue's labels, by name.
    pub labels: Vec<Label>,
    /// The issue this one is part of, if the caller can see it.
    #[schema(required = true)]
    pub parent: Option<IssueRef>,
    pub sub_issues: SubIssueCount,
    /// The cycle of its team the issue is planned in.
    #[schema(required = true)]
    pub cycle: Option<CycleRef>,
    #[schema(required = true)]
    pub assignee: Option<IssuePerson>,
    /// The agent the issue is delegated to.
    #[schema(required = true)]
    pub agent_id: Option<Uuid>,
    #[schema(required = true)]
    pub project_id: Option<Uuid>,
    /// The graph that plans and executes the issue, if one was created.
    #[schema(required = true)]
    pub graph_id: Option<Uuid>,
    #[schema(required = true)]
    pub creator_id: Option<Uuid>,
    /// The day the issue is due, if it has one.
    #[schema(required = true)]
    pub due_date: Option<NaiveDate>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[schema(required = true)]
    pub completed_at: Option<DateTime<Utc>>,
}

/// A project: a body of work that issues of any team can belong to.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Project {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub description: String,
    pub status: ProjectStatus,
    #[schema(required = true)]
    pub lead_id: Option<Uuid>,
    #[schema(required = true)]
    pub target_date: Option<NaiveDate>,
    /// Issues of the project the caller can see.
    pub issue_count: i64,
    /// Of those, the ones in a completed or canceled state.
    pub closed_count: i64,
    pub created_at: DateTime<Utc>,
}

string_enum!(
    /// What an entry of an issue's timeline records.
    IssueEventKind {
        Comment => "comment",
        State => "state",
        Priority => "priority",
        Assignee => "assignee",
        Title => "title",
        /// The due date was set, moved or removed.
        Due => "due",
        /// A run of the issue's graph finished: its result, with the run it came from.
        Run => "run",
    }
);

/// One entry of an issue's timeline: a comment, or a change to the issue.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct IssueEvent {
    pub id: Uuid,
    pub issue_id: Uuid,
    pub kind: IssueEventKind,
    /// Who wrote the comment or made the change; `null` once the account is gone.
    #[schema(required = true)]
    pub actor: Option<IssuePerson>,
    /// The text of a comment; empty for changes.
    pub body: String,
    /// What a change replaced, as it was shown then (a state name, a person).
    #[schema(required = true)]
    pub from: Option<String>,
    /// What a change set.
    #[schema(required = true)]
    pub to: Option<String>,
    pub created_at: DateTime<Utc>,
    /// When a comment was last edited.
    #[schema(required = true)]
    pub edited_at: Option<DateTime<Utc>>,
    /// The run a `run` entry reports; `null` for other entries and once the run is deleted.
    #[schema(required = true)]
    pub run_id: Option<Uuid>,
}

/// A change to record in an issue's timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueChange {
    pub kind: IssueEventKind,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// The timeline entries for an edit that turned `before` into `after`.
pub fn changes(before: &Issue, after: &Issue) -> Vec<IssueChange> {
    let mut out = Vec::new();
    let mut push = |kind, from: Option<String>, to: Option<String>| {
        if from != to {
            out.push(IssueChange { kind, from, to });
        }
    };
    push(
        IssueEventKind::Title,
        Some(before.title.clone()),
        Some(after.title.clone()),
    );
    if before.state.id != after.state.id {
        push(
            IssueEventKind::State,
            Some(before.state.name.clone()),
            Some(after.state.name.clone()),
        );
    }
    let priority = |i: &Issue| {
        PRIORITY_NAMES
            .get(usize::try_from(i.priority).unwrap_or_default())
            .map(|name| (*name).to_owned())
    };
    push(IssueEventKind::Priority, priority(before), priority(after));
    let assignee = |i: &Issue| i.assignee.as_ref().map(|a| a.user_id);
    if assignee(before) != assignee(after) {
        let name = |i: &Issue| i.assignee.as_ref().map(|a| a.name.clone());
        out.push(IssueChange {
            kind: IssueEventKind::Assignee,
            from: name(before),
            to: name(after),
        });
    }
    if before.due_date != after.due_date {
        let day = |i: &Issue| i.due_date.map(|d| d.to_string());
        out.push(IssueChange {
            kind: IssueEventKind::Due,
            from: day(before),
            to: day(after),
        });
    }
    out
}

string_enum!(
    /// Why an issue is in someone's inbox.
    NotificationKind {
        Assigned => "assigned",
        Comment => "comment",
        State => "state",
        /// The issue's due date has arrived (nobody did it: there is no actor).
        Due => "due",
        /// A tool call of a run waits for the member's approval.
        Approval => "approval",
        /// Something about an agent needs a look (a DLP finding, a risk score, an injection).
        Alert => "alert",
        /// A run that executes the issue finished; its result is on the issue.
        Run => "run",
    }
);

/// The issue a notification is about, as much as a list row shows.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NotificationIssue {
    pub id: Uuid,
    pub identifier: String,
    pub title: String,
}

/// The approval an inbox entry asks for, as much as the inbox shows.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NotificationApproval {
    pub id: Uuid,
    pub run_id: Uuid,
    pub node_id: Uuid,
    pub graph_id: Uuid,
    /// The acting principal.
    pub principal: String,
    pub tool: String,
    pub reason: String,
    pub status: super::approval::ApprovalStatus,
    pub expires_at: DateTime<Utc>,
}

/// The alert an inbox entry points at, as much as the inbox shows.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NotificationAlert {
    pub id: Uuid,
    pub kind: super::alert::AlertKind,
    pub severity: super::policy_document::Severity,
    pub rule: String,
    pub title: String,
    pub principal: String,
    #[schema(required = true)]
    pub run_id: Option<Uuid>,
    #[schema(required = true)]
    pub graph_id: Option<Uuid>,
}

/// One entry of a member's inbox.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Notification {
    pub id: Uuid,
    pub kind: NotificationKind,
    /// The issue (`assigned`, `comment`, `state`, `due`); `null` for the other kinds.
    #[schema(required = true)]
    pub issue: Option<NotificationIssue>,
    /// The approval to decide (`approval`); `null` for the other kinds.
    #[schema(required = true)]
    pub approval: Option<NotificationApproval>,
    /// The alert to look at (`alert`); `null` for the other kinds.
    #[schema(required = true)]
    pub alert: Option<NotificationAlert>,
    /// Who assigned, commented or moved the issue; `null` once the account is gone.
    #[schema(required = true)]
    pub actor: Option<IssuePerson>,
    pub created_at: DateTime<Utc>,
    #[schema(required = true)]
    pub read_at: Option<DateTime<Utc>>,
}

/// Most characters of a run's result an issue's timeline keeps; the run
/// itself keeps everything, and the entry links to it.
pub const RUN_REPORT_MAX_CHARS: usize = 6_000;

/// What one node of a finished run left: its output, or why it failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeOutcome {
    pub title: String,
    pub text: String,
}

/// The body of the `run` entry a finished run writes on its issue: what the
/// last nodes of the graph produced (the deliverable, not every step on the
/// way), then what failed. One result stands alone; several are headed by
/// their node. Cut to [`RUN_REPORT_MAX_CHARS`] with a marker that says how
/// much the run holds.
pub fn run_report(results: &[NodeOutcome], failures: &[NodeOutcome]) -> String {
    let mut out = String::new();
    let results: Vec<&NodeOutcome> = results
        .iter()
        .filter(|r| !r.text.trim().is_empty())
        .collect();
    match results.as_slice() {
        [] => {}
        [only] => out.push_str(only.text.trim()),
        many => {
            for (i, r) in many.iter().enumerate() {
                if i > 0 {
                    out.push_str("\n\n");
                }
                out.push_str(&format!("### {}\n\n{}", r.title.trim(), r.text.trim()));
            }
        }
    }
    for f in failures {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        let why = f.text.trim();
        let why = if why.is_empty() {
            "no reason was given"
        } else {
            why
        };
        out.push_str(&format!("**{}** failed: {}", f.title.trim(), why));
    }
    let total = out.chars().count();
    if total <= RUN_REPORT_MAX_CHARS {
        return out;
    }
    let marker = format!("\n\n[… the run holds {total} characters; open it for the rest]");
    let keep = RUN_REPORT_MAX_CHARS.saturating_sub(marker.chars().count());
    let mut cut: String = out.chars().take(keep).collect();
    // End on a whole line when one is near, rather than mid-word.
    if let Some(at) = cut.rfind('\n').filter(|&at| at > keep / 2) {
        cut.truncate(at);
    }
    cut.truncate(cut.trim_end().len());
    cut.push_str(&marker);
    cut
}

/// `#rrggbb`.
pub fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_report_carries_the_deliverable_and_what_failed() {
        let node = |title: &str, text: &str| NodeOutcome {
            title: title.into(),
            text: text.into(),
        };
        // One result stands alone.
        assert_eq!(
            run_report(&[node("Draft", "  The report.  ")], &[]),
            "The report."
        );
        // Several are headed by their node; empty outputs are left out.
        assert_eq!(
            run_report(&[node("A", "one"), node("B", " "), node("C", "two")], &[]),
            "### A\n\none\n\n### C\n\ntwo"
        );
        // Failures follow, with a reason even when none was recorded.
        assert_eq!(
            run_report(&[], &[node("Fetch", "timed out"), node("Parse", "")]),
            "**Fetch** failed: timed out\n\n**Parse** failed: no reason was given"
        );
        // A long result is cut on a line, with a marker that says how much the run holds.
        let long = "line of the result\n".repeat(1_000);
        let cut = run_report(&[node("Draft", &long)], &[]);
        assert!(cut.chars().count() <= RUN_REPORT_MAX_CHARS, "{}", cut.len());
        assert!(cut.ends_with("open it for the rest]"));
        assert!(
            cut.contains("line of the result\n\n[…"),
            "cut on a whole line"
        );
    }

    #[test]
    fn the_starter_workflow_covers_every_category_once_it_matters() {
        let mut names = std::collections::HashSet::new();
        for (name, _, color) in STARTER_STATES {
            assert!(names.insert(name), "state names are unique within a team");
            assert!(is_hex_color(color));
        }
        for category in ["backlog", "unstarted", "started", "completed", "canceled"] {
            let category: StateCategory = category.parse().unwrap();
            assert!(STARTER_STATES.iter().any(|(_, c, _)| *c == category));
        }
        assert!(StateCategory::Canceled.is_closed() && !StateCategory::Started.is_closed());
        assert!(!is_hex_color("red") && !is_hex_color("#12345g"));
    }
}
