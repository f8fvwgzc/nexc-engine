//! Read-only aggregates for the AgentSecOps console and the trace explorer's
//! search. Nothing here writes. Runs, spans and artifacts are counted only
//! across the graphs the caller may open (`graph_access!`).

use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::secops::{
    AgentCounts, EvidenceCounts, PolicyInForce, RiskPoint, RiskyAgent, RuleCount, RunCounts,
    SecopsSpan, SeverityCount, TraceSeal,
};

/// Most rules listed in a denial breakdown.
const RULES_MAX: i64 = 20;

/// Agents of the workspace by status.
pub async fn agent_counts(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<AgentCounts, sqlx::Error> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT status, count(*) FROM agents WHERE workspace_id = $1 GROUP BY status",
    )
    .bind(workspace_id)
    .fetch_all(db)
    .await?;
    let mut counts = AgentCounts::default();
    for (status, count) in rows {
        if let Ok(status) = status.parse() {
            counts.add(status, count);
        }
    }
    Ok(counts)
}

/// Runs created since `from`, by status, across the graphs `user` may open.
pub async fn run_counts(
    db: impl PgExecutor<'_>,
    user: Uuid,
    workspace_id: Uuid,
    from: DateTime<Utc>,
) -> Result<RunCounts, sqlx::Error> {
    let rows: Vec<(String, i64)> = sqlx::query_as(concat!(
        "SELECT r.status, count(*) FROM runs r JOIN graphs g ON g.id = r.graph_id
         WHERE g.workspace_id = $2 AND r.created_at >= $3 AND ",
        graph_access!("g", "$1"),
        " GROUP BY r.status"
    ))
    .bind(user)
    .bind(workspace_id)
    .bind(from)
    .fetch_all(db)
    .await?;
    let mut counts = RunCounts::default();
    for (status, count) in rows {
        counts.add(&status, count);
    }
    Ok(counts)
}

/// Spans started since `from`, across the graphs `user` may open.
pub async fn span_count(
    db: impl PgExecutor<'_>,
    user: Uuid,
    workspace_id: Uuid,
    from: DateTime<Utc>,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(concat!(
        "SELECT count(*) FROM agent_spans s
         JOIN runs r ON r.id = s.run_id JOIN graphs g ON g.id = r.graph_id
         WHERE g.workspace_id = $2 AND s.started_at >= $3 AND ",
        graph_access!("g", "$1")
    ))
    .bind(user)
    .bind(workspace_id)
    .bind(from)
    .fetch_one(db)
    .await
}

/// Policy denials since `from`, by the rule that denied, most frequent first.
pub async fn denials_by_rule(
    db: impl PgExecutor<'_>,
    user: Uuid,
    workspace_id: Uuid,
    from: DateTime<Utc>,
) -> Result<Vec<RuleCount>, sqlx::Error> {
    let rows: Vec<(String, i64)> = sqlx::query_as(concat!(
        "SELECT COALESCE(s.attributes->>'rule', '') AS rule, count(*) FROM agent_spans s
         JOIN runs r ON r.id = s.run_id JOIN graphs g ON g.id = r.graph_id
         WHERE g.workspace_id = $2 AND s.kind = 'policy' AND s.attributes->>'decision' = 'deny'
           AND s.started_at >= $3 AND ",
        graph_access!("g", "$1"),
        " GROUP BY 1 ORDER BY 2 DESC, 1 LIMIT $4"
    ))
    .bind(user)
    .bind(workspace_id)
    .bind(from)
    .bind(RULES_MAX)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(rule, count)| RuleCount { rule, count })
        .collect())
}

/// Outputs that answered since `from`, by evidence verdict, across the graphs `user` may open.
pub async fn evidence_counts(
    db: impl PgExecutor<'_>,
    user: Uuid,
    workspace_id: Uuid,
    from: DateTime<Utc>,
) -> Result<EvidenceCounts, sqlx::Error> {
    let rows: Vec<(String, i64)> = sqlx::query_as(concat!(
        "SELECT nr.evidence->>'verdict', count(*)
         FROM node_runs nr JOIN runs r ON r.id = nr.run_id JOIN graphs g ON g.id = r.graph_id
         WHERE g.workspace_id = $2 AND nr.evidence IS NOT NULL AND nr.finished_at >= $3 AND ",
        graph_access!("g", "$1"),
        " GROUP BY 1"
    ))
    .bind(user)
    .bind(workspace_id)
    .bind(from)
    .fetch_all(db)
    .await?;
    let mut counts = EvidenceCounts::default();
    for (verdict, count) in rows {
        if let Ok(verdict) = verdict.parse() {
            counts.add(verdict, count);
        }
    }
    Ok(counts)
}

/// Approvals nobody decided yet.
pub async fn approvals_open(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM approvals WHERE workspace_id = $1 AND status = 'pending'",
    )
    .bind(workspace_id)
    .fetch_one(db)
    .await
}

/// Unacknowledged alerts by severity, worst first.
pub async fn alerts_open(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<Vec<SeverityCount>, sqlx::Error> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT severity, count(*) FROM alerts
         WHERE workspace_id = $1 AND acknowledged_at IS NULL GROUP BY severity",
    )
    .bind(workspace_id)
    .fetch_all(db)
    .await?;
    let mut counts: Vec<SeverityCount> = rows
        .into_iter()
        .filter_map(|(severity, count)| {
            Some(SeverityCount {
                severity: severity.parse().ok()?,
                count,
            })
        })
        .collect();
    counts.sort_by_key(|c| std::cmp::Reverse(c.severity));
    Ok(counts)
}

/// Artifacts the DLP screen kept in and nobody released, across the graphs `user` may open.
pub async fn artifacts_quarantined(
    db: impl PgExecutor<'_>,
    user: Uuid,
    workspace_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(concat!(
        "SELECT count(*) FROM artifacts a
         JOIN runs r ON r.id = a.run_id JOIN graphs g ON g.id = r.graph_id
         WHERE g.workspace_id = $2 AND a.quarantined_at IS NOT NULL AND a.released_at IS NULL AND ",
        graph_access!("g", "$1")
    ))
    .bind(user)
    .bind(workspace_id)
    .fetch_one(db)
    .await
}

/// The `limit` agents scoring highest, highest first; only agents scored at least once.
pub async fn top_risk(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    limit: i64,
) -> Result<Vec<RiskyAgent>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, name, principal_id, status, risk_score, risk_reasons, risk_updated_at
         FROM agents WHERE workspace_id = $1 AND risk_updated_at IS NOT NULL
         ORDER BY risk_score DESC, risk_updated_at DESC, name LIMIT $2",
    )
    .bind(workspace_id)
    .bind(limit)
    .fetch_all(db)
    .await?;
    rows.iter()
        .map(|row: &PgRow| {
            Ok(RiskyAgent {
                agent_id: row.try_get("id")?,
                name: row.try_get("name")?,
                principal_id: row.try_get("principal_id")?,
                status: enum_col(row, "status")?,
                risk_score: row.try_get("risk_score")?,
                risk_reasons: serde_json::from_value(
                    row.try_get::<serde_json::Value, _>("risk_reasons")?,
                )
                .unwrap_or_default(),
                risk_updated_at: row.try_get("risk_updated_at")?,
            })
        })
        .collect()
}

/// The latest version at every level that has one, workspace first.
pub async fn policies_in_force(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<Vec<PolicyInForce>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT DISTINCT ON (scope, scope_id) scope, scope_id, version, created_at
         FROM policies WHERE workspace_id = $1 ORDER BY scope, scope_id, version DESC",
    )
    .bind(workspace_id)
    .fetch_all(db)
    .await?;
    let mut levels: Vec<PolicyInForce> = rows
        .iter()
        .map(|row| {
            Ok(PolicyInForce {
                scope: enum_col(row, "scope")?,
                scope_id: row.try_get("scope_id")?,
                version: row.try_get("version")?,
                created_at: row.try_get("created_at")?,
            })
        })
        .collect::<Result<_, sqlx::Error>>()?;
    levels.sort_by(|a, b| a.scope.cmp(&b.scope).then(a.scope_id.cmp(&b.scope_id)));
    Ok(levels)
}

/// The run sealed last among those `user` may open.
pub async fn last_seal(
    db: impl PgExecutor<'_>,
    user: Uuid,
    workspace_id: Uuid,
) -> Result<Option<TraceSeal>, sqlx::Error> {
    let row = sqlx::query(concat!(
        "SELECT r.id, r.graph_id, r.trace_hash, r.finished_at
         FROM runs r JOIN graphs g ON g.id = r.graph_id
         WHERE g.workspace_id = $2 AND r.trace_hash IS NOT NULL AND ",
        graph_access!("g", "$1"),
        " ORDER BY r.finished_at DESC NULLS LAST, r.id DESC LIMIT 1"
    ))
    .bind(user)
    .bind(workspace_id)
    .fetch_optional(db)
    .await?;
    row.map(|row| {
        let hash: Vec<u8> = row.try_get("trace_hash")?;
        Ok(TraceSeal {
            run_id: row.try_get("id")?,
            graph_id: row.try_get("graph_id")?,
            trace_hash: hex::encode(hash),
            sealed_at: row.try_get("finished_at")?,
        })
    })
    .transpose()
}

/// The highest score per agent per day since `from`, oldest day first.
pub async fn risk_history(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    from: DateTime<Utc>,
) -> Result<Vec<RiskPoint>, sqlx::Error> {
    let rows: Vec<(Uuid, String, f64)> = sqlx::query_as(
        "SELECT agent_id, to_char(date_trunc('day', scored_at), 'YYYY-MM-DD') AS day, max(score)
         FROM agent_risk_history WHERE workspace_id = $1 AND scored_at >= $2
         GROUP BY agent_id, day ORDER BY day, agent_id",
    )
    .bind(workspace_id)
    .bind(from)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(agent_id, day, score)| RiskPoint {
            agent_id,
            day,
            score,
        })
        .collect())
}

/// What the trace explorer narrows by. `None` leaves a dimension open.
#[derive(Debug, Clone, Default)]
pub struct SpanFilter<'a> {
    pub agent_id: Option<Uuid>,
    pub kind: Option<&'a str>,
    /// Only tool calls marked `inbound` (tasks received over A2A).
    pub inbound: bool,
    pub tool: Option<&'a str>,
    pub host: Option<&'a str>,
    pub run_id: Option<Uuid>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    /// A decision of a policy or approval span, or `evidence`: the spans that
    /// carry an evidence verdict.
    pub decision: Option<&'a str>,
    /// Only spans whose id is below this one (the `next` of the previous page).
    pub before: Option<Uuid>,
}

/// The pseudo-decision of the explorer: spans carrying an evidence verdict.
pub const EVIDENCE_DECISION: &str = "evidence";

fn text(attributes: &serde_json::Value, key: &str) -> Option<String> {
    attributes.get(key)?.as_str().map(str::to_owned)
}

fn number(attributes: &serde_json::Value, key: &str) -> Option<i64> {
    attributes.get(key)?.as_i64()
}

/// Up to `limit` spans matching `filter`, newest first (by id: UUID v7),
/// across the graphs `user` may open.
pub async fn search_spans(
    db: impl PgExecutor<'_>,
    user: Uuid,
    workspace_id: Uuid,
    filter: &SpanFilter<'_>,
    limit: i64,
) -> Result<Vec<SecopsSpan>, sqlx::Error> {
    let rows = sqlx::query(concat!(
        "SELECT s.id, s.run_id, g.id AS graph_id, s.node_id, s.agent_id, s.principal, s.kind, s.name,
                s.started_at, s.ended_at, s.status, s.attributes
         FROM agent_spans s JOIN runs r ON r.id = s.run_id JOIN graphs g ON g.id = r.graph_id
         WHERE g.workspace_id = $2 AND ",
        graph_access!("g", "$1"),
        "  AND ($3::uuid IS NULL OR s.agent_id = $3)
           AND ($4::text IS NULL OR s.kind = $4)
           AND ($5::text IS NULL OR s.attributes->>'tool' = $5 OR (s.kind = 'tool_call' AND s.name = $5))
           AND ($6::text IS NULL OR s.attributes->>'egress_host' = $6)
           AND ($7::uuid IS NULL OR s.run_id = $7)
           AND ($8::timestamptz IS NULL OR s.started_at >= $8)
           AND ($9::timestamptz IS NULL OR s.started_at < $9)
           AND ($10::text IS NULL OR s.attributes->>'decision' = $10
                OR ($10 = 'evidence' AND jsonb_exists(s.attributes, 'evidence')))
           AND ($11::uuid IS NULL OR s.id < $11)
           AND (NOT $13::bool OR s.attributes->>'inbound' = 'true')
         ORDER BY s.id DESC LIMIT $12"
    ))
    .bind(user)
    .bind(workspace_id)
    .bind(filter.agent_id)
    .bind(filter.kind)
    .bind(filter.tool)
    .bind(filter.host)
    .bind(filter.run_id)
    .bind(filter.from)
    .bind(filter.to)
    .bind(filter.decision)
    .bind(filter.before)
    .bind(limit)
    .bind(filter.inbound)
    .fetch_all(db)
    .await?;
    rows.iter()
        .map(|row| {
            let attributes: serde_json::Value = row.try_get("attributes")?;
            let kind: crate::domain::trace::SpanKind = enum_col(row, "kind")?;
            let name: String = row.try_get("name")?;
            let tool = text(&attributes, "tool").or_else(|| {
                (kind == crate::domain::trace::SpanKind::ToolCall).then(|| name.clone())
            });
            Ok(SecopsSpan {
                id: row.try_get("id")?,
                run_id: row.try_get("run_id")?,
                graph_id: row.try_get("graph_id")?,
                node_id: row.try_get("node_id")?,
                agent_id: row.try_get("agent_id")?,
                principal: row.try_get("principal")?,
                kind,
                name,
                started_at: row.try_get("started_at")?,
                ended_at: row.try_get("ended_at")?,
                status: enum_col(row, "status")?,
                tool,
                host: text(&attributes, "egress_host"),
                decision: text(&attributes, "decision"),
                rule: text(&attributes, "rule"),
                model: text(&attributes, "model"),
                tokens_in: number(&attributes, "tokens_in"),
                tokens_out: number(&attributes, "tokens_out"),
                inbound: attributes
                    .get("inbound")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true),
                evidence: text(&attributes, "evidence"),
            })
        })
        .collect()
}
