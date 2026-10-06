//! What the server runs on, for whoever administers the instance: the
//! database, the agent runtime, embeddings, and a way to check that another
//! PostgreSQL or Redis is reachable before pointing the server at it.
//!
//! The server's own connections come from its environment and are read at
//! start. They are shown here, never changed: moving to another database is
//! a restart with a new `NEXC_DATABASE_URL` after its data was copied over.

use std::time::Duration;

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use sqlx::Connection;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use utoipa::ToSchema;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::user::Role;
use crate::domain::validation::{FieldErrors, Validate};
use crate::engine::knowledge;
use crate::http::extract::{AuthUser, ValidatedJson};
use crate::http::problem::Problem;
use crate::repo;

const CHECK_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) fn require_instance_admin(auth: AuthUser) -> Result<(), AppError> {
    if auth.role == Role::Admin {
        Ok(())
    } else {
        Err(AppError::Forbidden(
            "only administrators of this server see its infrastructure".into(),
        ))
    }
}

/// A connection URL without its credentials: `postgres://host:5432/db`.
fn without_credentials(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let rest = rest.rsplit_once('@').map_or(rest, |(_, host)| host);
    let rest = rest.split('?').next().unwrap_or(rest);
    format!("{scheme}://{rest}")
}

/// The database the server is connected to.
#[derive(Debug, Serialize, ToSchema)]
pub struct DatabaseStatus {
    /// Where it is, without credentials.
    pub location: String,
    pub version: String,
    pub size_bytes: i64,
    /// The pgvector version, when the extension is enabled.
    #[schema(required = true)]
    pub pgvector: Option<String>,
    pub migrations_applied: i64,
    /// Migrations this build knows; more than applied means a restart is due.
    pub migrations_known: i64,
}

/// What the server runs on.
#[derive(Debug, Serialize, ToSchema)]
pub struct Infrastructure {
    pub database: DatabaseStatus,
    /// Where the agent runtime is expected, and whether it answers.
    pub runtime_url: String,
    pub runtime_reachable: bool,
    /// The server's embedding model (`builtin-hash-256` when none is configured).
    pub embedding_model: String,
    /// Where background work is queued and what caches there are: all in
    /// PostgreSQL and in process, with no other service to run.
    pub queue: String,
    pub cache: String,
}

/// The infrastructure this server runs on (server administrators only).
#[utoipa::path(get, path = "/admin/infrastructure", tag = "admin", security(("bearer" = [])),
    responses((status = 200, body = Infrastructure), (status = 403, body = Problem)))]
pub async fn status(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Infrastructure>, AppError> {
    require_instance_admin(auth)?;
    let (version, size_bytes): (String, i64) =
        sqlx::query_as("SELECT version(), pg_database_size(current_database())")
            .fetch_one(&state.db)
            .await?;
    let pgvector: Option<String> =
        sqlx::query_scalar("SELECT extversion FROM pg_extension WHERE extname = 'vector'")
            .fetch_optional(&state.db)
            .await?;
    let migrations_applied: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success")
            .fetch_one(&state.db)
            .await?;
    let runtime = state.health.agent_runtime(&state).await;
    // Any workspace resolves to the server's embedding when it set none of its own.
    let embedding_model = knowledge::server_embedding_model(&state);
    Ok(Json(Infrastructure {
        database: DatabaseStatus {
            location: without_credentials(state.settings.database_url.expose()),
            version: version.split(" on ").next().unwrap_or(&version).to_owned(),
            size_bytes,
            pgvector,
            migrations_applied,
            migrations_known: i64::try_from(repo::MIGRATOR.iter().count()).unwrap_or(0),
        },
        runtime_url: state.settings.runtime_url.clone(),
        runtime_reachable: runtime.ok,
        embedding_model,
        queue: "PostgreSQL (rows claimed with SKIP LOCKED)".into(),
        cache: "in process, per server instance".into(),
    }))
}

/// `POST /admin/infrastructure/check` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckConnection {
    /// `postgres://user:password@host:5432/db` or `redis://[:password@]host:6379`.
    /// Used for this check only: it is neither stored nor logged.
    pub url: String,
}

impl Validate for CheckConnection {
    fn validate(&self, errors: &mut FieldErrors) {
        let known = ["postgres://", "postgresql://", "redis://"];
        if !known.iter().any(|scheme| self.url.starts_with(scheme)) || self.url.len() > 2_000 {
            errors.add("url", "must start with postgres:// or redis://");
        }
    }
}

/// What a connection check found.
#[derive(Debug, Serialize, ToSchema)]
pub struct ConnectionCheck {
    pub reachable: bool,
    /// The server's version, or why it could not be reached.
    pub detail: String,
    /// PostgreSQL: whether pgvector can be enabled there.
    #[schema(required = true)]
    pub pgvector_available: Option<bool>,
    /// PostgreSQL: how many of this build's migrations are applied there;
    /// `0` for an empty database, which the server sets up when it starts on it.
    #[schema(required = true)]
    pub migrations_applied: Option<i64>,
}

async fn check_postgres(url: &str) -> Result<ConnectionCheck, sqlx::Error> {
    let mut conn = sqlx::PgConnection::connect(url).await?;
    let version: String = sqlx::query_scalar("SELECT version()")
        .fetch_one(&mut conn)
        .await?;
    let pgvector: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_available_extensions WHERE name = 'vector')",
    )
    .fetch_one(&mut conn)
    .await?;
    let has_migrations: bool =
        sqlx::query_scalar("SELECT to_regclass('_sqlx_migrations') IS NOT NULL")
            .fetch_one(&mut conn)
            .await?;
    let applied: i64 = if has_migrations {
        sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success")
            .fetch_one(&mut conn)
            .await?
    } else {
        0
    };
    conn.close().await?;
    Ok(ConnectionCheck {
        reachable: true,
        detail: version.split(" on ").next().unwrap_or(&version).to_owned(),
        pgvector_available: Some(pgvector),
        migrations_applied: Some(applied),
    })
}

/// Says PING to a Redis server (after AUTH when the URL carries a password)
/// and returns its reply.
async fn check_redis(url: &str) -> anyhow::Result<String> {
    let rest = url.trim_start_matches("redis://");
    let (credentials, address) = match rest.rsplit_once('@') {
        Some((credentials, address)) => (Some(credentials), address),
        None => (None, rest),
    };
    let address = address.split('/').next().unwrap_or(address);
    let address = if address.contains(':') {
        address.to_owned()
    } else {
        format!("{address}:6379")
    };
    let mut stream = tokio::net::TcpStream::connect(&address).await?;
    let mut reply = [0u8; 256];
    if let Some(password) = credentials.map(|c| c.rsplit_once(':').map_or(c, |(_, p)| p)) {
        let auth = format!("*2\r\n$4\r\nAUTH\r\n${}\r\n{password}\r\n", password.len());
        stream.write_all(auth.as_bytes()).await?;
        let read = stream.read(&mut reply).await?;
        anyhow::ensure!(
            reply[..read].starts_with(b"+OK"),
            "the server refused the password"
        );
    }
    stream.write_all(b"*1\r\n$4\r\nPING\r\n").await?;
    let read = stream.read(&mut reply).await?;
    let text = String::from_utf8_lossy(&reply[..read]).trim().to_owned();
    anyhow::ensure!(text.starts_with("+PONG"), "unexpected reply: {text}");
    Ok("Redis answered PONG".to_owned())
}

/// Checks that a PostgreSQL or Redis server can be reached from this server
/// with the given URL (server administrators only). Nothing is changed and
/// the URL is not kept.
#[utoipa::path(post, path = "/admin/infrastructure/check", tag = "admin", security(("bearer" = [])),
    request_body = CheckConnection,
    responses((status = 200, body = ConnectionCheck), (status = 403, body = Problem), (status = 422, body = Problem)))]
pub async fn check(
    State(_state): State<AppState>,
    auth: AuthUser,
    ValidatedJson(req): ValidatedJson<CheckConnection>,
) -> Result<Json<ConnectionCheck>, AppError> {
    require_instance_admin(auth)?;
    let unreachable = |detail: String| ConnectionCheck {
        reachable: false,
        detail,
        pgvector_available: None,
        migrations_applied: None,
    };
    let result = if req.url.starts_with("redis://") {
        match tokio::time::timeout(CHECK_TIMEOUT, check_redis(&req.url)).await {
            Ok(Ok(detail)) => ConnectionCheck {
                reachable: true,
                detail,
                pgvector_available: None,
                migrations_applied: None,
            },
            Ok(Err(err)) => unreachable(err.to_string()),
            Err(_) => unreachable("no answer within 5 seconds".into()),
        }
    } else {
        match tokio::time::timeout(CHECK_TIMEOUT, check_postgres(&req.url)).await {
            Ok(Ok(found)) => found,
            // The driver's message names the host and the reason, not the password.
            Ok(Err(err)) => unreachable(err.to_string()),
            Err(_) => unreachable("no answer within 5 seconds".into()),
        }
    };
    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_location_never_shows_credentials() {
        assert_eq!(
            without_credentials("postgres://nexc:s3cr3t@db.internal:5432/nexc?sslmode=require"),
            "postgres://db.internal:5432/nexc"
        );
        assert_eq!(
            without_credentials("postgres://localhost/nexc"),
            "postgres://localhost/nexc"
        );
        assert_eq!(
            without_credentials("postgres://u:p@ss@host/db"),
            "postgres://host/db",
            "a password may contain an @"
        );
    }
}
