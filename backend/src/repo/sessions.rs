//! What the session tree is built from (`domain::session`): a run with its
//! graph, its node results with their titles, and the list of a
//! workspace's runs with the rollups the list shows. Read-only.

use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::session::{NodeFacts, Rollup, RunFacts, SessionAgentRef, SessionSummary};

/// The run and its graph, for the tree's header.
pub async fn run_facts(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
) -> Result<Option<RunFacts>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT r.id, r.graph_id, g.name AS graph_name, g.workspace_id, r.status, r.created_at,
                r.started_at, r.finished_at
         FROM runs r JOIN graphs g ON g.id = r.graph_id WHERE r.id = $1",
    )
    .bind(run_id)
    .fetch_optional(db)
    .await?;
    row.map(|row| {
        Ok(RunFacts {
            id: row.try_get("id")?,
            graph_id: row.try_get("graph_id")?,
            graph_name: row.try_get("graph_name")?,
            workspace_id: row.try_get("workspace_id")?,
            status: enum_col(&row, "status")?,
            created_at: row.try_get("created_at")?,
            started_at: row.try_get("started_at")?,
            finished_at: row.try_get("finished_at")?,
        })
    })
    .transpose()
}

/// The node results of a run with their titles, in submission order.
pub async fn node_facts(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
) -> Result<Vec<NodeFacts>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT nr.node_id, COALESCE(n.title, '') AS title, nr.executor, nr.status, nr.attempt,
                nr.cached, nr.error, nr.started_at, nr.finished_at, nr.tokens_in, nr.tokens_out,
                nr.cost_usd
         FROM node_runs nr LEFT JOIN nodes n ON n.id = nr.node_id
         WHERE nr.run_id = $1 ORDER BY nr.position",
    )
    .bind(run_id)
    .fetch_all(db)
    .await?;
    rows.iter()
        .map(|row: &PgRow| {
            Ok(NodeFacts {
                node_id: row.try_get("node_id")?,
                title: row.try_get("title")?,
                executor: enum_col(row, "executor")?,
                status: enum_col(row, "status")?,
                attempt: row.try_get("attempt")?,
                cached: row.try_get("cached")?,
                error: row.try_get("error")?,
                started_at: row.try_get("started_at")?,
                finished_at: row.try_get("finished_at")?,
                tokens_in: row.try_get("tokens_in")?,
                tokens_out: row.try_get("tokens_out")?,
                cost_usd: row.try_get("cost_usd")?,
            })
        })
        .collect()
}

/// What the session list narrows by. `None` leaves a dimension open.
#[derive(Debug, Clone, Default)]
pub struct SessionFilter<'a> {
    /// Only runs an agent worked in.
    pub agent_id: Option<Uuid>,
    /// Only runs in this state (a `RunStatus`).
    pub status: Option<&'a str>,
    /// Only runs created at or after this time.
    pub from: Option<DateTime<Utc>>,
    /// Only runs created before this time.
    pub to: Option<DateTime<Utc>>,
    /// Only runs whose id is below this one (the `next` of the previous page).
    pub before: Option<Uuid>,
}

/// Up to `limit` runs of the workspace matching `filter`, newest first (by
/// id: UUID v7), across the graphs `user` may open, each with its rollups.
pub async fn list(
    db: impl PgExecutor<'_>,
    user: Uuid,
    workspace_id: Uuid,
    filter: &SessionFilter<'_>,
    limit: i64,
) -> Result<Vec<SessionSummary>, sqlx::Error> {
    let rows = sqlx::query(concat!(
        "SELECT r.id, r.graph_id, g.name AS graph_name, r.status, r.created_at, r.started_at,
                r.finished_at, r.tokens_in, r.tokens_out, r.cost_usd,
                (SELECT count(*) FROM node_runs nr WHERE nr.run_id = r.id) AS nodes,
                s.spans, s.llm_calls, s.tool_calls, s.denials, s.span_tokens_in, s.span_tokens_out,
                s.tokens_cached, a.agents
         FROM runs r JOIN graphs g ON g.id = r.graph_id
         LEFT JOIN LATERAL (
            SELECT count(*) AS spans,
                   count(*) FILTER (WHERE s.kind = 'llm_call') AS llm_calls,
                   count(*) FILTER (WHERE s.kind = 'tool_call') AS tool_calls,
                   count(*) FILTER (WHERE s.kind = 'policy' AND s.attributes->>'decision' = 'deny') AS denials,
                   COALESCE(sum(CASE WHEN s.kind = 'llm_call' AND jsonb_typeof(s.attributes->'tokens_in') = 'number'
                                     THEN (s.attributes->>'tokens_in')::bigint ELSE 0 END), 0)::bigint AS span_tokens_in,
                   COALESCE(sum(CASE WHEN s.kind = 'llm_call' AND jsonb_typeof(s.attributes->'tokens_out') = 'number'
                                     THEN (s.attributes->>'tokens_out')::bigint ELSE 0 END), 0)::bigint AS span_tokens_out,
                   COALESCE(sum(CASE WHEN s.kind = 'llm_call' AND jsonb_typeof(s.attributes->'tokens_cached') = 'number'
                                     THEN (s.attributes->>'tokens_cached')::bigint ELSE 0 END), 0)::bigint AS tokens_cached
            FROM agent_spans s WHERE s.run_id = r.id) s ON true
         LEFT JOIN LATERAL (
            SELECT COALESCE(jsonb_agg(DISTINCT jsonb_build_object('id', ag.id, 'name', ag.name)), '[]'::jsonb) AS agents
            FROM node_runs nr JOIN agents ag ON ag.id = nr.agent_id WHERE nr.run_id = r.id) a ON true
         WHERE g.workspace_id = $2 AND ",
        graph_access!("g", "$1"),
        "  AND ($3::uuid IS NULL OR EXISTS (SELECT 1 FROM node_runs nr WHERE nr.run_id = r.id AND nr.agent_id = $3))
           AND ($4::text IS NULL OR r.status = $4)
           AND ($5::timestamptz IS NULL OR r.created_at >= $5)
           AND ($6::timestamptz IS NULL OR r.created_at < $6)
           AND ($7::uuid IS NULL OR r.id < $7)
         ORDER BY r.id DESC LIMIT $8"
    ))
    .bind(user)
    .bind(workspace_id)
    .bind(filter.agent_id)
    .bind(filter.status)
    .bind(filter.from)
    .bind(filter.to)
    .bind(filter.before)
    .bind(limit)
    .fetch_all(db)
    .await?;
    rows.iter()
        .map(|row| {
            let started_at: Option<DateTime<Utc>> = row.try_get("started_at")?;
            let finished_at: Option<DateTime<Utc>> = row.try_get("finished_at")?;
            let tokens_in: i64 = row.try_get("tokens_in")?;
            let tokens_out: i64 = row.try_get("tokens_out")?;
            let span_tokens_in: i64 = row.try_get("span_tokens_in")?;
            let span_tokens_out: i64 = row.try_get("span_tokens_out")?;
            let agents: serde_json::Value = row.try_get("agents")?;
            let agents: Vec<SessionAgentRef> =
                serde_json::from_value::<Vec<serde_json::Value>>(agents)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|a| {
                        Some(SessionAgentRef {
                            id: a.get("id")?.as_str()?.parse().ok()?,
                            name: a.get("name")?.as_str()?.to_owned(),
                        })
                    })
                    .collect();
            Ok(SessionSummary {
                run_id: row.try_get("id")?,
                graph_id: row.try_get("graph_id")?,
                graph_name: row.try_get("graph_name")?,
                status: enum_col(row, "status")?,
                created_at: row.try_get("created_at")?,
                started_at,
                finished_at,
                duration_ms: match (started_at, finished_at) {
                    (Some(s), Some(f)) => Some((f - s).num_milliseconds().max(0)),
                    _ => None,
                },
                nodes: row.try_get("nodes")?,
                agents,
                rollup: Rollup {
                    spans: row.try_get("spans")?,
                    llm_calls: row.try_get("llm_calls")?,
                    tool_calls: row.try_get("tool_calls")?,
                    denials: row.try_get("denials")?,
                    tokens_in: tokens_in.max(span_tokens_in),
                    tokens_out: tokens_out.max(span_tokens_out),
                    tokens_cached: row.try_get("tokens_cached")?,
                    cost_usd: row.try_get("cost_usd")?,
                },
            })
        })
        .collect()
}
