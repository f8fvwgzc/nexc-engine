//! Work on a whole workspace that reaches beyond the database.

use uuid::Uuid;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::platform::PlatformAction;
use crate::repo;
use crate::repo::evidence::WorkspaceEvidence;

/// Removes a workspace with everything in it: its rows, and the uploaded
/// documents and run artifacts on disk, which the database does not reach.
/// Its evidence (runs, spans, the audit log) goes with it: this is one of the
/// purge paths. Since the workspace's own log goes too, the platform log says
/// who erased it (`actor`), how (`how`) and what evidence went, written in
/// the transaction that erases it.
pub async fn erase(
    state: &AppState,
    wid: Uuid,
    actor: Uuid,
    how: &str,
) -> Result<WorkspaceEvidence, AppError> {
    let documents: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM documents WHERE workspace_id = $1")
            .bind(wid)
            .fetch_all(&state.db)
            .await?;
    let runs: Vec<Uuid> = sqlx::query_scalar(
        "SELECT r.id FROM runs r JOIN graphs g ON g.id = r.graph_id WHERE g.workspace_id = $1",
    )
    .bind(wid)
    .fetch_all(&state.db)
    .await?;
    let mut tx = state.db.begin().await?;
    let name: String = sqlx::query_scalar("SELECT name FROM workspaces WHERE id = $1 FOR UPDATE")
        .bind(wid)
        .fetch_one(&mut *tx)
        .await?;
    let evidence = repo::evidence::of_workspace(&mut *tx, wid).await?;
    let detail = format!("{how}; {}", evidence.describe());
    let action = PlatformAction::WorkspaceDeleted;
    repo::platform::record(&mut *tx, actor, action, &name, &detail).await?;
    repo::evidence::allow_purge(&mut tx).await?;
    repo::workspaces::delete(&mut *tx, wid).await?;
    tx.commit().await?;
    state.memories.invalidate(wid);
    for key in [
        crate::app::caches::Key::Policies(wid),
        crate::app::caches::Key::Guardrails(wid),
        crate::app::caches::Key::WorkspaceLlm(wid),
    ] {
        state.caches.invalidate(key);
    }
    super::artifacts::remove_runs(state, &runs).await;
    for id in documents {
        let _ = tokio::fs::remove_file(state.settings.documents_dir().join(id.to_string())).await;
    }
    Ok(evidence)
}
