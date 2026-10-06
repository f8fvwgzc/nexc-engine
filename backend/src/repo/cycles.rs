//! Cycles of a team.

use chrono::{NaiveDate, Utc};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgConnection, PgExecutor, Row};
use uuid::Uuid;

use crate::domain::cycle::{Cycle, CycleStatus};

impl FromRow<'_, PgRow> for Cycle {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let starts_on: NaiveDate = row.try_get("starts_on")?;
        let ends_on: NaiveDate = row.try_get("ends_on")?;
        Ok(Cycle {
            id: row.try_get("id")?,
            team_id: row.try_get("team_id")?,
            number: row.try_get("number")?,
            name: row.try_get("name")?,
            starts_on,
            ends_on,
            status: CycleStatus::on(Utc::now().date_naive(), starts_on, ends_on),
            issue_count: row.try_get("issue_count")?,
            closed_count: row.try_get("closed_count")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

macro_rules! cycle_select {
    ($tail:literal) => {
        concat!(
            "SELECT c.id, c.team_id, c.number, c.name, c.starts_on, c.ends_on, c.created_at,
                    (SELECT count(*) FROM issues i WHERE i.cycle_id = c.id) AS issue_count,
                    (SELECT count(*) FROM issues i JOIN issue_states s ON s.id = i.state_id
                     WHERE i.cycle_id = c.id AND s.category IN ('completed', 'canceled'))
                        AS closed_count
             FROM cycles c WHERE ",
            $tail
        )
    };
}

/// The cycles of a team, latest first.
pub async fn list(db: impl PgExecutor<'_>, team_id: Uuid) -> Result<Vec<Cycle>, sqlx::Error> {
    sqlx::query_as(cycle_select!(
        "c.team_id = $1 ORDER BY c.starts_on DESC, c.number DESC"
    ))
    .bind(team_id)
    .fetch_all(db)
    .await
}

/// One cycle of a team.
pub async fn find(
    db: impl PgExecutor<'_>,
    team_id: Uuid,
    id: Uuid,
) -> Result<Option<Cycle>, sqlx::Error> {
    sqlx::query_as(cycle_select!("c.team_id = $1 AND c.id = $2"))
        .bind(team_id)
        .bind(id)
        .fetch_optional(db)
        .await
}

pub async fn count(db: impl PgExecutor<'_>, team_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM cycles WHERE team_id = $1")
        .bind(team_id)
        .fetch_one(db)
        .await
}

/// Whether another cycle of the team shares a day with the given dates.
pub async fn overlaps(
    db: impl PgExecutor<'_>,
    team_id: Uuid,
    except: Option<Uuid>,
    starts_on: NaiveDate,
    ends_on: NaiveDate,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM cycles
                        WHERE team_id = $1 AND ($2::uuid IS NULL OR id <> $2)
                          AND starts_on <= $4 AND ends_on >= $3)",
    )
    .bind(team_id)
    .bind(except)
    .bind(starts_on)
    .bind(ends_on)
    .fetch_one(db)
    .await
}

/// Adds a cycle with the team's next cycle number. Must run in a transaction
/// that holds the team row, so two cycles cannot take the same number.
pub async fn create(
    db: &mut PgConnection,
    team_id: Uuid,
    name: &str,
    starts_on: NaiveDate,
    ends_on: NaiveDate,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query("SELECT 1 FROM teams WHERE id = $1 FOR UPDATE")
        .bind(team_id)
        .execute(&mut *db)
        .await?;
    sqlx::query_scalar(
        "INSERT INTO cycles (id, team_id, number, name, starts_on, ends_on)
         VALUES ($1, $2, COALESCE((SELECT max(number) FROM cycles WHERE team_id = $2), 0) + 1,
                 $3, $4, $5)
         RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(team_id)
    .bind(name)
    .bind(starts_on)
    .bind(ends_on)
    .fetch_one(&mut *db)
    .await
}

pub async fn save(
    db: impl PgExecutor<'_>,
    id: Uuid,
    name: &str,
    starts_on: NaiveDate,
    ends_on: NaiveDate,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE cycles SET name = $2, starts_on = $3, ends_on = $4 WHERE id = $1")
        .bind(id)
        .bind(name)
        .bind(starts_on)
        .bind(ends_on)
        .execute(db)
        .await?;
    Ok(())
}

/// Deletes a cycle; its issues stay, in no cycle.
pub async fn delete(db: impl PgExecutor<'_>, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM cycles WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}
