//! PostgreSQL repositories, one module per aggregate. All SQL is static and
//! every value is a bound parameter. Queries touching user data are scoped by
//! the owner's id so that other users' resources read as "not found".
#![forbid(unsafe_code)]

/// SQL predicate: user `$u` may work on the graph row aliased `$g`: it is not
/// deleted (archived graphs keep their runs' evidence, `graph_access_any!`),
/// and `$u` may open it.
macro_rules! graph_access {
    ($g:literal, $u:literal) => {
        concat!(
            "(",
            $g,
            ".deleted_at IS NULL AND ",
            graph_access_any!($g, $u),
            ")"
        )
    };
}

/// SQL predicate: user `$u` may open the graph row aliased `$g`, deleted or
/// not. Only the evidence of a deleted graph's runs is read this way: their
/// traces, their verification and their evidence bundles.
///
/// A graph of a team is open to that team's members; a graph that belongs to
/// the workspace as a whole is open to every member but guests. This is the
/// SQL form of `domain::workspace::TeamAccess::can_contribute` and of
/// `WorkspaceRole::is_member`; `tests/workspaces.rs` holds the two together.
/// Graphs that predate workspaces (no `workspace_id` yet) stay with their creator.
macro_rules! graph_access_any {
    ($g:literal, $u:literal) => {
        concat!(
            "((",
            $g,
            ".workspace_id IS NULL AND ",
            $g,
            ".owner_id = ",
            $u,
            ") OR EXISTS (",
            "SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = ",
            $g,
            ".workspace_id",
            " AND wm.user_id = ",
            $u,
            " AND CASE WHEN ",
            $g,
            ".team_id IS NULL",
            " THEN wm.role <> 'guest'",
            " ELSE EXISTS (SELECT 1 FROM team_members tm WHERE tm.team_id = ",
            $g,
            ".team_id",
            " AND tm.user_id = ",
            $u,
            ") END))"
        )
    };
}

/// SQL predicate: user `$u`, whose workspace membership row is aliased
/// `$wm`, may see the team row aliased `$t`. The SQL form of
/// `domain::workspace::TeamAccess::can_view`.
macro_rules! team_visible {
    ($t:literal, $wm:literal, $u:literal) => {
        concat!(
            "(",
            $wm,
            ".role IN ('owner', 'admin') OR (NOT ",
            $t,
            ".private AND ",
            $wm,
            ".role <> 'guest') OR EXISTS (SELECT 1 FROM team_members tv WHERE tv.team_id = ",
            $t,
            ".id AND tv.user_id = ",
            $u,
            "))"
        )
    };
}

pub mod a2a;
pub mod account_events;
pub mod agent_evaluations;
pub mod agent_tokens;
pub mod agents;
pub mod alert_deliveries;
pub mod alert_destinations;
pub mod alerts;
pub mod approvals;
pub mod artifact_files;
pub mod artifacts;
pub mod assistant;
pub mod audit;
pub mod baselines;
pub mod cycles;
pub mod edges;
pub mod evidence;
pub mod external_runs;
pub mod graphs;
pub mod idempotency;
pub mod insight;
pub mod issues;
pub mod knowledge;
pub mod knowledge_vectors;
pub mod mcp_servers;
pub mod memories;
pub mod nodes;
pub mod onto_export;
pub mod onto_migrations;
pub mod onto_projection;
pub mod otlp_export;
pub mod outbox;
pub mod plans;
pub mod platform;
pub mod policies;
pub mod resets;
pub mod run_commands;
pub mod run_leases;
pub mod runs;
pub mod secops;
pub mod sessions;
pub mod settings;
pub mod span_payloads;
pub mod spans;
pub mod sso;
pub mod teams;
pub mod tickets;
pub mod tokens;
pub mod two_factor;
pub mod usage;
pub mod users;
pub mod workspaces;

use std::str::FromStr;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgRow};
use sqlx::{PgPool, Row};

use crate::domain::AppError;

/// Embedded migrations from `backend/migrations`.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Opens a connection pool of `max_connections`, without a limit on how long
/// a statement may run: the workspace transfer and `nexc doctor` use it.
pub async fn connect(url: &str, max_connections: u32) -> anyhow::Result<PgPool> {
    connect_limited(url, max_connections, Duration::ZERO).await
}

/// Opens a connection pool of `max_connections` whose statements are cut off
/// after `statement_timeout` (zero: never), as the server's are
/// (`NEXC_DB_STATEMENT_TIMEOUT_SECS`).
///
/// A connection is not pinged before it is handed out (sqlx pings by
/// default): that ping is a whole round trip to PostgreSQL before every
/// query run on the pool, which costs little next to a local database and a
/// millisecond or more per query next to a managed one. A connection the
/// server closed fails its query instead, and is replaced; connections are
/// renewed after 30 minutes, and after ten idle minutes, anyway.
///
/// The limits are `SET` once each connection is opened, rather than sent as
/// startup options, which PgBouncer refuses. A transaction left open is
/// ended after twice the statement limit (at least a minute), so a stuck
/// request cannot hold its locks and its connection for ever.
pub async fn connect_limited(
    url: &str,
    max_connections: u32,
    statement_timeout: Duration,
) -> anyhow::Result<PgPool> {
    let options = PgConnectOptions::from_str(url)?.application_name("nexc");
    let session: Option<std::sync::Arc<str>> = session_limits(statement_timeout).map(Into::into);
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .min_connections(max_connections.div_ceil(4).min(4))
        .acquire_timeout(Duration::from_secs(10))
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(30 * 60))
        .test_before_acquire(false)
        .after_connect(move |conn, _| {
            let session = session.clone();
            Box::pin(async move {
                if let Some(sql) = session {
                    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
                        .execute(conn)
                        .await?;
                }
                Ok(())
            })
        })
        .connect_with(options)
        .await?;
    Ok(pool)
}

/// The `SET`s that put `statement_timeout` on a new connection; `None` for no limit.
fn session_limits(statement_timeout: Duration) -> Option<String> {
    let ms = statement_timeout.as_millis();
    (ms > 0).then(|| {
        let idle = (ms * 2).max(60_000);
        format!("SET statement_timeout = {ms}; SET idle_in_transaction_session_timeout = {idle}")
    })
}

/// A small pool beside the server's own, for what must not wait behind a
/// burst of requests: renewing the leases of the runs executing here. Its
/// connections open when first needed.
pub fn connect_lazy_small(pool: &PgPool, max_connections: u32) -> PgPool {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .min_connections(0)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(30 * 60))
        .test_before_acquire(false)
        .connect_lazy_with((*pool.connect_options()).clone())
}

/// Taken by the server that applies migrations; the others ask for it again until it is free.
const MIGRATION_LOCK: &str = "SELECT pg_try_advisory_lock(hashtext('nexc.migrate'))";
const MIGRATION_UNLOCK: &str = "SELECT pg_advisory_unlock(hashtext('nexc.migrate'))";
/// How often a server that waits for another's migrations asks for the lock again.
const MIGRATION_LOCK_POLL: Duration = Duration::from_millis(500);

/// Applies pending migrations, one server at a time.
///
/// The lock is asked for again and again rather than waited for. A session blocked on a lock
/// keeps the snapshot of its statement, and a `CREATE INDEX CONCURRENTLY` of the server holding
/// the lock waits for every older snapshot: two servers starting together would deadlock, and the
/// build would be left behind invalid (docs/DATABASE.md, "Building indexes"). sqlx's own lock,
/// which blocks, is then taken by the holder alone, at once.
///
/// `run_direct` is what `Migrator::run` does once it has a connection. Given a connection, `run`
/// ties its future to the lifetime of `Acquire` and the future is no longer `Send`, which the
/// workspace transfer needs: it migrates its target inside a spawned task.
pub async fn migrate(pool: &PgPool) -> anyhow::Result<()> {
    let mut conn = pool.acquire().await?;
    // An index built on a large table takes longer than any request may: the migrations run
    // without the pool's limits, on a connection that is closed afterwards rather than reused.
    conn.close_on_drop();
    sqlx::raw_sql("SET statement_timeout = 0; SET idle_in_transaction_session_timeout = 0")
        .execute(&mut *conn)
        .await?;
    loop {
        let locked: bool = sqlx::query_scalar(MIGRATION_LOCK)
            .fetch_one(&mut *conn)
            .await?;
        if locked {
            break;
        }
        tokio::time::sleep(MIGRATION_LOCK_POLL).await;
    }
    // A run that failed may still hold sqlx's lock: the connection is closed, and its locks go
    // with it.
    MIGRATOR.run_direct(None, &mut *conn, false).await?;
    sqlx::query(MIGRATION_UNLOCK).execute(&mut *conn).await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pool_limits_statements_only_when_asked_to() {
        assert_eq!(session_limits(Duration::ZERO), None);
        let limited = session_limits(Duration::from_secs(30)).unwrap();
        assert_eq!(
            limited,
            "SET statement_timeout = 30000; SET idle_in_transaction_session_timeout = 60000"
        );
        let long = session_limits(Duration::from_secs(45)).unwrap();
        assert!(
            long.ends_with("idle_in_transaction_session_timeout = 90000"),
            "{long}"
        );
    }
}
