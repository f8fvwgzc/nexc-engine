//! Alert destinations of a workspace (alert routing): where its alerts and
//! approvals go. Admins only; every change is audited as
//! `alert_destination_changed`.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

use super::workspaces::{audit, member_of, require};
use crate::alerting;
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::alert_routing::{
    AlertDelivery, AlertDestination, AlertDestinationInput, AlertDestinationPatch,
    DESTINATIONS_MAX, DestinationTest, IssuedAlertDestination, LOG_MAX, SECRET_PREFIX,
};
use crate::domain::audit::AuditAction;
use crate::domain::policy_document::Severity;
use crate::domain::validation::{FieldErrors, Validate};
use crate::domain::workspace::{Workspace, WorkspaceAction};
use crate::http::extract::{AuthUser, Path, Query, ValidatedJson};
use crate::http::problem::Problem;
use crate::repo;
use crate::repo::alert_destinations::{AlertDestinationRow, DestinationChanges, NewDestination};
use crate::repo::audit::Subject;
use crate::security::random::random_token;

impl Validate for AlertDestinationInput {
    fn validate(&self, errors: &mut FieldErrors) {
        self.check(errors);
    }
}

impl Validate for AlertDestinationPatch {
    fn validate(&self, errors: &mut FieldErrors) {
        self.check(errors);
    }
}

/// Query of `GET …/alert-destinations/{id}/deliveries`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct LogQuery {
    /// Entries to return, 1–200 (default 50).
    pub limit: Option<i64>,
}

/// The workspace, if the caller administers it (members 403, others 404).
async fn administered(state: &AppState, auth: AuthUser, wid: Uuid) -> Result<Workspace, AppError> {
    let workspace = member_of(state, auth, wid).await?;
    require(&workspace, WorkspaceAction::UpdateSettings)?;
    Ok(workspace)
}

/// One destination of the workspace, or 404.
async fn destination_of(
    state: &AppState,
    wid: Uuid,
    id: Uuid,
) -> Result<AlertDestinationRow, AppError> {
    repo::alert_destinations::find(&state.db, wid, id)
        .await?
        .ok_or(AppError::NotFound("alert destination"))
}

/// The URL checked against the server's host rules, sealed for the row.
fn seal_url(state: &AppState, id: Uuid, url: &str) -> Result<(Vec<u8>, String), AppError> {
    let host = alerting::check_destination_url(
        url,
        state.settings.alert_allow_private_hosts,
        &state.egress,
    )
    .map_err(|problem| AppError::field("url", problem))?;
    let sealed = state
        .secret_box
        .seal(url.trim().as_bytes(), id.as_bytes())
        .map_err(AppError::Internal)?;
    Ok((sealed, host))
}

fn describe_filters(d: &AlertDestination) -> String {
    let kinds = if d.kinds.is_empty() {
        "every kind".to_owned()
    } else {
        d.kinds
            .iter()
            .map(|k| k.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "{kinds} from {} up{}",
        d.min_severity,
        if d.include_approvals {
            ", approvals too"
        } else {
            ""
        }
    )
}

/// The alert destinations of a workspace (admins).
#[utoipa::path(get, path = "/workspaces/{wid}/alert-destinations", tag = "alert-destinations", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, body = [AlertDestination]), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Json<Vec<AlertDestination>>, AppError> {
    administered(&state, auth, wid).await?;
    Ok(Json(
        repo::alert_destinations::list(&state.db, wid)
            .await?
            .into_iter()
            .map(|row| row.destination)
            .collect(),
    ))
}

/// Registers a destination (admins). The signing secret is in the answer
/// once and never shown again; the URL is kept encrypted.
#[utoipa::path(post, path = "/workspaces/{wid}/alert-destinations", tag = "alert-destinations", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")), request_body = AlertDestinationInput,
    responses((status = 201, body = IssuedAlertDestination), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, body = Problem), (status = 422, body = Problem)))]
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<AlertDestinationInput>,
) -> Result<(StatusCode, Json<IssuedAlertDestination>), AppError> {
    administered(&state, auth, wid).await?;
    let existing = repo::alert_destinations::list(&state.db, wid).await?;
    if existing.len() as i64 >= DESTINATIONS_MAX {
        return Err(AppError::Unprocessable(format!(
            "a workspace registers at most {DESTINATIONS_MAX} alert destinations"
        )));
    }
    let name = req.name.trim().to_owned();
    if existing.iter().any(|row| row.destination.name == name) {
        return Err(AppError::Conflict(format!(
            "an alert destination named {name} is already registered"
        )));
    }
    let id = Uuid::now_v7();
    let (url, host) = seal_url(&state, id, &req.url)?;
    let secret = format!("{SECRET_PREFIX}{}", random_token());
    let sealed_secret = state
        .secret_box
        .seal(secret.as_bytes(), id.as_bytes())
        .map_err(AppError::Internal)?;
    let mut kinds = req.kinds.clone();
    kinds.sort();
    kinds.dedup();
    let row = repo::alert_destinations::insert(
        &state.db,
        id,
        &NewDestination {
            workspace_id: wid,
            name: &name,
            kind: req.kind,
            url: &url,
            host: &host,
            secret: &sealed_secret,
            kinds: &kinds,
            min_severity: req.min_severity.unwrap_or(Severity::Low),
            include_approvals: req.include_approvals,
            enabled: req.enabled.unwrap_or(true),
            created_by: auth.id,
        },
    )
    .await?;
    audit(
        &state,
        wid,
        auth.id,
        AuditAction::AlertDestinationChanged,
        Subject::Text(&format!("alerts:{name}")),
        &format!(
            "added: {} to {host}; {}",
            row.destination.kind,
            describe_filters(&row.destination)
        ),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(IssuedAlertDestination {
            destination: row.destination,
            secret,
        }),
    ))
}

/// One destination (admins).
#[utoipa::path(get, path = "/workspaces/{wid}/alert-destinations/{id}", tag = "alert-destinations", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("id" = Uuid, Path, description = "Destination id")),
    responses((status = 200, body = AlertDestination), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<AlertDestination>, AppError> {
    administered(&state, auth, wid).await?;
    Ok(Json(destination_of(&state, wid, id).await?.destination))
}

/// Changes a destination (admins): name, URL, filters, on or off. Enabling
/// one the platform switched off clears its failure count.
#[utoipa::path(patch, path = "/workspaces/{wid}/alert-destinations/{id}", tag = "alert-destinations", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("id" = Uuid, Path, description = "Destination id")),
    request_body = AlertDestinationPatch,
    responses((status = 200, body = AlertDestination), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, body = Problem), (status = 422, body = Problem)))]
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, id)): Path<(Uuid, Uuid)>,
    ValidatedJson(req): ValidatedJson<AlertDestinationPatch>,
) -> Result<Json<AlertDestination>, AppError> {
    administered(&state, auth, wid).await?;
    let before = destination_of(&state, wid, id).await?.destination;
    if req.is_empty() {
        return Ok(Json(before));
    }
    let name = req.name.as_deref().map(str::trim);
    if let Some(name) = name
        && name != before.name
        && repo::alert_destinations::list(&state.db, wid)
            .await?
            .iter()
            .any(|row| row.destination.name == name)
    {
        return Err(AppError::Conflict(format!(
            "an alert destination named {name} is already registered"
        )));
    }
    let sealed = match &req.url {
        Some(url) => Some(seal_url(&state, id, url)?),
        None => None,
    };
    let kinds = req.kinds.clone().map(|mut kinds| {
        kinds.sort();
        kinds.dedup();
        kinds
    });
    let after = repo::alert_destinations::update(
        &state.db,
        wid,
        id,
        &DestinationChanges {
            name,
            url: sealed
                .as_ref()
                .map(|(url, host)| (url.as_slice(), host.as_str())),
            kinds: kinds.as_deref(),
            min_severity: req.min_severity,
            include_approvals: req.include_approvals,
            enabled: req.enabled,
        },
    )
    .await?
    .ok_or(AppError::NotFound("alert destination"))?
    .destination;
    let mut changes = Vec::new();
    if before.name != after.name {
        changes.push(format!("renamed from {}", before.name));
    }
    if sealed.is_some() {
        changes.push(format!("URL now at {}", after.host));
    }
    if before.kinds != after.kinds
        || before.min_severity != after.min_severity
        || before.include_approvals != after.include_approvals
    {
        changes.push(format!("sends {}", describe_filters(&after)));
    }
    if before.enabled != after.enabled {
        changes.push(if after.enabled { "enabled" } else { "disabled" }.to_owned());
    }
    if !changes.is_empty() {
        audit(
            &state,
            wid,
            auth.id,
            AuditAction::AlertDestinationChanged,
            Subject::Text(&format!("alerts:{}", after.name)),
            &changes.join("; "),
        )
        .await;
    }
    Ok(Json(after))
}

/// Removes a destination and its delivery log (admins).
#[utoipa::path(delete, path = "/workspaces/{wid}/alert-destinations/{id}", tag = "alert-destinations", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("id" = Uuid, Path, description = "Destination id")),
    responses((status = 204, description = "Removed"), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    administered(&state, auth, wid).await?;
    let row = destination_of(&state, wid, id).await?;
    if !repo::alert_destinations::delete(&state.db, wid, id).await? {
        return Err(AppError::NotFound("alert destination"));
    }
    audit(
        &state,
        wid,
        auth.id,
        AuditAction::AlertDestinationChanged,
        Subject::Text(&format!("alerts:{}", row.destination.name)),
        &format!(
            "removed: {} to {}",
            row.destination.kind, row.destination.host
        ),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Sends a test event to the destination at once and says what it answered
/// (admins). The attempt is in the destination's log; it does not count
/// toward switching the destination off.
#[utoipa::path(post, path = "/workspaces/{wid}/alert-destinations/{id}/test", tag = "alert-destinations", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("id" = Uuid, Path, description = "Destination id")),
    responses((status = 200, body = DestinationTest), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn send_test(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<DestinationTest>, AppError> {
    administered(&state, auth, wid).await?;
    let row = destination_of(&state, wid, id).await?;
    let found = alerting::send_test(&state, &row)
        .await
        .map_err(AppError::Internal)?;
    audit(
        &state,
        wid,
        auth.id,
        AuditAction::AlertDestinationChanged,
        Subject::Text(&format!("alerts:{}", row.destination.name)),
        &format!(
            "tested: {}",
            if found.ok {
                format!("HTTP {}", found.status.unwrap_or(0))
            } else {
                found.detail.clone()
            }
        ),
    )
    .await;
    Ok(Json(found))
}

/// The delivery log of a destination, newest first (admins).
#[utoipa::path(get, path = "/workspaces/{wid}/alert-destinations/{id}/deliveries", tag = "alert-destinations", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("id" = Uuid, Path, description = "Destination id"), LogQuery),
    responses((status = 200, body = [AlertDelivery]), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn deliveries(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, id)): Path<(Uuid, Uuid)>,
    Query(query): Query<LogQuery>,
) -> Result<Json<Vec<AlertDelivery>>, AppError> {
    administered(&state, auth, wid).await?;
    destination_of(&state, wid, id).await?;
    let limit = query.limit.unwrap_or(50).clamp(1, LOG_MAX);
    Ok(Json(
        repo::alert_deliveries::log(&state.db, id, limit).await?,
    ))
}
