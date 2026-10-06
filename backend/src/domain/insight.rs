//! What a workspace's owners want to see at a glance: what happened on a
//! day, across everything in the workspace, and how its parts relate.

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

/// One thing that happened in a workspace.
#[derive(Debug, Clone, Serialize, ToSchema, sqlx::FromRow)]
pub struct TimelineEntry {
    pub at: DateTime<Utc>,
    /// What happened: an audit action (`member_added`, `team_created`, …),
    /// `issue_created`, `issue_state`, `issue_comment`, `issue_assignee`,
    /// `issue_priority`, `issue_title`, `graph_created`, `run_succeeded`,
    /// `run_failed` (and the other run statuses), `document_added`,
    /// `memories_learned`.
    pub kind: String,
    /// Who did it, by name; `null` for what the system did or whose account is gone.
    #[schema(required = true)]
    pub actor: Option<String>,
    /// What it happened to: `workspace`, `issue`, `graph`, `document`.
    pub entity_type: String,
    /// The id of that thing, when it can be opened.
    #[schema(required = true)]
    pub entity_id: Option<Uuid>,
    /// Its name as it was shown: an issue's identifier and title, a graph's name.
    pub title: String,
    /// What changed, in words.
    pub detail: String,
}

/// How much happened on one day.
#[derive(Debug, Clone, Serialize, ToSchema, sqlx::FromRow)]
pub struct TimelineDay {
    /// The day (UTC).
    pub day: NaiveDate,
    pub events: i64,
}

/// One kind of thing a workspace holds, and how many of it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MapEntity {
    /// `member`, `team`, `project`, `issue`, `graph`, `run`, `agent`,
    /// `document`, `passage`, `memory`, `label`, `cycle`.
    pub key: String,
    pub label: String,
    pub count: i64,
}

/// How two kinds of things are tied together in this workspace.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MapRelation {
    pub from: String,
    pub to: String,
    /// The relation, read from `from` to `to`: "belongs to", "plans", …
    pub label: String,
    /// How many such ties exist right now.
    pub count: i64,
}

/// The relationship map of a workspace: its kinds of things and the ties
/// between them, with today's numbers.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct WorkspaceMap {
    pub entities: Vec<MapEntity>,
    pub relations: Vec<MapRelation>,
}
