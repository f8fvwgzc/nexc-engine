//! LLM settings: the account a user connects, the credential of a workspace,
//! and the models their provider offers.

use std::net::IpAddr;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::workspaces::{audit, member_of, require};
use crate::domain::audit::AuditAction;
use crate::repo::audit::Subject;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::settings::{LlmProviderKind, LlmSettings, key_hint};
use crate::domain::validation::{FieldErrors, Validate, check_text};
use crate::domain::workspace::WorkspaceAction;
use crate::engine::credentials;
use crate::http::extract::{AuthUser, Path, Query, ValidatedJson};
use crate::http::problem::Problem;
use crate::llm::catalog::{self, ModelCatalog};
use crate::repo;
use crate::repo::settings::KeyUpdate;

/// `PUT /settings/llm` body. `api_key`: omitted keeps the stored key,
/// `""` deletes it, any other value replaces it.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateLlmSettings {
    pub provider: LlmProviderKind,
    pub model: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
}

impl Validate for UpdateLlmSettings {
    fn validate(&self, errors: &mut FieldErrors) {
        check_text(errors, "model", &self.model, 100);
        if let Some(url) = self.base_url.as_deref().filter(|u| !u.is_empty())
            && !is_http_url(url)
        {
            errors.add(
                "base_url",
                "must be an http(s) URL of at most 300 characters",
            );
        }
        if let Some(key) = self.api_key.as_deref().filter(|k| !k.is_empty())
            && (key.len() < 8
                || key.len() > 512
                || key.chars().any(|c| c.is_whitespace() || c.is_control()))
        {
            errors.add(
                "api_key",
                "must be 8-512 printable characters without spaces",
            );
        }
    }
}

fn is_http_url(url: &str) -> bool {
    url.len() <= 300
        && reqwest::Url::parse(url)
            .is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
}

/// In production only public `https` endpoints may be configured, so the
/// server cannot be pointed at internal services (SSRF).
fn check_production_url(url: &str) -> Result<(), AppError> {
    let parsed =
        reqwest::Url::parse(url).map_err(|_| AppError::field("base_url", "invalid URL"))?;
    let host = parsed.host_str().unwrap_or_default();
    let private_ip = host
        .trim_matches(['[', ']'])
        .parse::<IpAddr>()
        .is_ok_and(|ip| match ip {
            IpAddr::V4(v4) => {
                v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_unspecified()
            }
            IpAddr::V6(v6) => {
                v6.is_loopback() || v6.is_unspecified() || (v6.segments()[0] & 0xfe00) == 0xfc00
            }
        });
    if parsed.scheme() != "https"
        || private_ip
        || host == "localhost"
        || host.ends_with(".internal")
    {
        return Err(AppError::field(
            "base_url",
            "must be a public https URL in production",
        ));
    }
    Ok(())
}

/// Query of the `/settings/llm` reads.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct LlmScopeQuery {
    /// The workspace the caller is working in; its credential applies when
    /// they have not connected an account of their own.
    pub workspace_id: Option<Uuid>,
}

/// 404 unless the caller belongs to the workspace they name.
async fn checked_workspace(
    state: &AppState,
    auth: AuthUser,
    workspace_id: Option<Uuid>,
) -> Result<Option<Uuid>, AppError> {
    if let Some(id) = workspace_id {
        member_of(state, auth, id).await?;
    }
    Ok(workspace_id)
}

/// Validated pieces of an [`UpdateLlmSettings`]: the base URL to store and
/// what to do with the key, sealed to `owner` (a user or a workspace).
fn prepare(
    state: &AppState,
    req: &UpdateLlmSettings,
    owner: Uuid,
) -> Result<(Option<String>, KeyUpdate), AppError> {
    let base_url = req
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty());
    if let Some(url) = base_url
        && state.settings.is_production()
    {
        check_production_url(url)?;
    }
    let key = match req.api_key.as_deref() {
        None => KeyUpdate::Keep,
        Some("") => KeyUpdate::Clear,
        Some(key) => KeyUpdate::Set {
            ciphertext: state.secret_box.seal(key.as_bytes(), owner.as_bytes())?,
            hint: key_hint(key),
        },
    };
    Ok((base_url.map(str::to_owned), key))
}

/// The LLM settings that apply to the caller: their own account, else the
/// credential of the workspace they name, else the server defaults.
#[utoipa::path(get, path = "/settings/llm", tag = "settings", security(("bearer" = [])),
    params(LlmScopeQuery),
    responses((status = 200, body = LlmSettings), (status = 401, body = Problem), (status = 404, body = Problem)))]
pub async fn get_llm(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<LlmScopeQuery>,
) -> Result<Json<LlmSettings>, AppError> {
    let workspace = checked_workspace(&state, auth, query.workspace_id).await?;
    Ok(Json(
        credentials::resolve(&state, auth.id, workspace)
            .await?
            .settings(),
    ))
}

/// Connects the caller's own account: provider, model, base URL and
/// (encrypted) API key. It takes precedence over any workspace credential.
#[utoipa::path(put, path = "/settings/llm", tag = "settings", security(("bearer" = [])),
    request_body = UpdateLlmSettings,
    responses((status = 200, body = LlmSettings), (status = 422, body = Problem)))]
pub async fn put_llm(
    State(state): State<AppState>,
    auth: AuthUser,
    ValidatedJson(req): ValidatedJson<UpdateLlmSettings>,
) -> Result<Json<LlmSettings>, AppError> {
    let (base_url, key) = prepare(&state, &req, auth.id)?;
    repo::settings::upsert(
        &state.db,
        auth.id,
        req.provider,
        req.model.trim(),
        base_url.as_deref(),
        key,
    )
    .await?;
    Ok(Json(
        credentials::resolve(&state, auth.id, None)
            .await?
            .settings(),
    ))
}

/// Disconnects the caller's own account, so the workspace's credential (or
/// the server default) applies to their work again.
#[utoipa::path(delete, path = "/settings/llm", tag = "settings", security(("bearer" = [])),
    responses((status = 204, description = "Disconnected (also when nothing was connected)")))]
pub async fn delete_llm(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<StatusCode, AppError> {
    repo::settings::delete(&state.db, auth.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The models the provider behind the caller's effective settings offers
/// right now, newest first. Asked from the provider, never from a built-in list.
#[utoipa::path(get, path = "/settings/llm/models", tag = "settings", security(("bearer" = [])),
    params(LlmScopeQuery),
    responses((status = 200, body = ModelCatalog), (status = 404, body = Problem),
        (status = 422, description = "The provider could not be asked", body = Problem)))]
pub async fn llm_models(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<LlmScopeQuery>,
) -> Result<Json<ModelCatalog>, AppError> {
    let workspace = checked_workspace(&state, auth, query.workspace_id).await?;
    let resolved = credentials::resolve(&state, auth.id, workspace).await?;
    Ok(Json(catalog::list(&state.http, &resolved.target).await?))
}

/// The credential of a workspace, as every member may see it (never the key).
#[utoipa::path(get, path = "/workspaces/{wid}/llm", tag = "settings", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, description = "`null` when the workspace has no credential", body = Option<LlmSettings>),
        (status = 404, body = Problem)))]
pub async fn get_workspace_llm(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Json<Option<LlmSettings>>, AppError> {
    member_of(&state, auth, wid).await?;
    Ok(Json(credentials::workspace_settings(&state, wid).await?))
}

/// Sets the credential members use when they have not connected their own
/// account (workspace admins and owners).
#[utoipa::path(put, path = "/workspaces/{wid}/llm", tag = "settings", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")), request_body = UpdateLlmSettings,
    responses((status = 200, body = LlmSettings), (status = 403, body = Problem),
        (status = 404, body = Problem), (status = 422, body = Problem)))]
pub async fn put_workspace_llm(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<UpdateLlmSettings>,
) -> Result<Json<LlmSettings>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    let (base_url, key) = prepare(&state, &req, wid)?;
    repo::settings::upsert_for_workspace(
        &state.db,
        wid,
        auth.id,
        req.provider,
        req.model.trim(),
        base_url.as_deref(),
        key,
    )
    .await?;
    // The provider and model are noted; the key never is.
    let detail = format!("{} · {}", req.provider, req.model.trim());
    let subject = Subject::Text("Workspace credential");
    audit(
        &state,
        wid,
        auth.id,
        AuditAction::CredentialSet,
        subject,
        &detail,
    )
    .await;
    credentials::workspace_settings(&state, wid)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound("workspace credential"))
}

/// Removes the workspace's credential (workspace admins and owners).
#[utoipa::path(delete, path = "/workspaces/{wid}/llm", tag = "settings", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 204, description = "Removed (also when there was none)"),
        (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn delete_workspace_llm(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    repo::settings::delete_for_workspace(&state.db, wid).await?;
    let subject = Subject::Text("Workspace credential");
    audit(
        &state,
        wid,
        auth.id,
        AuditAction::CredentialRemoved,
        subject,
        "",
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_urls_must_be_public_https() {
        assert!(check_production_url("https://api.example.com/v1").is_ok());
        for bad in [
            "http://api.example.com",
            "https://localhost:11434",
            "https://127.0.0.1",
            "https://10.0.0.5",
            "https://[::1]:8080",
            "https://metadata.internal",
        ] {
            assert!(check_production_url(bad).is_err(), "{bad}");
        }
        assert!(is_http_url("http://localhost:11434/v1"));
        assert!(!is_http_url("file:///etc/passwd"));
    }
}
