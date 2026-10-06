//! The LLM usage ledger and its reports.

use chrono::NaiveDate;
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, PgPool, Row};
use uuid::Uuid;

use crate::domain::settings::ConfigScope;
use crate::domain::usage::{
    UsageEvent, UsageMember, UsageModel, UsagePurpose, UsageSlice, UsageTotals,
};

/// Appends one call to the ledger.
pub async fn insert(db: impl PgExecutor<'_>, e: &UsageEvent) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO llm_usage (id, workspace_id, user_id, graph_id, run_id, purpose, provider, model,
                                credential, tokens_in, tokens_out, cost_usd)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
    )
    .bind(Uuid::now_v7())
    .bind(e.workspace_id)
    .bind(e.user_id)
    .bind(e.graph_id)
    .bind(e.run_id)
    .bind(e.purpose.as_str())
    .bind(e.provider.as_str())
    .bind(&e.model)
    .bind(e.credential.as_str())
    .bind(e.tokens_in.max(0))
    .bind(e.tokens_out.max(0))
    .bind(e.cost_usd)
    .execute(db)
    .await?;
    Ok(())
}

/// Which rows a report covers: a workspace over the last `days` days,
/// optionally only one member's calls.
#[derive(Debug, Clone, Copy)]
pub struct Window {
    pub workspace_id: Uuid,
    pub days: i64,
    pub only_user: Option<Uuid>,
}

/// Every report query selects from this: `$1` workspace, `$2` days, `$3` optional user.
macro_rules! window {
    ($select:literal, $tail:literal) => {
        concat!(
            "SELECT ",
            $select,
            ", count(*) AS calls, COALESCE(sum(u.tokens_in), 0)::bigint AS tokens_in,
               COALESCE(sum(u.tokens_out), 0)::bigint AS tokens_out,
               COALESCE(sum(u.cost_usd), 0)::float8 AS cost_usd
             FROM llm_usage u
             WHERE u.workspace_id = $1 AND u.created_at >= now() - make_interval(days => $2::int)
               AND ($3::uuid IS NULL OR u.user_id = $3) ",
            $tail
        )
    };
}

fn totals(row: &PgRow) -> Result<UsageTotals, sqlx::Error> {
    UsageTotals::from_row(row)
}

async fn slices<K>(
    db: &PgPool,
    sql: &'static str,
    w: Window,
    key: impl Fn(&PgRow) -> Result<K, sqlx::Error>,
) -> Result<Vec<UsageSlice<K>>, sqlx::Error> {
    let rows = sqlx::query(sql)
        .bind(w.workspace_id)
        .bind(w.days)
        .bind(w.only_user)
        .fetch_all(db)
        .await?;
    rows.iter()
        .map(|row| {
            Ok(UsageSlice {
                key: key(row)?,
                totals: totals(row)?,
            })
        })
        .collect()
}

/// Sums over the whole window.
pub async fn total(db: &PgPool, w: Window) -> Result<UsageTotals, sqlx::Error> {
    let row = sqlx::query(window!("1 AS one", ""))
        .bind(w.workspace_id)
        .bind(w.days)
        .bind(w.only_user)
        .fetch_one(db)
        .await?;
    totals(&row)
}

/// Usage per UTC day, oldest first.
pub async fn by_day(db: &PgPool, w: Window) -> Result<Vec<UsageSlice<NaiveDate>>, sqlx::Error> {
    slices(
        db,
        window!(
            "(u.created_at AT TIME ZONE 'UTC')::date AS day",
            "GROUP BY day ORDER BY day"
        ),
        w,
        |row| row.try_get("day"),
    )
    .await
}

/// Usage per member, highest cost first.
pub async fn by_member(
    db: &PgPool,
    w: Window,
) -> Result<Vec<UsageSlice<UsageMember>>, sqlx::Error> {
    slices(
        db,
        window!(
            "u.user_id, COALESCE((SELECT name FROM users WHERE id = u.user_id), 'Deleted account') AS name",
            "GROUP BY u.user_id ORDER BY cost_usd DESC, tokens_out DESC, tokens_in DESC"
        ),
        w,
        |row| {
            Ok(UsageMember {
                user_id: row.try_get("user_id")?,
                name: row.try_get("name")?,
            })
        },
    )
    .await
}

/// Usage per provider and model, highest cost first.
pub async fn by_model(db: &PgPool, w: Window) -> Result<Vec<UsageSlice<UsageModel>>, sqlx::Error> {
    slices(
        db,
        window!(
            "u.provider, u.model",
            "GROUP BY u.provider, u.model ORDER BY cost_usd DESC, tokens_out DESC, tokens_in DESC"
        ),
        w,
        |row| {
            Ok(UsageModel {
                provider: row.try_get("provider")?,
                model: row.try_get("model")?,
            })
        },
    )
    .await
}

fn parsed<T: std::str::FromStr>(row: &PgRow, column: &str) -> Result<T, sqlx::Error>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    let raw: String = row.try_get(column)?;
    raw.parse().map_err(|e| sqlx::Error::ColumnDecode {
        index: column.to_owned(),
        source: Box::new(e),
    })
}

/// Usage per purpose (planning, node execution, memory extraction).
pub async fn by_purpose(
    db: &PgPool,
    w: Window,
) -> Result<Vec<UsageSlice<UsagePurpose>>, sqlx::Error> {
    slices(
        db,
        window!("u.purpose", "GROUP BY u.purpose ORDER BY u.purpose"),
        w,
        |row| parsed(row, "purpose"),
    )
    .await
}

/// Usage per paying account (members' own, the workspace's, the server's).
pub async fn by_credential(
    db: &PgPool,
    w: Window,
) -> Result<Vec<UsageSlice<ConfigScope>>, sqlx::Error> {
    slices(
        db,
        window!(
            "u.credential",
            "GROUP BY u.credential ORDER BY u.credential"
        ),
        w,
        |row| parsed(row, "credential"),
    )
    .await
}
