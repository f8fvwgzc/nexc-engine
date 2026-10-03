//! PostgreSQL repositories, one module per aggregate. All SQL is static and
//! every value is a bound parameter. Queries touching user data are scoped by
//! the owner's id so that other users' resources read as "not found".
#![forbid(unsafe_code)]

pub mod agents;
pub mod artifacts;
pub mod edges;
pub mod graphs;
pub mod memories;
pub mod nodes;
pub mod outbox;
pub mod plans;
pub mod runs;
pub mod settings;
pub mod tickets;
pub mod tokens;
pub mod users;

use std::str::FromStr;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgRow};
use sqlx::{PgPool, Row};

use crate::domain::AppError;

/// Embedded migrations from `backend/migrations`.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Opens a connection pool of `max_connections`.
pub async fn connect(url: &str, max_connections: u32) -> anyhow::Result<PgPool> {
    let options = PgConnectOptions::from_str(url)?.application_name("nexc");
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .idle_timeout(Duration::from_secs(600))
        .connect_with(options)
        .await?;
    Ok(pool)
}

/// Applies pending migrations.
pub async fn migrate(pool: &PgPool) -> anyhow::Result<()> {
    MIGRATOR.run(pool).await?;
    Ok(())
}

/// `server_version_num` of the connected server (e.g. 170002).
pub async fn server_version(pool: &PgPool) -> anyhow::Result<i32> {
    let v: String = sqlx::query_scalar("SHOW server_version_num")
        .fetch_one(pool)
        .await?;
    Ok(v.parse()?)
}

/// Reads a TEXT column holding a [`crate::domain`] enum spelling.
pub(crate) fn enum_col<T>(row: &PgRow, col: &str) -> Result<T, sqlx::Error>
where
    T: FromStr<Err = crate::domain::UnknownVariant>,
{
    let raw: String = row.try_get(col)?;
    raw.parse().map_err(|e| sqlx::Error::ColumnDecode {
        index: col.to_owned(),
        source: Box::new(e),
    })
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        match &err {
            sqlx::Error::RowNotFound => AppError::NotFound("resource"),
            sqlx::Error::Database(db) if db.is_unique_violation() => {
                AppError::Conflict("a resource with the same unique key already exists".into())
            }
            sqlx::Error::Database(db) if db.is_foreign_key_violation() => {
                AppError::NotFound("referenced resource")
            }
            _ => AppError::Internal(err.into()),
        }
    }
}

/// Maps `RowNotFound` to a typed 404 for `resource`.
pub(crate) trait OrNotFound<T> {
    fn or_not_found(self, resource: &'static str) -> Result<T, AppError>;
}

impl<T> OrNotFound<T> for Result<Option<T>, sqlx::Error> {
    fn or_not_found(self, resource: &'static str) -> Result<T, AppError> {
        self?.ok_or(AppError::NotFound(resource))
    }
}
