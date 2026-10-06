//! Application wiring: state, router, background workers and the server
//! lifecycle (graceful shutdown on Ctrl-C / SIGTERM).
#![forbid(unsafe_code)]

pub mod router;
mod state;
pub mod workers;

use std::net::SocketAddr;

use tokio_util::sync::CancellationToken;

pub use state::{AppState, http_client};

use crate::config::Settings;
use crate::domain::user::{self, Role};
use crate::{memory, orchestrator, repo, security};

/// Connects to the database and applies migrations.
pub async fn connect_db(settings: &Settings) -> anyhow::Result<sqlx::PgPool> {
    let pool = repo::connect(settings.database_url.expose(), settings.db_max_connections).await?;
    repo::migrate(&pool).await?;
    Ok(pool)
}

/// Creates the bootstrap admin from `NEXC_ADMIN_EMAIL` / `NEXC_ADMIN_PASSWORD`
/// if both are set and the account does not exist yet.
pub async fn bootstrap_admin(state: &AppState) -> anyhow::Result<()> {
    let (Some(email), Some(password)) =
        (&state.settings.admin_email, &state.settings.admin_password)
    else {
        return Ok(());
    };
    if repo::users::email_exists(&state.db, email).await? {
        return Ok(());
    }
    let hash = security::password::hash_password(password.expose())?;
    crate::http::handlers::auth::create_user(
        state,
        &user::normalize_email(email),
        "Administrator",
        Role::Admin,
        &hash,
    )
    .await
    .map_err(|e| anyhow::anyhow!("cannot create bootstrap admin: {e}"))?;
    tracing::info!(%email, "bootstrap admin created");
    Ok(())
}

/// Gives every account created before workspaces existed a personal one.
pub async fn seed_missing_workspaces(state: &AppState) -> anyhow::Result<()> {
    use crate::http::handlers::{auth::personal_workspace_name, workspaces::create_owned};
    for (user, name) in repo::workspaces::users_without_workspace(&state.db).await? {
        let mut tx = state.db.begin().await?;
        create_owned(
            &mut tx,
            user,
            &personal_workspace_name(&name),
            &state.settings.llm_model,
        )
        .await
        .map_err(|e| anyhow::anyhow!("cannot create a workspace for {user}: {e}"))?;
        tx.commit().await?;
    }
    let adopted = repo::graphs::adopt_orphans(&state.db).await?;
    if adopted > 0 {
        tracing::info!(adopted, "graphs assigned to their creators' workspaces");
    }
    for team in repo::issues::teams_without_states(&state.db).await? {
        let mut tx = state.db.begin().await?;
        repo::issues::seed_states(&mut tx, team).await?;
        tx.commit().await?;
    }
    let adopted = repo::memories::adopt_orphans(&state.db).await?;
    if adopted > 0 {
        tracing::info!(adopted, "memories assigned to workspaces");
    }
    Ok(())
}

/// Runs the HTTP server until Ctrl-C / SIGTERM, then drains connections and
/// cancels local runs.
pub async fn serve(settings: Settings) -> anyhow::Result<()> {
    let db = connect_db(&settings).await?;
    let addr = SocketAddr::new(settings.host, settings.port);
    let state = AppState::new(settings, db)?;
    if memory::vectors::ensure(&state.db).await? {
        let filled = memory::vectors::backfill(&state.db).await?;
        state.memories.set_vector_search(true);
        tracing::info!(filled, "memory vector search is on (pgvector, HNSW)");
    } else {
        tracing::info!("memory vector search is off: the database has no pgvector extension");
    }
    bootstrap_admin(&state).await?;
    seed_missing_workspaces(&state).await?;
    orchestrator::seed_missing_orgs(&state).await?;
    tokio::fs::create_dir_all(state.settings.artifacts_dir()).await?;
    tokio::fs::create_dir_all(state.settings.documents_dir()).await?;

    let shutdown = CancellationToken::new();
    workers::spawn(&state, shutdown.clone());
    let app = router::build(state.clone());
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, env = ?state.settings.env, "nexc listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    tracing::info!("shutting down");
    state.engine.cancel_all();
    shutdown.cancel();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    state.db.close().await;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
