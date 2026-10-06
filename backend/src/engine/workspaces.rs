//! Work on a whole workspace that reaches beyond the database.

use uuid::Uuid;

use crate::app::AppState;
use crate::domain::AppError;
use crate::repo;

/// Removes a workspace with everything in it: its rows, and the uploaded
/// documents and run artifacts on disk, which the database does not reach.
pub async fn erase(state: &AppState, wid: Uuid) -> Result<(), AppError> {
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
    repo::workspaces::delete(&state.db, wid).await?;
    state.memories.invalidate(wid);
    super::artifacts::remove_runs(state, &runs).await;
    for id in documents {
        let _ = tokio::fs::remove_file(state.settings.documents_dir().join(id.to_string())).await;
    }
    Ok(())
}
