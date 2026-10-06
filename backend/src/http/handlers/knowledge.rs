//! The knowledge base of a workspace: its documents, the search over them,
//! and how they are embedded and used.

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::workspaces::{audit, member_of, require};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::audit::AuditAction;
use crate::domain::knowledge::{
    DOCUMENT_MAX_BYTES, DOCUMENT_NAME_MAX, DOCUMENTS_MAX, Document, KnowledgeSettings, Passage,
};
use crate::domain::settings::key_hint;
use crate::domain::validation::{FieldErrors, Validate};
use crate::domain::workspace::{Workspace, WorkspaceAction};
use crate::engine::knowledge;
use crate::http::extract::{AuthUser, Path, Query, ValidatedJson};
use crate::http::problem::Problem;
use crate::repo::audit::Subject;
use crate::repo::knowledge::SettingsUpdate;
use crate::repo::settings::KeyUpdate;
use crate::repo::{self, OrNotFound};

/// Everyone but guests reads and adds documents.
fn require_member(workspace: &Workspace) -> Result<(), AppError> {
    if workspace.role.is_member() {
        Ok(())
    } else {
        Err(AppError::Forbidden(
            "guests do not have access to the workspace's documents".into(),
        ))
    }
}

/// Query of `GET /workspaces/{wid}/documents`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct DocumentQuery {
    /// Part of the file name.
    pub q: Option<String>,
    /// 1-100, default 20.
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// One page of a workspace's documents, newest first.
#[utoipa::path(get, path = "/workspaces/{wid}/documents", tag = "knowledge", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), DocumentQuery),
    responses((status = 200, body = [Document]), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    Query(query): Query<DocumentQuery>,
) -> Result<Json<Vec<Document>>, AppError> {
    require_member(&member_of(&state, auth, wid).await?)?;
    let q = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(|q| q.chars().take(100).collect::<String>());
    Ok(Json(
        repo::knowledge::list(
            &state.db,
            wid,
            q.as_deref(),
            query.limit.unwrap_or(20).clamp(1, 100),
            query.offset.unwrap_or(0).clamp(0, 1_000_000),
        )
        .await?,
    ))
}

/// Query of `POST /workspaces/{wid}/documents`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct UploadQuery {
    /// The file's name, with its extension (it decides how the file is read).
    pub name: String,
}

/// Adds a file to the knowledge base. The body is the file itself (at most
/// 50 MiB); it is parsed, split into passages and embedded in the background,
/// and `status` tells how far that is. The same file is stored once: sending
/// it again returns the document it already is.
#[utoipa::path(post, path = "/workspaces/{wid}/documents", tag = "knowledge", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), UploadQuery),
    request_body(content = Vec<u8>, content_type = "application/octet-stream"),
    responses((status = 201, body = Document), (status = 200, description = "Already in the knowledge base", body = Document),
        (status = 403, body = Problem), (status = 404, body = Problem), (status = 413, body = Problem),
        (status = 422, body = Problem)))]
pub async fn upload(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    Query(query): Query<UploadQuery>,
    body: Bytes,
) -> Result<(StatusCode, Json<Document>), AppError> {
    require_member(&member_of(&state, auth, wid).await?)?;
    // Only the last path segment: a name is shown and decides the parser, never a location.
    let name = query
        .name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    if name.is_empty() || name.chars().count() > DOCUMENT_NAME_MAX {
        return Err(AppError::field("name", "must be 1 to 255 characters"));
    }
    if body.is_empty() {
        return Err(AppError::Unprocessable("the file is empty".into()));
    }
    if body.len() > DOCUMENT_MAX_BYTES {
        return Err(AppError::Unprocessable(
            "a document is at most 50 MiB".into(),
        ));
    }
    let sha256 = hex::encode(Sha256::digest(&body));
    if let Some(existing) = repo::knowledge::find_by_hash(&state.db, wid, &sha256).await? {
        return Ok((StatusCode::OK, Json(existing)));
    }
    if repo::knowledge::count(&state.db, wid).await? >= DOCUMENTS_MAX {
        return Err(AppError::Unprocessable(format!(
            "a workspace holds at most {DOCUMENTS_MAX} documents"
        )));
    }
    // The file is on disk before the row that makes a worker look for it.
    let id = Uuid::now_v7();
    let path = state.settings.documents_dir().join(id.to_string());
    tokio::fs::write(&path, &body)
        .await
        .map_err(|err| anyhow::anyhow!("cannot store the upload: {err}"))?;
    let size = i64::try_from(body.len()).unwrap_or(i64::MAX);
    match repo::knowledge::insert(&state.db, id, wid, &name, size, &sha256, auth.id).await? {
        Some(document) => Ok((StatusCode::CREATED, Json(document))),
        None => {
            // Someone sent the same file at the same moment; theirs stands.
            let _ = tokio::fs::remove_file(&path).await;
            let existing = repo::knowledge::find_by_hash(&state.db, wid, &sha256)
                .await
                .or_not_found("document")?;
            Ok((StatusCode::OK, Json(existing)))
        }
    }
}

/// One document.
#[utoipa::path(get, path = "/workspaces/{wid}/documents/{did}", tag = "knowledge", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("did" = Uuid, Path, description = "Document id")),
    responses((status = 200, body = Document), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, did)): Path<(Uuid, Uuid)>,
) -> Result<Json<Document>, AppError> {
    require_member(&member_of(&state, auth, wid).await?)?;
    Ok(Json(
        repo::knowledge::find(&state.db, wid, did)
            .await
            .or_not_found("document")?,
    ))
}

/// Removes a document and its passages: whoever uploaded it, or an admin.
#[utoipa::path(delete, path = "/workspaces/{wid}/documents/{did}", tag = "knowledge", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("did" = Uuid, Path, description = "Document id")),
    responses((status = 204, description = "Deleted"), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, did)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    let workspace = member_of(&state, auth, wid).await?;
    require_member(&workspace)?;
    let document = repo::knowledge::find(&state.db, wid, did)
        .await
        .or_not_found("document")?;
    if document.uploaded_by != Some(auth.id) && !workspace.role.is_admin() {
        return Err(AppError::Forbidden(
            "only whoever uploaded a document, or an admin, removes it".into(),
        ));
    }
    repo::knowledge::delete(&state.db, did).await?;
    let _ = tokio::fs::remove_file(state.settings.documents_dir().join(did.to_string())).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Query of `GET /workspaces/{wid}/knowledge/search`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct SearchQuery {
    pub q: String,
    /// 1-20, default 8.
    pub limit: Option<usize>,
}

/// Searches the workspace's documents: the passages that best answer `q`, by
/// keywords and by the similarity of their embeddings, best first.
#[utoipa::path(get, path = "/workspaces/{wid}/knowledge/search", tag = "knowledge", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), SearchQuery),
    responses((status = 200, body = [Passage]), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn search(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<Passage>>, AppError> {
    require_member(&member_of(&state, auth, wid).await?)?;
    let q: String = query.q.trim().chars().take(1_000).collect();
    let limit = query.limit.unwrap_or(8).clamp(1, 20);
    Ok(Json(knowledge::search(&state, wid, &q, limit).await?))
}

/// How the workspace embeds and uses its documents.
#[utoipa::path(get, path = "/workspaces/{wid}/knowledge/settings", tag = "knowledge", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, body = KnowledgeSettings), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn settings(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Json<KnowledgeSettings>, AppError> {
    require_member(&member_of(&state, auth, wid).await?)?;
    Ok(Json(knowledge::settings(&state, wid).await?.view()))
}

/// `PUT /workspaces/{wid}/knowledge/settings` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateKnowledgeSettings {
    /// An OpenAI-compatible endpoint (`https://api.openai.com/v1`); `null` or
    /// empty uses the server's, else the built-in embedding.
    pub embed_base_url: Option<String>,
    /// Required with `embed_base_url`.
    pub embed_model: Option<String>,
    /// Vector size to ask for, when the model can shorten its vectors (64-4096).
    pub embed_dims: Option<i32>,
    /// Leave out to keep the stored key, send `""` to remove it.
    pub api_key: Option<String>,
    /// 0-20; 0 keeps documents out of prompts.
    pub passages: i32,
    /// 500-40000.
    pub budget_chars: i32,
    pub use_in_nodes: bool,
    pub use_in_plan: bool,
}

impl Validate for UpdateKnowledgeSettings {
    fn validate(&self, errors: &mut FieldErrors) {
        let url = self.embed_base_url.as_deref().map(str::trim).unwrap_or("");
        let model = self.embed_model.as_deref().map(str::trim).unwrap_or("");
        if !url.is_empty() {
            if !(url.starts_with("https://") || url.starts_with("http://")) || url.len() > 500 {
                errors.add("embed_base_url", "must be an http(s) URL");
            }
            if model.is_empty() || model.len() > 200 {
                errors.add("embed_model", "name the embedding model of that endpoint");
            }
        }
        if self.embed_dims.is_some_and(|d| !(64..=4096).contains(&d)) {
            errors.add("embed_dims", "must be between 64 and 4096");
        }
        if !(0..=20).contains(&self.passages) {
            errors.add("passages", "must be between 0 and 20");
        }
        if !(500..=40_000).contains(&self.budget_chars) {
            errors.add("budget_chars", "must be between 500 and 40000");
        }
        if self.api_key.as_deref().is_some_and(|k| k.len() > 500) {
            errors.add("api_key", "is too long");
        }
    }
}

/// Sets how the workspace embeds and uses its documents (admins and owners).
/// When the embedding model changes, ready documents are embedded again in
/// the background; until then they are found by keywords.
#[utoipa::path(put, path = "/workspaces/{wid}/knowledge/settings", tag = "knowledge", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")), request_body = UpdateKnowledgeSettings,
    responses((status = 200, body = KnowledgeSettings), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 422, body = Problem)))]
pub async fn put_settings(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<UpdateKnowledgeSettings>,
) -> Result<Json<KnowledgeSettings>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    let url = req
        .embed_base_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty());
    if let Some(url) = url
        && state.settings.is_production()
        && !url.starts_with("https://")
    {
        return Err(AppError::field(
            "embed_base_url",
            "must be an https URL in production",
        ));
    }
    let model = req
        .embed_model
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty() && url.is_some());
    let key = match req.api_key.as_deref() {
        None => KeyUpdate::Keep,
        Some("") => KeyUpdate::Clear,
        Some(key) => KeyUpdate::Set {
            ciphertext: state.secret_box.seal(key.as_bytes(), wid.as_bytes())?,
            hint: key_hint(key),
        },
    };
    let update = SettingsUpdate {
        embed_base_url: url,
        embed_model: model,
        embed_dims: req.embed_dims.filter(|_| url.is_some()),
        key,
        passages: req.passages,
        budget_chars: req.budget_chars,
        use_in_nodes: req.use_in_nodes,
        use_in_plan: req.use_in_plan,
    };
    repo::knowledge::upsert_settings(&state.db, wid, auth.id, update).await?;
    let resolved = knowledge::settings(&state, wid).await?;
    // Passages embedded with another model cannot be compared with new queries.
    let requeued =
        repo::knowledge::requeue_for_model(&state.db, wid, &resolved.target.model).await?;
    // The endpoint and model are noted; the key never is.
    let detail = format!(
        "{} · {requeued} document(s) to embed again",
        resolved.target.model
    );
    let subject = Subject::Text("Knowledge settings");
    audit(
        &state,
        wid,
        auth.id,
        AuditAction::KnowledgeChanged,
        subject,
        &detail,
    )
    .await;
    Ok(Json(resolved.view()))
}
