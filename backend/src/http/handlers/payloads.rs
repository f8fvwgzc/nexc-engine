//! What a span said, for those who may read it: the recorded prompt,
//! completion or tool input and output of a span, decrypted, with the
//! classifications of the data its node was given.

use axum::Json;
use axum::extract::State;
use uuid::Uuid;

use super::workspaces::audit;
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::audit::AuditAction;
use crate::domain::recording::{SpanPayload, SpanPayloads};
use crate::http::extract::{AuthUser, Path};
use crate::http::problem::Problem;
use crate::repo;
use crate::repo::audit::Subject;

/// The recorded payloads of a span (the run's owner and the workspace's
/// admins; other members 403; outsiders 404). Reading is audited as
/// `payload_viewed` when there was something to read; a span recorded
/// nothing answers with no payloads.
#[utoipa::path(get, path = "/spans/{sid}/payload", tag = "runs", security(("bearer" = [])),
    params(("sid" = Uuid, Path, description = "Span id")),
    responses((status = 200, body = SpanPayloads), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn payload(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(sid): Path<Uuid>,
) -> Result<Json<SpanPayloads>, AppError> {
    let span = repo::spans::find_any(&state.db, sid)
        .await?
        .ok_or(AppError::NotFound("span"))?;
    let run = repo::runs::find_owned(&state.db, auth.id, span.run_id)
        .await?
        .ok_or(AppError::NotFound("span"))?;
    let workspace = repo::graphs::workspace_of(&state.db, run.graph_id).await?;
    let admin = match workspace {
        Some(workspace) => repo::workspaces::role_of(&state.db, workspace, auth.id)
            .await?
            .is_some_and(|role| role.is_admin()),
        None => false,
    };
    if run.owner_id != auth.id && !admin {
        return Err(AppError::Forbidden(
            "only the run's owner and the workspace's admins read what a span said".into(),
        ));
    }
    let rows = repo::span_payloads::of_span(&state.db, sid).await?;
    let mut payloads = Vec::with_capacity(rows.len());
    for row in rows {
        let bytes = state
            .secret_box
            .open(&row.content, sid.as_bytes())
            .map_err(AppError::Internal)?;
        payloads.push(SpanPayload {
            kind: row.kind,
            content: String::from_utf8_lossy(&bytes).into_owned(),
            size: i64::from(row.size),
            redactions: i64::from(row.redactions),
            created_at: row.created_at,
        });
    }
    let labels = repo::runs::context_labels(&state.db, span.run_id, span.node_id).await?;
    if let (Some(workspace), false) = (workspace, payloads.is_empty()) {
        let kinds: Vec<&str> = payloads.iter().map(|p| p.kind.as_str()).collect();
        audit(
            &state,
            workspace,
            auth.id,
            AuditAction::PayloadViewed,
            Subject::Text(&sid.to_string()),
            &format!("run {}: {}", span.run_id, kinds.join(", ")),
        )
        .await;
    }
    Ok(Json(SpanPayloads {
        span_id: sid,
        run_id: span.run_id,
        node_id: span.node_id,
        kind: span.kind,
        labels,
        payloads,
    }))
}
