//! The trace spine's rows (`agent_spans`): appended in batches, read by run
//! or by agent, never updated.

use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::trace::Span;

fn digest(row: &PgRow, col: &str) -> Result<[u8; 32], sqlx::Error> {
    let bytes: Vec<u8> = row.try_get(col)?;
    bytes.try_into().map_err(|_| sqlx::Error::ColumnDecode {
        index: col.to_owned(),
        source: "a digest is 32 bytes".into(),
    })
}

impl FromRow<'_, PgRow> for Span {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(Span {
            id: row.try_get("id")?,
            run_id: row.try_get("run_id")?,
            node_id: row.try_get("node_id")?,
            agent_id: row.try_get("agent_id")?,
            principal: row.try_get("principal")?,
            parent_id: row.try_get("parent_id")?,
            kind: enum_col(row, "kind")?,
            name: row.try_get("name")?,
            started_at: row.try_get("started_at")?,
            ended_at: row.try_get("ended_at")?,
            status: enum_col(row, "status")?,
            attributes: row.try_get("attributes")?,
            seq: row.try_get("seq")?,
            prev_hash: digest(row, "prev_hash")?,
            hash: digest(row, "hash")?,
        })
    }
}

/// The columns of a span, optionally prefixed by a table alias (SQL stays static).
macro_rules! span_columns {
    () => {
        "id, run_id, node_id, agent_id, principal, parent_id, kind, name, started_at, ended_at,
         status, attributes, seq, prev_hash, hash"
    };
    (s) => {
        "s.id, s.run_id, s.node_id, s.agent_id, s.principal, s.parent_id, s.kind, s.name,
         s.started_at, s.ended_at, s.status, s.attributes, s.seq, s.prev_hash, s.hash"
    };
}

/// Appends `spans` in one statement.
pub async fn insert(db: impl PgExecutor<'_>, spans: &[Span]) -> Result<(), sqlx::Error> {
    if spans.is_empty() {
        return Ok(());
    }
    let ids: Vec<Uuid> = spans.iter().map(|s| s.id).collect();
    let run_ids: Vec<Uuid> = spans.iter().map(|s| s.run_id).collect();
    let node_ids: Vec<Uuid> = spans.iter().map(|s| s.node_id).collect();
    let agent_ids: Vec<Option<Uuid>> = spans.iter().map(|s| s.agent_id).collect();
    let principals: Vec<&str> = spans.iter().map(|s| s.principal.as_str()).collect();
    let parent_ids: Vec<Option<Uuid>> = spans.iter().map(|s| s.parent_id).collect();
    let kinds: Vec<&str> = spans.iter().map(|s| s.kind.as_str()).collect();
    let names: Vec<&str> = spans.iter().map(|s| s.name.as_str()).collect();
    let started: Vec<DateTime<Utc>> = spans.iter().map(|s| s.started_at).collect();
    let ended: Vec<Option<DateTime<Utc>>> = spans.iter().map(|s| s.ended_at).collect();
    let statuses: Vec<&str> = spans.iter().map(|s| s.status.as_str()).collect();
    let attributes: Vec<serde_json::Value> = spans.iter().map(|s| s.attributes.clone()).collect();
    let seqs: Vec<i32> = spans.iter().map(|s| s.seq).collect();
    let prev_hashes: Vec<Vec<u8>> = spans.iter().map(|s| s.prev_hash.to_vec()).collect();
    let hashes: Vec<Vec<u8>> = spans.iter().map(|s| s.hash.to_vec()).collect();
    sqlx::query(
        "INSERT INTO agent_spans (id, run_id, node_id, agent_id, principal, parent_id, kind, name,
                                  started_at, ended_at, status, attributes, seq, prev_hash, hash)
         SELECT * FROM unnest($1::uuid[], $2::uuid[], $3::uuid[], $4::uuid[], $5::text[], $6::uuid[],
                              $7::text[], $8::text[], $9::timestamptz[], $10::timestamptz[], $11::text[],
                              $12::jsonb[], $13::int[], $14::bytea[], $15::bytea[])
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(&ids)
    .bind(&run_ids)
    .bind(&node_ids)
    .bind(&agent_ids)
    .bind(&principals)
    .bind(&parent_ids)
    .bind(&kinds)
    .bind(&names)
    .bind(&started)
    .bind(&ended)
    .bind(&statuses)
    .bind(&attributes)
    .bind(&seqs)
    .bind(&prev_hashes)
    .bind(&hashes)
    .execute(db)
    .await?;
    Ok(())
}

/// Up to `limit` spans of a run in id order, after `after` if given.
pub async fn page(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
    after: Option<Uuid>,
    limit: i64,
) -> Result<Vec<Span>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT ",
        span_columns!(),
        " FROM agent_spans
         WHERE run_id = $1 AND ($2::uuid IS NULL OR id > $2)
         ORDER BY id LIMIT $3"
    ))
    .bind(run_id)
    .bind(after)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// One span of a run, by id.
pub async fn find(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
    id: Uuid,
) -> Result<Option<Span>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT ",
        span_columns!(),
        " FROM agent_spans WHERE run_id = $1 AND id = $2"
    ))
    .bind(run_id)
    .bind(id)
    .fetch_optional(db)
    .await
}

/// Every span of a run in chain order.
pub async fn chain(db: impl PgExecutor<'_>, run_id: Uuid) -> Result<Vec<Span>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT ",
        span_columns!(),
        " FROM agent_spans WHERE run_id = $1 ORDER BY seq"
    ))
    .bind(run_id)
    .fetch_all(db)
    .await
}

/// The last `limit` spans of an agent across runs, newest first, of runs
/// whose graph `user` may work on.
pub async fn activity(
    db: impl PgExecutor<'_>,
    user: Uuid,
    agent_id: Uuid,
    limit: i64,
) -> Result<Vec<Span>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT ",
        span_columns!(s),
        " FROM agent_spans s
         JOIN runs r ON r.id = s.run_id JOIN graphs g ON g.id = r.graph_id
         WHERE s.agent_id = $1 AND ",
        graph_access!("g", "$2"),
        " ORDER BY s.started_at DESC, s.id DESC LIMIT $3"
    ))
    .bind(agent_id)
    .bind(user)
    .bind(limit)
    .fetch_all(db)
    .await
}
