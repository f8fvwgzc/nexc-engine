//! What a workspace's owners want to see at a glance: what happened on a
//! day, across everything in the workspace, and how its parts relate.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::string_enum;

/// Name of the structured output a day summary is asked for in.
pub const DAY_SUMMARY_SCHEMA_NAME: &str = "day_summary";
/// Most entries of a day put before the model, each on a line of its own.
pub const DIGEST_LINES: usize = 240;
/// Longest line of a digest.
const DIGEST_LINE_CHARS: usize = 220;
/// Bounds of what is kept of a summary, whatever the model returned.
pub const HEADLINE_MAX: usize = 300;
pub const POINTS_MAX: usize = 8;
pub const POINT_MAX: usize = 400;

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

string_enum!(
    /// A kind of thing on the workspace map whose ties can be followed.
    MapKind {
        Member => "member",
        Team => "team",
        Project => "project",
        Issue => "issue",
        Graph => "graph",
        Document => "document",
        Agent => "agent",
    }
);

/// One thing of a workspace, as the map names it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MapItem {
    /// A [`MapKind`] when its ties can be followed; otherwise a kind that is
    /// only named here: `label`, `cycle`, `topic`.
    pub kind: String,
    pub id: Uuid,
    /// A member's name, an issue's identifier and title, a graph's name.
    pub title: String,
    /// What tells it apart at a glance: an e-mail, a team key, a state.
    pub subtitle: String,
}

/// What one thing is tied to in one way.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MapTie {
    /// The relation, read from the thing: "is on", "tracks", "was filed by".
    pub label: String,
    /// The kind of thing at the other end (also `run`, `node`, `memory`,
    /// `passage`, which are only counted).
    pub kind: String,
    /// How many there are.
    pub count: i64,
    /// The first of them; empty for kinds that are only counted.
    pub items: Vec<MapItem>,
}

/// One thing of a workspace with everything it is tied to.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MapNeighbourhood {
    pub item: MapItem,
    /// Its ties that exist; a tie to nothing is left out.
    pub ties: Vec<MapTie>,
}

/// A day of a workspace's timeline, told in a few lines by its model.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DaySummary {
    /// The day (UTC).
    pub day: NaiveDate,
    /// The day in one sentence.
    pub headline: String,
    /// What happened, most important first.
    pub highlights: Vec<String>,
    /// What deserves a look: failures, blocked work, unusual changes. Often empty.
    pub attention: Vec<String>,
    /// How many timeline entries it was written from.
    pub event_count: i64,
    /// Whether more has happened on the day since it was written.
    pub stale: bool,
    /// The model that wrote it.
    pub model: String,
    /// Who asked for it, by name; `null` once the account is gone.
    #[schema(required = true)]
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// What the model returns for a day.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DaySummaryOutput {
    #[serde(default)]
    pub headline: String,
    #[serde(default)]
    pub highlights: Vec<String>,
    #[serde(default)]
    pub attention: Vec<String>,
}

impl DaySummaryOutput {
    /// The output within the bounds a summary is stored in: trimmed, without
    /// empty points, and no longer than a reader wants.
    pub fn bounded(self) -> Self {
        let clip = |text: &str, max: usize| text.trim().chars().take(max).collect::<String>();
        let points = |list: Vec<String>| -> Vec<String> {
            list.iter()
                .map(|p| clip(p, POINT_MAX))
                .filter(|p| !p.is_empty())
                .take(POINTS_MAX)
                .collect()
        };
        DaySummaryOutput {
            headline: clip(&self.headline, HEADLINE_MAX),
            highlights: points(self.highlights),
            attention: points(self.attention),
        }
    }
}

fn counted(counts: BTreeMap<&str, usize>) -> String {
    let mut pairs: Vec<(&str, usize)> = counts.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    pairs
        .iter()
        .map(|(name, n)| format!("{name} {n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A day's entries as a model reads them: how much happened and by whom,
/// what surrounded the day without being an event (`around`: deadlines,
/// cycle boundaries, spending), then the entries oldest first, one per line. `entries` are newest first,
/// as the timeline returns them, and `total` is how many the day has, which
/// may be more. A day with more than [`DIGEST_LINES`] entries keeps its
/// beginning and its end and says how many lie between, so the prompt stays
/// the same size however busy the day was.
pub fn day_digest(
    day: NaiveDate,
    total: i64,
    entries: &[TimelineEntry],
    around: &[String],
) -> String {
    let mut kinds = BTreeMap::new();
    let mut people = BTreeMap::new();
    for entry in entries {
        *kinds.entry(entry.kind.as_str()).or_insert(0) += 1;
        *people
            .entry(entry.actor.as_deref().unwrap_or("the system"))
            .or_insert(0) += 1;
    }
    let line = |entry: &TimelineEntry| -> String {
        let mut text = format!(
            "{} · {} · {} · {}",
            entry.at.format("%H:%M"),
            entry.actor.as_deref().unwrap_or("the system"),
            entry.kind,
            entry.title.trim()
        );
        let detail = entry
            .detail
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if !detail.is_empty() {
            text.push_str(" · ");
            text.push_str(&detail);
        }
        text.chars().take(DIGEST_LINE_CHARS).collect()
    };
    let oldest_first: Vec<&TimelineEntry> = entries.iter().rev().collect();
    let mut out = format!(
        "Day: {day} (UTC)\nEntries: {total}\nBy kind: {}\nBy person: {}\n",
        counted(kinds),
        counted(people)
    );
    if !around.is_empty() {
        out.push_str("\n## Around the day (not events: deadlines, cycles and spending)\n");
        for line in around {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.push_str("\n## Entries, oldest first (time · who · what · subject · detail)\n");
    let shown = oldest_first.len();
    let left_out = usize::try_from(total)
        .unwrap_or(shown)
        .saturating_sub(shown);
    if shown <= DIGEST_LINES && left_out == 0 {
        for entry in &oldest_first {
            out.push_str(&line(entry));
            out.push('\n');
        }
        return out;
    }
    let half = DIGEST_LINES / 2;
    let (head, tail) = if shown > DIGEST_LINES {
        (&oldest_first[..half], &oldest_first[shown - half..])
    } else {
        (&oldest_first[..], &oldest_first[shown..])
    };
    for entry in head {
        out.push_str(&line(entry));
        out.push('\n');
    }
    let between = shown - head.len() - tail.len() + left_out;
    out.push_str(&format!(
        "… {between} more entries are not listed; the counts above include them …\n"
    ));
    for entry in tail {
        out.push_str(&line(entry));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn entry(minute: u32, actor: Option<&str>, kind: &str, title: &str) -> TimelineEntry {
        TimelineEntry {
            at: Utc.with_ymd_and_hms(2026, 10, 6, 9, minute, 0).unwrap(),
            kind: kind.into(),
            actor: actor.map(str::to_owned),
            entity_type: "issue".into(),
            entity_id: None,
            title: title.into(),
            detail: "todo  ->\n in progress".into(),
        }
    }

    #[test]
    fn a_digest_counts_then_lists_oldest_first() {
        let day = NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
        // Newest first, as the timeline returns them.
        let entries = [
            entry(30, None, "run_failed", "Launch graph"),
            entry(20, Some("Bob"), "issue_state", "ENG-2 Fix login"),
            entry(10, Some("Bob"), "issue_created", "ENG-2 Fix login"),
        ];
        let digest = day_digest(day, 3, &entries, &[]);
        assert!(digest.starts_with("Day: 2026-10-06 (UTC)\nEntries: 3\n"));
        assert!(digest.contains("By kind: issue_created 1, issue_state 1, run_failed 1\n"));
        assert!(digest.contains("By person: Bob 2, the system 1\n"));
        let lines: Vec<&str> = digest
            .lines()
            .skip_while(|l| !l.starts_with("##"))
            .collect();
        assert_eq!(
            lines[1],
            "09:10 · Bob · issue_created · ENG-2 Fix login · todo -> in progress"
        );
        assert!(lines[3].starts_with("09:30 · the system · run_failed · Launch graph"));
        assert!(!digest.contains("not listed") && !digest.contains("Around the day"));
        let around = ["Due this day: ENG-2 Fix login (Todo)".to_owned()];
        let digest = day_digest(day, 3, &entries, &around);
        let at = |needle: &str| digest.find(needle).unwrap();
        assert!(
            at("## Around the day") < at("Due this day: ENG-2")
                && at("ENG-2 Fix login (Todo)") < at("## Entries")
        );
    }

    #[test]
    fn a_busy_day_keeps_its_beginning_and_end() {
        let day = NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
        let entries: Vec<TimelineEntry> = (0..DIGEST_LINES + 60)
            .rev()
            .map(|i| {
                entry(
                    (i % 60) as u32,
                    Some("Bob"),
                    "issue_comment",
                    &format!("n{i}"),
                )
            })
            .collect();
        // The day has more entries than the timeline returned.
        let digest = day_digest(day, entries.len() as i64 + 40, &entries, &[]);
        let listed = digest
            .lines()
            .filter(|l| l.contains("· issue_comment ·"))
            .count();
        assert_eq!(listed, DIGEST_LINES);
        assert!(digest.contains("… 100 more entries are not listed"));
        assert!(
            digest.contains("· n0 ·") && digest.contains(&format!("· n{} ·", DIGEST_LINES + 59))
        );
        assert!(
            digest
                .lines()
                .all(|l| l.chars().count() <= DIGEST_LINE_CHARS)
        );
    }

    #[test]
    fn a_summary_is_kept_within_bounds() {
        let out = DaySummaryOutput {
            headline: format!("  {}  ", "h".repeat(HEADLINE_MAX + 50)),
            highlights: (0..POINTS_MAX + 3)
                .map(|i| format!(" point {i} "))
                .collect(),
            attention: vec!["  ".into(), "A run failed.".into()],
        }
        .bounded();
        assert_eq!(out.headline.chars().count(), HEADLINE_MAX);
        assert_eq!(out.highlights.len(), POINTS_MAX);
        assert_eq!(out.highlights[0], "point 0");
        assert_eq!(out.attention, ["A run failed."]);
    }
}
