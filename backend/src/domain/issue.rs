//! Issues, workflow states and projects.

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

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
    out
}

/// `#rrggbb`.
pub fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

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
