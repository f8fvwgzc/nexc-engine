//! The workspace's ontology in onto-intelli, as the console reaches it: its
//! status with the open schema proposals (Settings → Ontology), and the
//! browser session the embedded onto-intelli UI (the Map page) signs in
//! with. The backend's service token never reaches the browser: the session
//! is the member's own, short-lived and bound to the workspace's tenant.

use axum::Json;
use axum::extract::State;
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use super::workspaces::{member_of, require};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::ontology::{
    OntoMode, OntologySession, OntologyStatus, ProposalStatus, SchemaProposal,
};
use crate::domain::workspace::WorkspaceAction;
use crate::http::extract::{AuthUser, Path};
use crate::http::problem::Problem;
use crate::ontology::{OntoError, Principal, tenant_of, workspace_name};

/// Open proposals listed at most.
const PROPOSALS_MAX: u32 = 50;

/// onto-intelli's UI, as browsers reach it (`…/ui/`).
pub fn ui_url(state: &AppState) -> Option<String> {
    state
        .ontology
        .as_ref()
        .and(state.settings.onto_public_url.as_ref())
        .map(|base| format!("{base}/ui/"))
}

/// A proposal as onto-intelli answers it, as nexc lists it.
pub fn proposal_of(value: &Value, ui_url: Option<&str>) -> Option<SchemaProposal> {
    let id = value["id"].as_str()?.to_owned();
    let text = |key: &str| value[key].as_str().unwrap_or_default().to_owned();
    let created_at = value["created_at"]
        .as_i64()
        .and_then(DateTime::<Utc>::from_timestamp_millis)
        .unwrap_or_else(Utc::now);
    Some(SchemaProposal {
        title: text("title"),
        rationale: text("rationale"),
        status: value["status"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(ProposalStatus::Open),
        proposer: value["proposer"]["id"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        by_agent: value["proposer"]["agent"].as_bool().unwrap_or(false),
        breaking: value["plan"]["breaking"].as_bool().unwrap_or(false),
        changes: value["change_set"]["changes"]
            .as_array()
            .map_or(0, Vec::len),
        created_at,
        review_url: ui_url.map(|ui| format!("{ui}#/proposals/{id}")),
        id,
    })
}

fn unavailable(err: OntoError) -> AppError {
    match err {
        OntoError::Unavailable(detail) => {
            AppError::Unavailable(format!("onto-intelli is not reachable: {detail}"))
        }
        other => AppError::Unavailable(format!("onto-intelli did not open a session: {other}")),
    }
}

/// How the workspace's ontology is kept: the mode, where onto-intelli's UI
/// is, the workspace's tenant there, and the schema change proposals still
/// open, newest first, each with its link to onto-intelli's review inbox
/// (approval happens there). Members and above.
#[utoipa::path(get, path = "/workspaces/{wid}/ontology", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, body = OntologyStatus), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn status(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Json<OntologyStatus>, AppError> {
    let workspace = member_of(&state, auth, wid).await?;
    require(&workspace, WorkspaceAction::ViewMembers)?;
    let Some(backend) = &state.ontology else {
        return Ok(Json(OntologyStatus {
            mode: OntoMode::Off,
            ui_url: None,
            tenant: None,
            workspace: None,
            proposals: Vec::new(),
            error: None,
        }));
    };
    let prefix = &state.settings.onto_workspace_prefix;
    let principal = Principal::user(prefix, wid, auth.id, workspace.role);
    let ui = ui_url(&state);
    let (proposals, error) = match backend
        .proposals(&principal, Some("open"), PROPOSALS_MAX)
        .await
    {
        Ok(found) => (
            found
                .iter()
                .filter_map(|p| proposal_of(p, ui.as_deref()))
                .collect(),
            None,
        ),
        Err(err) => (Vec::new(), Some(err.to_string())),
    };
    Ok(Json(OntologyStatus {
        mode: state.onto_mode(),
        ui_url: ui,
        tenant: Some(tenant_of(wid)),
        workspace: Some(workspace_name(prefix, wid)),
        proposals,
        error,
    }))
}

/// Opens a browser session for onto-intelli's UI, for the caller in this
/// workspace's tenant: their own principal with the roles of their
/// workspace role, short-lived (`NEXC_ONTO_SESSION_TTL_SECS`). The page
/// hands it to the embedded UI with `postMessage` once the UI announced
/// itself (`onto:ready`). Members and above; 409 when onto-intelli is off,
/// 503 when it is not reachable.
#[utoipa::path(post, path = "/workspaces/{wid}/ontology/session", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, body = OntologySession), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "onto-intelli is off (NEXC_ONTO_MODE=off)", body = Problem),
        (status = 503, description = "onto-intelli is not reachable", body = Problem)))]
pub async fn session(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Json<OntologySession>, AppError> {
    let workspace = member_of(&state, auth, wid).await?;
    require(&workspace, WorkspaceAction::ViewMembers)?;
    let (Some(backend), Some(ui_url)) = (&state.ontology, ui_url(&state)) else {
        return Err(AppError::Conflict(
            "the workspace map is off: onto-intelli is not configured (NEXC_ONTO_MODE=off)".into(),
        ));
    };
    let prefix = &state.settings.onto_workspace_prefix;
    let principal = Principal::user(prefix, wid, auth.id, workspace.role);
    let minted = backend
        .mint_session(&principal, state.settings.onto_session_ttl.as_secs())
        .await
        .map_err(unavailable)?;
    Ok(Json(OntologySession {
        ui_url,
        token: minted.token,
        tenant: minted.tenant,
        workspace: principal.workspace,
        expires_at: DateTime::<Utc>::from_timestamp(minted.expires_at, 0).unwrap_or_else(Utc::now),
    }))
}
