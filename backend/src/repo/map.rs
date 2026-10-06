//! Following the ties of one thing on the workspace map: who is on a team,
//! what a member filed, which graph plans an issue.
//!
//! Every statement is put together from the fixed fragments below and takes
//! the workspace as `$1`. Things at the far end of a tie are read through
//! [`rows`], which only yields rows of that workspace, so a tie can never
//! lead out of it.

use sqlx::postgres::PgRow;
use sqlx::{AssertSqlSafe, PgPool, Row};
use uuid::Uuid;

use crate::domain::insight::{MapItem, MapKind, MapNeighbourhood, MapTie};

/// How many things at the end of one tie are returned with it.
pub const TIE_ITEMS: i64 = 12;

/// The things of one kind in workspace `$1`, as `(id, title, subtitle, sort)`.
fn rows(kind: MapKind) -> &'static str {
    match kind {
        MapKind::Member => {
            "SELECT u.id, u.name AS title, u.email AS subtitle, lower(u.name) AS sort
             FROM users u JOIN workspace_members wm ON wm.user_id = u.id
             WHERE wm.workspace_id = $1"
        }
        MapKind::Team => {
            "SELECT t.id, t.name AS title, t.key AS subtitle, lower(t.name) AS sort
             FROM teams t WHERE t.workspace_id = $1"
        }
        MapKind::Project => {
            "SELECT p.id, p.name AS title, p.status AS subtitle, lower(p.name) AS sort
             FROM projects p WHERE p.workspace_id = $1"
        }
        MapKind::Issue => {
            "SELECT i.id, t.key || '-' || i.number || ' ' || i.title AS title, s.name AS subtitle,
                    t.key || lpad(i.number::text, 9, '0') AS sort
             FROM issues i JOIN teams t ON t.id = i.team_id
             JOIN issue_states s ON s.id = i.state_id
             WHERE i.workspace_id = $1"
        }
        MapKind::Graph => {
            "SELECT g.id, g.name AS title, left(COALESCE(g.goal, ''), 120) AS subtitle,
                    lower(g.name) AS sort
             FROM graphs g WHERE g.workspace_id = $1"
        }
        MapKind::Document => {
            "SELECT d.id, d.name AS title, d.status AS subtitle, lower(d.name) AS sort
             FROM documents d WHERE d.workspace_id = $1"
        }
        MapKind::Agent => {
            "SELECT a.id, a.name AS title, a.role AS subtitle, lower(a.name) AS sort
             FROM agents a WHERE a.workspace_id = $1"
        }
    }
}

/// What is at the far end of a tie of the thing `$2`.
enum Reach {
    /// Things of a kind whose own ties can be followed; the statement gives their ids.
    Things(MapKind, &'static str),
    /// Things that are only named; the statement gives `(id, title, subtitle, sort)`.
    Named(&'static str, &'static str),
    /// Things that are only counted; the statement gives the count.
    Counted(&'static str, &'static str),
}

struct Tie {
    label: &'static str,
    reach: Reach,
}

const fn tie(label: &'static str, reach: Reach) -> Tie {
    Tie { label, reach }
}

/// The ties of each kind, in the order they are shown.
fn ties(kind: MapKind) -> &'static [Tie] {
    use MapKind::{Agent, Document, Graph, Issue, Member, Project, Team};
    use Reach::{Counted, Named, Things};
    match kind {
        Member => {
            const TIES: &[Tie] = &[
                tie(
                    "is on",
                    Things(Team, "SELECT team_id FROM team_members WHERE user_id = $2"),
                ),
                tie(
                    "is assigned",
                    Things(Issue, "SELECT id FROM issues WHERE assignee_id = $2"),
                ),
                tie(
                    "filed",
                    Things(Issue, "SELECT id FROM issues WHERE creator_id = $2"),
                ),
                tie(
                    "leads",
                    Things(Project, "SELECT id FROM projects WHERE lead_id = $2"),
                ),
                tie(
                    "created",
                    Things(Graph, "SELECT id FROM graphs WHERE owner_id = $2"),
                ),
                tie(
                    "uploaded",
                    Things(Document, "SELECT id FROM documents WHERE uploaded_by = $2"),
                ),
                tie(
                    "set up",
                    Things(Agent, "SELECT id FROM agents WHERE owner_id = $2"),
                ),
                tie(
                    "started",
                    Counted(
                        "run",
                        "SELECT count(*) FROM runs r JOIN graphs g ON g.id = r.graph_id
                         WHERE g.workspace_id = $1 AND r.owner_id = $2",
                    ),
                ),
            ];
            TIES
        }
        Team => {
            const TIES: &[Tie] = &[
                tie(
                    "has",
                    Things(
                        Member,
                        "SELECT user_id FROM team_members WHERE team_id = $2",
                    ),
                ),
                tie(
                    "tracks",
                    Things(Issue, "SELECT id FROM issues WHERE team_id = $2"),
                ),
                tie(
                    "owns",
                    Things(Graph, "SELECT id FROM graphs WHERE team_id = $2"),
                ),
                tie(
                    "plans in",
                    Named(
                        "cycle",
                        concat!(
                            "SELECT c.id, ",
                            "COALESCE(NULLIF(c.name, ''), 'Cycle ' || c.number) AS title, ",
                            "c.starts_on || ' to ' || c.ends_on AS subtitle, ",
                            "lpad(c.number::text, 9, '0') AS sort ",
                            "FROM cycles c JOIN teams t ON t.id = c.team_id ",
                            "WHERE t.workspace_id = $1 AND c.team_id = $2"
                        ),
                    ),
                ),
            ];
            TIES
        }
        Project => {
            const TIES: &[Tie] = &[
                tie(
                    "holds",
                    Things(Issue, "SELECT id FROM issues WHERE project_id = $2"),
                ),
                tie(
                    "is led by",
                    Things(Member, "SELECT lead_id FROM projects WHERE id = $2"),
                ),
                tie(
                    "involves",
                    Things(Team, "SELECT team_id FROM issues WHERE project_id = $2"),
                ),
            ];
            TIES
        }
        Issue => {
            const TIES: &[Tie] = &[
                tie(
                    "belongs to",
                    Things(Team, "SELECT team_id FROM issues WHERE id = $2"),
                ),
                tie(
                    "is part of",
                    Things(Project, "SELECT project_id FROM issues WHERE id = $2"),
                ),
                tie(
                    "is assigned to",
                    Things(Member, "SELECT assignee_id FROM issues WHERE id = $2"),
                ),
                tie(
                    "was filed by",
                    Things(Member, "SELECT creator_id FROM issues WHERE id = $2"),
                ),
                tie(
                    "is worked on by",
                    Things(Agent, "SELECT agent_id FROM issues WHERE id = $2"),
                ),
                tie(
                    "is under",
                    Things(Issue, "SELECT parent_id FROM issues WHERE id = $2"),
                ),
                tie(
                    "is split into",
                    Things(Issue, "SELECT id FROM issues WHERE parent_id = $2"),
                ),
                tie(
                    "is planned as",
                    Things(Graph, "SELECT graph_id FROM issues WHERE id = $2"),
                ),
                tie(
                    "is labelled",
                    Named(
                        "label",
                        "SELECT l.id, l.name AS title, '' AS subtitle, lower(l.name) AS sort
                         FROM labels l JOIN issue_labels il ON il.label_id = l.id
                         WHERE l.workspace_id = $1 AND il.issue_id = $2",
                    ),
                ),
                tie(
                    "is scheduled in",
                    Named(
                        "cycle",
                        concat!(
                            "SELECT c.id, ",
                            "COALESCE(NULLIF(c.name, ''), 'Cycle ' || c.number) AS title, ",
                            "c.starts_on || ' to ' || c.ends_on AS subtitle, ",
                            "lpad(c.number::text, 9, '0') AS sort ",
                            "FROM cycles c JOIN issues i ON i.cycle_id = c.id ",
                            "WHERE i.workspace_id = $1 AND i.id = $2"
                        ),
                    ),
                ),
            ];
            TIES
        }
        Graph => {
            const TIES: &[Tie] = &[
                tie(
                    "belongs to",
                    Things(Team, "SELECT team_id FROM graphs WHERE id = $2"),
                ),
                tie(
                    "plans",
                    Things(Issue, "SELECT id FROM issues WHERE graph_id = $2"),
                ),
                tie(
                    "was created by",
                    Things(Member, "SELECT owner_id FROM graphs WHERE id = $2"),
                ),
                tie(
                    "has",
                    Counted(
                        "node",
                        "SELECT count(*) FROM nodes n JOIN graphs g ON g.id = n.graph_id
                         WHERE g.workspace_id = $1 AND n.graph_id = $2",
                    ),
                ),
                tie(
                    "was run",
                    Counted(
                        "run",
                        "SELECT count(*) FROM runs r JOIN graphs g ON g.id = r.graph_id
                         WHERE g.workspace_id = $1 AND r.graph_id = $2",
                    ),
                ),
                tie(
                    "taught",
                    Counted(
                        "memory",
                        "SELECT count(*) FROM memories WHERE workspace_id = $1 AND graph_id = $2",
                    ),
                ),
            ];
            TIES
        }
        Document => {
            const TIES: &[Tie] = &[
                tie(
                    "was uploaded by",
                    Things(Member, "SELECT uploaded_by FROM documents WHERE id = $2"),
                ),
                tie(
                    "is about",
                    Named(
                        "topic",
                        "SELECT k.id, k.label AS title, count(*) || ' passages' AS subtitle,
                                lpad((1000000000 - count(*))::text, 10, '0') AS sort
                         FROM document_chunks c JOIN knowledge_topics k ON k.id = c.topic_id
                         WHERE c.workspace_id = $1 AND c.document_id = $2
                         GROUP BY k.id, k.label",
                    ),
                ),
                tie(
                    "has",
                    Counted(
                        "passage",
                        "SELECT count(*) FROM document_chunks
                         WHERE workspace_id = $1 AND document_id = $2",
                    ),
                ),
            ];
            TIES
        }
        Agent => {
            const TIES: &[Tie] = &[
                tie(
                    "works on",
                    Things(Issue, "SELECT id FROM issues WHERE agent_id = $2"),
                ),
                tie(
                    "reports to",
                    Things(Agent, "SELECT reports_to FROM agents WHERE id = $2"),
                ),
                tie(
                    "manages",
                    Things(Agent, "SELECT id FROM agents WHERE reports_to = $2"),
                ),
                tie(
                    "was set up by",
                    Things(Member, "SELECT owner_id FROM agents WHERE id = $2"),
                ),
            ];
            TIES
        }
    }
}

fn item(kind: &str, row: &PgRow) -> Result<MapItem, sqlx::Error> {
    Ok(MapItem {
        kind: kind.to_owned(),
        id: row.try_get("id")?,
        title: row.try_get("title")?,
        subtitle: row.try_get("subtitle")?,
    })
}

/// The things of one kind in a workspace, by name, matched by `q`.
pub async fn list(
    db: &PgPool,
    workspace_id: Uuid,
    kind: MapKind,
    q: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<MapItem>, sqlx::Error> {
    let sql = format!(
        "SELECT x.id, x.title, x.subtitle FROM ({}) x
         WHERE $2::text IS NULL OR x.title ILIKE '%' || $2 || '%'
            OR x.subtitle ILIKE '%' || $2 || '%'
         ORDER BY x.sort, x.id LIMIT $3 OFFSET $4",
        rows(kind)
    );
    sqlx::query(AssertSqlSafe(sql))
        .bind(workspace_id)
        .bind(q)
        .bind(limit)
        .bind(offset)
        .fetch_all(db)
        .await?
        .iter()
        .map(|row| item(kind.as_str(), row))
        .collect()
}

/// One thing of a workspace with what it is tied to, or `None` when the
/// workspace has no such thing.
pub async fn neighbourhood(
    db: &PgPool,
    workspace_id: Uuid,
    kind: MapKind,
    id: Uuid,
) -> Result<Option<MapNeighbourhood>, sqlx::Error> {
    let sql = format!(
        "SELECT x.id, x.title, x.subtitle FROM ({}) x WHERE x.id = $2",
        rows(kind)
    );
    let Some(found) = sqlx::query(AssertSqlSafe(sql))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(db)
        .await?
    else {
        return Ok(None);
    };
    let mut reached = Vec::new();
    for tie in ties(kind) {
        let (end, statement) = match &tie.reach {
            Reach::Things(end, ids) => (
                end.as_str(),
                format!(
                    "SELECT x.id, x.title, x.subtitle, count(*) OVER () AS total
                     FROM ({}) x WHERE x.id IN ({ids}) ORDER BY x.sort, x.id LIMIT {TIE_ITEMS}",
                    rows(*end)
                ),
            ),
            Reach::Named(end, named) => (
                *end,
                format!(
                    "SELECT x.id, x.title, x.subtitle, count(*) OVER () AS total
                     FROM ({named}) x ORDER BY x.sort, x.id LIMIT {TIE_ITEMS}"
                ),
            ),
            Reach::Counted(end, counted) => {
                let count: i64 = sqlx::query_scalar(AssertSqlSafe((*counted).to_owned()))
                    .bind(workspace_id)
                    .bind(id)
                    .fetch_one(db)
                    .await?;
                if count > 0 {
                    reached.push(MapTie {
                        label: tie.label.to_owned(),
                        kind: (*end).to_owned(),
                        count,
                        items: Vec::new(),
                    });
                }
                continue;
            }
        };
        let found = sqlx::query(AssertSqlSafe(statement))
            .bind(workspace_id)
            .bind(id)
            .fetch_all(db)
            .await?;
        let Some(first) = found.first() else { continue };
        reached.push(MapTie {
            label: tie.label.to_owned(),
            kind: end.to_owned(),
            count: first.try_get("total")?,
            items: found
                .iter()
                .map(|row| item(end, row))
                .collect::<Result<_, _>>()?,
        });
    }
    Ok(Some(MapNeighbourhood {
        item: item(kind.as_str(), &found)?,
        ties: reached,
    }))
}
