//! Issues, workflow states and projects.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::cycle::CycleRef;
use super::run::RunStatus;
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
/// Most characters of a link to a GitHub issue or pull request, as given.
pub const GITHUB_URL_MAX: usize = 300;
/// Why a GitHub link was refused (the message of the `github_url` field error).
pub const GITHUB_URL_INVALID: &str =
    "must be a GitHub issue or pull request URL, like https://github.com/owner/repo/issues/12";

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
    /// The GitHub issue or pull request the issue is about (a link only; nothing is synced).
    #[schema(required = true)]
    pub github_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[schema(required = true)]
    pub completed_at: Option<DateTime<Utc>>,
    /// The status of the latest run of the issue's graph; `null` without a graph or runs.
    #[schema(required = true)]
    pub run_status: Option<RunStatus>,
}

/// An issue as a list shows it: every field of [`Issue`] but its
/// description, which only the issue's own page reads (it may be 64 KiB).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct IssueSummary {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub team_id: Uuid,
    /// The team key and the issue number, e.g. `ENG-12`.
    pub identifier: String,
    pub number: i32,
    pub title: String,
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
    /// The GitHub issue or pull request the issue is about (a link only; nothing is synced).
    #[schema(required = true)]
    pub github_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[schema(required = true)]
    pub completed_at: Option<DateTime<Utc>>,
    /// The status of the latest run of the issue's graph; `null` without a graph or runs.
    #[schema(required = true)]
    pub run_status: Option<RunStatus>,
}

impl IssueSummary {
    /// The whole issue, given its description.
    pub fn with_description(self, description: String) -> Issue {
        Issue {
            id: self.id,
            workspace_id: self.workspace_id,
            team_id: self.team_id,
            identifier: self.identifier,
            number: self.number,
            title: self.title,
            description,
            state: self.state,
            priority: self.priority,
            labels: self.labels,
            parent: self.parent,
            sub_issues: self.sub_issues,
            cycle: self.cycle,
            assignee: self.assignee,
            agent_id: self.agent_id,
            project_id: self.project_id,
            graph_id: self.graph_id,
            creator_id: self.creator_id,
            due_date: self.due_date,
            github_url: self.github_url,
            created_at: self.created_at,
            updated_at: self.updated_at,
            completed_at: self.completed_at,
            run_status: self.run_status,
        }
    }
}

/// Most unread notifications counted for the sidebar's badge: the inbox
/// shows the newest 100 (`GET /workspaces/{wid}/inbox`), and the badge counts
/// the unread among those, so it never counts one the inbox cannot show or
/// mark read.
pub const UNREAD_MAX: i64 = 100;

/// How many of a member's notifications are unread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
pub struct UnreadCount {
    /// At most [`UNREAD_MAX`].
    pub count: i64,
}

/// A team, as much as a project shows of the team that owns it.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct TeamRef {
    pub id: Uuid,
    pub key: String,
    pub name: String,
}

/// A project: a body of work that issues of any team can belong to, owned
/// by one team or by the workspace as a whole.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Project {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub description: String,
    pub status: ProjectStatus,
    /// The team that owns the project; `null` for a project of the whole workspace.
    #[schema(required = true)]
    pub team_id: Option<Uuid>,
    /// That team's key and name, for display.
    #[schema(required = true)]
    pub team: Option<TeamRef>,
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
        /// The GitHub link was set, changed or removed (`from` and `to` are the links).
        Github => "github",
        /// One of the issue's files was uploaded, updated, duplicated or deleted: `body` is
        /// the file's name, `from` the version it replaced and `to` what was done
        /// (`uploaded`, `updated:v<N>`, `duplicated`, `deleted`).
        Artifact => "artifact",
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
    if before.github_url != after.github_url {
        out.push(IssueChange {
            kind: IssueEventKind::Github,
            from: before.github_url.clone(),
            to: after.github_url.clone(),
        });
    }
    out
}

/// The canonical form of a link to a GitHub issue or pull request,
/// `https://github.com/<owner>/<repo>/issues|pull/<number>`, or `None` for a
/// blank one, which removes the link. A fragment, a query and trailing
/// slashes are dropped; `http://`, `www.` and a missing scheme become
/// `https://github.com/`. Anything else is refused with
/// [`GITHUB_URL_INVALID`].
pub fn github_url(raw: &str) -> Result<Option<String>, &'static str> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    if raw.chars().count() > GITHUB_URL_MAX {
        return Err(GITHUB_URL_INVALID);
    }
    let cut = raw.find(['#', '?']).map_or(raw, |at| &raw[..at]);
    let cut = cut.trim_end_matches('/');
    let lower = cut.to_ascii_lowercase();
    let rest = ["https://", "http://", ""]
        .iter()
        .flat_map(|scheme| ["github.com/", "www.github.com/"].map(|host| format!("{scheme}{host}")))
        .find_map(|prefix| lower.starts_with(&prefix).then(|| &cut[prefix.len()..]))
        .ok_or(GITHUB_URL_INVALID)?;
    let parts: Vec<&str> = rest.split('/').collect();
    let [owner, repo, kind, number] = parts.as_slice() else {
        return Err(GITHUB_URL_INVALID);
    };
    let kind = kind.to_ascii_lowercase();
    let owner_ok =
        !owner.is_empty() && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let repo_ok = !matches!(*repo, "" | "." | "..")
        && repo
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    let number_ok = !number.is_empty()
        && !number.starts_with('0')
        && number.chars().all(|c| c.is_ascii_digit());
    if !(owner_ok && repo_ok && number_ok && matches!(kind.as_str(), "issues" | "pull")) {
        return Err(GITHUB_URL_INVALID);
    }
    Ok(Some(format!(
        "https://github.com/{owner}/{repo}/{kind}/{number}"
    )))
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
    fn github_links_are_kept_in_one_canonical_form() {
        let canonical = |raw: &str| github_url(raw).unwrap();
        assert_eq!(
            canonical("https://github.com/acme/app/issues/12"),
            Some("https://github.com/acme/app/issues/12".to_owned())
        );
        // Trailing slashes, fragments and queries go; http, www and no scheme become https.
        for raw in [
            "  https://github.com/acme/app/pull/7/  ",
            "https://github.com/acme/app/pull/7#issuecomment-1",
            "https://github.com/acme/app/pull/7?diff=split",
            "http://github.com/acme/app/pull/7",
            "https://www.github.com/acme/app/pull/7",
            "HTTP://WWW.GitHub.com/acme/app/pull/7",
            "github.com/acme/app/pull/7",
        ] {
            assert_eq!(
                canonical(raw).as_deref(),
                Some("https://github.com/acme/app/pull/7"),
                "{raw}"
            );
        }
        assert_eq!(
            canonical("https://github.com/my-org/repo.name_x/issues/3").as_deref(),
            Some("https://github.com/my-org/repo.name_x/issues/3")
        );
        // Blank removes the link.
        assert_eq!(canonical("   "), None);
        for bad in [
            "https://gitlab.com/acme/app/issues/1",
            "https://github.com.evil.com/acme/app/issues/1",
            "https://evil.com/github.com/acme/app/issues/1",
            "https://github.com/acme/app",
            "https://github.com/acme/app/issues",
            "https://github.com/acme/app/issues/abc",
            "https://github.com/acme/app/issues/012",
            "https://github.com/acme/app/discussions/1",
            "https://github.com/acme/app/pull/1/files",
            "https://github.com/ac me/app/issues/1",
            "https://github.com/acme/../issues/1",
            "https://user@github.com/acme/app/issues/1",
            "javascript:alert(1)",
        ] {
            assert_eq!(github_url(bad), Err(GITHUB_URL_INVALID), "{bad}");
        }
        let long = format!("https://github.com/acme/{}/issues/1", "a".repeat(300));
        assert_eq!(github_url(&long), Err(GITHUB_URL_INVALID));
        // What it keeps is what the database's check accepts.
        let kept = canonical("http://www.github.com/a-b/c.d_e-f/issues/99/").unwrap();
        assert!(kept.starts_with("https://github.com/") && !kept.ends_with('/'));
    }

    #[test]
    fn a_github_link_change_is_recorded() {
        let issue = |url: Option<&str>| Issue {
            id: Uuid::nil(),
            workspace_id: Uuid::nil(),
            team_id: Uuid::nil(),
            identifier: "ENG-1".into(),
            number: 1,
            title: "Fix".into(),
            description: String::new(),
            state: IssueState {
                id: Uuid::nil(),
                team_id: Uuid::nil(),
                name: "Todo".into(),
                category: StateCategory::Unstarted,
                color: "#64748b".into(),
                position: 0,
            },
            priority: 0,
            labels: Vec::new(),
            parent: None,
            sub_issues: SubIssueCount {
                total: 0,
                closed: 0,
            },
            cycle: None,
            assignee: None,
            agent_id: None,
            project_id: None,
            graph_id: None,
            creator_id: None,
            due_date: None,
            github_url: url.map(str::to_owned),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
            run_status: None,
        };
        let link = "https://github.com/acme/app/issues/1";
        assert_eq!(
            changes(&issue(None), &issue(Some(link))),
            [IssueChange {
                kind: IssueEventKind::Github,
                from: None,
                to: Some(link.to_owned()),
            }]
        );
        assert!(changes(&issue(Some(link)), &issue(Some(link))).is_empty());
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
