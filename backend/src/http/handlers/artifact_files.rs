//! Artifacts kept by hand: a workspace's folders and text files (code, HTML,
//! Markdown) with every version, and the files the runs of an issue's graph
//! produced, which can be kept among the issue's files.
//!
//! A file attached to an issue is seen by whoever sees the issue; any other
//! file, and every folder, by every member but guests. Guests only read.

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::PgExecutor;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::artifacts::attachment;
use super::issues::double_option;
use super::workspaces::{audit, member_of};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::artifact_file::{
    ArtifactContent, ArtifactFile, ArtifactFolder, ArtifactVersion, FILE_MAX_BYTES, FILE_NAME_MAX,
    FOLDER_NAME_MAX, FOLDERS_MAX, HAS_CREDENTIAL, LIST_MAX, NOT_TEXT, RunArtifact, check_name,
    copy_name, extension_of, text_of,
};
use crate::domain::audit::AuditAction;
use crate::domain::guardrails::redact_secrets;
use crate::domain::run::Artifact;
use crate::domain::validation::{FieldErrors, Validate, search_text};
use crate::domain::workspace::Workspace;
use crate::engine::artifacts;
use crate::http::extract::{AuthUser, Path, Query, ValidatedJson};
use crate::http::problem::Problem;
use crate::repo::artifact_files::{FileFilter, NewFile};
use crate::repo::audit::Subject;
use crate::repo::{self, OrNotFound};

/// Most copies of one name tried before a copy needs a name of its own.
const COPIES_MAX: u32 = 1_000;

/// Everyone but guests changes artifacts; guests read what they can see.
fn require_writer(workspace: &Workspace) -> Result<(), AppError> {
    if workspace.role.is_member() {
        Ok(())
    } else {
        Err(AppError::Forbidden("guests can only read artifacts".into()))
    }
}

/// The text of an upload or a new version: at most 1 MiB (413), UTF-8
/// without a NUL byte (415), and without a credential in it (422).
fn checked_text(bytes: &[u8]) -> Result<&str, AppError> {
    if bytes.len() > FILE_MAX_BYTES {
        return Err(AppError::PayloadTooLarge);
    }
    let text = text_of(bytes).ok_or_else(|| AppError::UnsupportedMediaType(NOT_TEXT.into()))?;
    if redact_secrets(text).1 > 0 {
        return Err(AppError::Unprocessable(HAS_CREDENTIAL.into()));
    }
    Ok(text)
}

/// A file name, trimmed: of the right shape (422) and a text file's (415).
fn checked_file_name(raw: &str) -> Result<String, AppError> {
    let name =
        check_name(raw, FILE_NAME_MAX).map_err(|message| AppError::field("name", message))?;
    if extension_of(&name).is_none() {
        return Err(AppError::UnsupportedMediaType(NOT_TEXT.into()));
    }
    Ok(name)
}

fn sha256_hex(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

/// What a failed write of a file of an issue (or of none) tells: its name is
/// taken by a file of the same issue in that folder (409), or the folder or
/// the issue went in the meantime (404).
fn file_not_saved(on_issue: bool) -> impl FnOnce(sqlx::Error) -> AppError {
    move |err| match &err {
        sqlx::Error::Database(db) if db.is_unique_violation() => AppError::Conflict(
            if on_issue {
                "the issue already has a file with that name in this folder"
            } else {
                "a file with that name is already in this folder"
            }
            .into(),
        ),
        sqlx::Error::Database(db) if db.constraint() == Some("artifact_files_folder_id_fkey") => {
            AppError::NotFound("folder")
        }
        sqlx::Error::Database(db) if db.constraint() == Some("artifact_files_issue_id_fkey") => {
            AppError::NotFound("issue")
        }
        _ => err.into(),
    }
}

/// A folder delete that Postgres stopped because it crossed another write
/// on the same rows (a deadlock) is a 409 to try again, not a 500.
fn crossed(err: sqlx::Error) -> AppError {
    match &err {
        sqlx::Error::Database(db) if db.code().as_deref() == Some("40P01") => AppError::Conflict(
            "something under this folder changed while it was deleted; try again".into(),
        ),
        _ => err.into(),
    }
}

fn folder_name_taken(err: sqlx::Error) -> AppError {
    match &err {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            AppError::Conflict("a folder with that name is already there".into())
        }
        _ => err.into(),
    }
}

/// Checks that `folder_id`, when given, is a folder of the workspace.
async fn check_folder(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    folder_id: Option<Uuid>,
    field: &str,
) -> Result<(), AppError> {
    if let Some(id) = folder_id
        && repo::artifact_files::find_folder(db, workspace_id, id)
            .await?
            .is_none()
    {
        return Err(AppError::field(field, "unknown folder in this workspace"));
    }
    Ok(())
}

/// Checks that `issue_id`, when given, is an issue of the workspace the
/// caller can see.
async fn check_issue(
    state: &AppState,
    auth: AuthUser,
    workspace_id: Uuid,
    issue_id: Option<Uuid>,
) -> Result<(), AppError> {
    let Some(id) = issue_id else {
        return Ok(());
    };
    match repo::issues::find(&state.db, auth.id, id).await? {
        Some(issue) if issue.workspace_id == workspace_id => Ok(()),
        _ => Err(AppError::field(
            "issue_id",
            "unknown issue in this workspace",
        )),
    }
}

/// A file the caller can read.
async fn readable(state: &AppState, auth: AuthUser, id: Uuid) -> Result<ArtifactFile, AppError> {
    repo::artifact_files::find_file(&state.db, auth.id, id)
        .await
        .or_not_found("file")
}

/// A file the caller can read and change.
async fn writable(state: &AppState, auth: AuthUser, id: Uuid) -> Result<ArtifactFile, AppError> {
    let file = readable(state, auth, id).await?;
    require_writer(&member_of(state, auth, file.workspace_id).await?)?;
    Ok(file)
}

/// The first name free in a folder for a file of an issue (or of none):
/// `name` itself (unless only a copy's name will do), else its first copy
/// name no file of that issue there has taken.
async fn free_name(
    state: &AppState,
    workspace_id: Uuid,
    folder_id: Option<Uuid>,
    issue_id: Option<Uuid>,
    name: &str,
    copies_only: bool,
) -> Result<String, AppError> {
    let taken =
        repo::artifact_files::names_in(&state.db, workspace_id, folder_id, issue_id).await?;
    if !copies_only && !taken.contains(&name.to_lowercase()) {
        return Ok(name.to_owned());
    }
    (1..=COPIES_MAX)
        .map(|n| copy_name(name, n))
        .find(|copy| !taken.contains(&copy.to_lowercase()))
        .ok_or_else(|| AppError::Conflict("too many copies of this file; name the copy".into()))
}

// ---------- folders ----------

/// `POST /workspaces/{wid}/artifact-folders` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateFolder {
    /// 1-120 characters, without `/`, `\` or control characters; unique among its siblings.
    pub name: String,
    /// The folder to put it in (default: the top level).
    pub parent_id: Option<Uuid>,
}

impl Validate for CreateFolder {
    fn validate(&self, errors: &mut FieldErrors) {
        if let Err(message) = check_name(&self.name, FOLDER_NAME_MAX) {
            errors.add("name", message);
        }
    }
}

/// `PATCH /workspaces/{wid}/artifact-folders/{fid}` body: any subset.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateFolder {
    pub name: Option<String>,
    /// The folder to move it into; `null` moves it to the top level.
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<Uuid>, nullable)]
    pub parent_id: Option<Option<Uuid>>,
}

impl Validate for UpdateFolder {
    fn validate(&self, errors: &mut FieldErrors) {
        if let Some(name) = &self.name
            && let Err(message) = check_name(name, FOLDER_NAME_MAX)
        {
            errors.add("name", message);
        }
    }
}

/// The folders of a workspace, flat and by name: the client builds the
/// tree from `parent_id`. Guests see none.
#[utoipa::path(get, path = "/workspaces/{wid}/artifact-folders", tag = "artifact-files", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, body = [ArtifactFolder]), (status = 404, body = Problem)))]
pub async fn list_folders(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Json<Vec<ArtifactFolder>>, AppError> {
    if !member_of(&state, auth, wid).await?.role.is_member() {
        return Ok(Json(Vec::new()));
    }
    Ok(Json(repo::artifact_files::folders(&state.db, wid).await?))
}

/// Adds a folder (everyone but guests).
#[utoipa::path(post, path = "/workspaces/{wid}/artifact-folders", tag = "artifact-files", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")), request_body = CreateFolder,
    responses((status = 201, body = ArtifactFolder), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "Name taken", body = Problem), (status = 422, body = Problem)))]
pub async fn create_folder(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<CreateFolder>,
) -> Result<(StatusCode, Json<ArtifactFolder>), AppError> {
    require_writer(&member_of(&state, auth, wid).await?)?;
    let mut tx = state.db.begin().await?;
    repo::artifact_files::lock_folders(&mut *tx, wid).await?;
    if repo::artifact_files::count_folders(&mut *tx, wid).await? >= FOLDERS_MAX {
        return Err(AppError::Unprocessable(format!(
            "a workspace has at most {FOLDERS_MAX} folders"
        )));
    }
    check_folder(&mut *tx, wid, req.parent_id, "parent_id").await?;
    let folder =
        repo::artifact_files::create_folder(&mut *tx, wid, req.parent_id, req.name.trim(), auth.id)
            .await
            .map_err(folder_name_taken)?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(folder)))
}

/// Renames a folder or moves it (everyone but guests); never into itself or
/// a folder under it.
#[utoipa::path(patch, path = "/workspaces/{wid}/artifact-folders/{fid}", tag = "artifact-files", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("fid" = Uuid, Path, description = "Folder id")),
    request_body = UpdateFolder,
    responses((status = 200, body = ArtifactFolder), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "Name taken", body = Problem), (status = 422, body = Problem)))]
pub async fn update_folder(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, fid)): Path<(Uuid, Uuid)>,
    ValidatedJson(req): ValidatedJson<UpdateFolder>,
) -> Result<Json<ArtifactFolder>, AppError> {
    require_writer(&member_of(&state, auth, wid).await?)?;
    let mut tx = state.db.begin().await?;
    repo::artifact_files::lock_folders(&mut *tx, wid).await?;
    let mut folder = repo::artifact_files::find_folder(&mut *tx, wid, fid)
        .await
        .or_not_found("folder")?;
    if let Some(Some(parent)) = req.parent_id {
        check_folder(&mut *tx, wid, Some(parent), "parent_id").await?;
        if repo::artifact_files::folder_is_within(&mut *tx, fid, parent).await? {
            return Err(AppError::field(
                "parent_id",
                "a folder cannot be moved into itself or a folder under it",
            ));
        }
    }
    if let Some(name) = req.name {
        folder.name = name.trim().to_owned();
    }
    if let Some(parent) = req.parent_id {
        folder.parent_id = parent;
    }
    let folder = repo::artifact_files::save_folder(&mut *tx, &folder)
        .await
        .map_err(folder_name_taken)?;
    tx.commit().await?;
    Ok(Json(folder))
}

/// Deletes a folder with the folders and files in it (everyone but guests).
/// The issues its files were on note that they went. Refused while it holds
/// files the caller cannot see. Audited as `artifact_folder_deleted`.
#[utoipa::path(delete, path = "/workspaces/{wid}/artifact-folders/{fid}", tag = "artifact-files", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("fid" = Uuid, Path, description = "Folder id")),
    responses((status = 204, description = "Deleted"), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "It holds files of issues the caller cannot see, or crossed another change to it (try again)", body = Problem)))]
pub async fn delete_folder(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, fid)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    require_writer(&member_of(&state, auth, wid).await?)?;
    let mut tx = state.db.begin().await?;
    repo::artifact_files::lock_folders(&mut *tx, wid).await?;
    let folder = repo::artifact_files::find_folder(&mut *tx, wid, fid)
        .await
        .or_not_found("folder")?;
    // A file put or moved into it meanwhile is counted, or waits and then finds it gone; a
    // team or issue delete that would take or leave its files goes first or waits.
    repo::artifact_files::lock_under(&mut tx, fid)
        .await
        .map_err(crossed)?;
    // Refused without saying how many: what the caller cannot see is not counted out to them.
    if repo::artifact_files::hidden_files_under(&mut *tx, auth.id, fid).await? > 0 {
        return Err(AppError::Conflict(
            "this folder holds files of issues you cannot see; a workspace admin can delete it"
                .into(),
        ));
    }
    for (issue_id, name, version) in repo::artifact_files::issue_files_under(&mut *tx, fid).await? {
        let from = version.to_string();
        repo::issues::record_artifact(&mut *tx, issue_id, auth.id, &name, Some(&from), "deleted")
            .await
            .map_err(crossed)?;
    }
    repo::artifact_files::delete_folder(&mut *tx, fid)
        .await
        .map_err(crossed)?;
    tx.commit().await?;
    let subject = Subject::Text(&folder.name);
    audit(
        &state,
        wid,
        auth.id,
        AuditAction::ArtifactFolderDeleted,
        subject,
        "",
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- files ----------

/// Query of `GET /workspaces/{wid}/artifact-files`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct FileQuery {
    /// Only the files directly in this folder.
    pub folder_id: Option<Uuid>,
    /// `true`: only the files at the top level, in no folder.
    pub root: Option<bool>,
    /// Only the files attached to this issue, in any folder.
    pub issue_id: Option<Uuid>,
    /// Part of the name, any case; without control characters.
    pub q: Option<String>,
}

/// Query of `POST /workspaces/{wid}/artifact-files`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct UploadQuery {
    /// The file's name with its extension, which must be a text file's.
    pub name: String,
    /// The folder to put it in (default: the top level).
    pub folder_id: Option<Uuid>,
    /// An issue of the workspace to attach it to.
    pub issue_id: Option<Uuid>,
}

/// Query of the reads of one version.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct VersionQuery {
    /// The version to read (default: the current one).
    pub version: Option<i32>,
}

/// Query of `PUT /artifact-files/{id}/content`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct ContentQuery {
    /// The version the new content was made from: it must still be the current one.
    pub base_version: i32,
}

/// `PATCH /artifact-files/{id}` body: any subset.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateFile {
    /// A new name; its extension must be a text file's.
    pub name: Option<String>,
    /// The folder to move it into; `null` moves it to the top level.
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<Uuid>, nullable)]
    pub folder_id: Option<Option<Uuid>>,
    /// An issue of the workspace to attach it to; `null` detaches it.
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<Uuid>, nullable)]
    pub issue_id: Option<Option<Uuid>>,
}

impl Validate for UpdateFile {
    fn validate(&self, errors: &mut FieldErrors) {
        if let Some(name) = &self.name
            && let Err(message) = check_name(name, FILE_NAME_MAX)
        {
            errors.add("name", message);
        }
    }
}

/// `POST /artifact-files/{id}/duplicate` body.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DuplicateFile {
    /// The folder to put the copy in (default: the file's own); `null` for the top level.
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<Uuid>, nullable)]
    pub folder_id: Option<Option<Uuid>>,
    /// The copy's name (default: the file's with ` copy`, ` copy 2`… before the extension).
    pub name: Option<String>,
}

impl Validate for DuplicateFile {
    fn validate(&self, errors: &mut FieldErrors) {
        if let Some(name) = &self.name
            && let Err(message) = check_name(name, FILE_NAME_MAX)
        {
            errors.add("name", message);
        }
    }
}

/// `POST /issues/{iid}/run-artifacts/{aid}/keep` body.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct KeepArtifact {
    /// The folder to put the file in (default: the top level).
    pub folder_id: Option<Uuid>,
}

impl Validate for KeepArtifact {
    fn validate(&self, _errors: &mut FieldErrors) {}
}

/// Files of a workspace the caller can see: those directly in a folder, at
/// the top level, or of an issue, by name; or every one, most recently
/// saved first. At most 1,000.
#[utoipa::path(get, path = "/workspaces/{wid}/artifact-files", tag = "artifact-files", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), FileQuery),
    responses((status = 200, body = [ArtifactFile]), (status = 404, body = Problem), (status = 422, body = Problem)))]
pub async fn list_files(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    Query(query): Query<FileQuery>,
) -> Result<Json<Vec<ArtifactFile>>, AppError> {
    member_of(&state, auth, wid).await?;
    let root = query.root.unwrap_or(false);
    if root && query.folder_id.is_some() {
        return Err(AppError::field(
            "root",
            "give either a folder or root=true, not both",
        ));
    }
    let folder = match query.folder_id {
        Some(id) => Some(Some(id)),
        None if root => Some(None),
        None => None,
    };
    let filter = FileFilter {
        by_name: folder.is_some() || query.issue_id.is_some(),
        folder,
        issue_id: query.issue_id,
        q: search_text("q", query.q, 200)?,
        limit: LIST_MAX,
    };
    Ok(Json(
        repo::artifact_files::files(&state.db, auth.id, wid, &filter).await?,
    ))
}

/// Adds a text file (everyone but guests). The body is the file itself, at
/// most 1 MiB of UTF-8 (empty for a new, empty file); it becomes version 1.
/// Only code, markup, configuration and text files are taken (415), and
/// never one that carries a credential (422).
#[utoipa::path(post, path = "/workspaces/{wid}/artifact-files", tag = "artifact-files", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), UploadQuery),
    request_body(content = Vec<u8>, content_type = "application/octet-stream",
        description = "The file's bytes (`text/plain` works as well)"),
    responses((status = 201, body = ArtifactFile), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "The name is taken in that folder", body = Problem),
        (status = 413, body = Problem), (status = 415, description = "Not a text file", body = Problem),
        (status = 422, body = Problem)))]
pub async fn create_file(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    Query(query): Query<UploadQuery>,
    body: Bytes,
) -> Result<(StatusCode, Json<ArtifactFile>), AppError> {
    require_writer(&member_of(&state, auth, wid).await?)?;
    let name = checked_file_name(&query.name)?;
    let text = checked_text(&body)?;
    check_folder(&state.db, wid, query.folder_id, "folder_id").await?;
    check_issue(&state, auth, wid, query.issue_id).await?;
    let sha256 = sha256_hex(text);
    let new = NewFile {
        workspace_id: wid,
        folder_id: query.folder_id,
        issue_id: query.issue_id,
        name: &name,
        content: text,
        sha256: &sha256,
        duplicated_from: None,
        created_by: auth.id,
    };
    let id = store(&state, &new, auth, &name, "uploaded").await?;
    Ok((StatusCode::CREATED, Json(readable(&state, auth, id).await?)))
}

/// Inserts a file and notes on its issue, if it has one, what was done to
/// which file (`noted`: the new file's name, or the one it was copied from).
async fn store(
    state: &AppState,
    new: &NewFile<'_>,
    auth: AuthUser,
    noted: &str,
    action: &str,
) -> Result<Uuid, AppError> {
    let mut tx = state.db.begin().await?;
    let id = repo::artifact_files::create_file(&mut tx, new)
        .await
        .map_err(file_not_saved(new.issue_id.is_some()))?;
    if let Some(issue) = new.issue_id {
        repo::issues::record_artifact(&mut *tx, issue, auth.id, noted, None, action).await?;
    }
    tx.commit().await?;
    Ok(id)
}

/// One file, as of its current version.
#[utoipa::path(get, path = "/artifact-files/{id}", tag = "artifact-files", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "File id")),
    responses((status = 200, body = ArtifactFile), (status = 404, body = Problem)))]
pub async fn get_file(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<ArtifactFile>, AppError> {
    Ok(Json(readable(&state, auth, id).await?))
}

/// The versions of a file, newest first.
#[utoipa::path(get, path = "/artifact-files/{id}/versions", tag = "artifact-files", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "File id")),
    responses((status = 200, body = [ArtifactVersion]), (status = 404, body = Problem)))]
pub async fn versions(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<ArtifactVersion>>, AppError> {
    let file = readable(&state, auth, id).await?;
    Ok(Json(
        repo::artifact_files::versions(&state.db, file.id).await?,
    ))
}

/// The text of one version of a file (the current one by default).
#[utoipa::path(get, path = "/artifact-files/{id}/content", tag = "artifact-files", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "File id"), VersionQuery),
    responses((status = 200, body = ArtifactContent), (status = 404, body = Problem)))]
pub async fn content(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(query): Query<VersionQuery>,
) -> Result<Json<ArtifactContent>, AppError> {
    let file = readable(&state, auth, id).await?;
    Ok(Json(
        repo::artifact_files::content(&state.db, file.id, query.version)
            .await
            .or_not_found("version")?,
    ))
}

/// Downloads one version of a file (the current one by default) as an
/// attachment of plain text; it is never served to be shown, so HTML in it
/// does not run.
#[utoipa::path(get, path = "/artifact-files/{id}/raw", tag = "artifact-files", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "File id"), VersionQuery),
    responses(
        (status = 200, description = "The text, as an attachment", content_type = "text/plain", body = String),
        (status = 404, body = Problem),
    ))]
pub async fn raw(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(query): Query<VersionQuery>,
) -> Result<Response, AppError> {
    let file = readable(&state, auth, id).await?;
    let content = repo::artifact_files::content(&state.db, file.id, query.version)
        .await
        .or_not_found("version")?;
    let headers = [
        (
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        ),
        (header::CONTENT_DISPOSITION, attachment(&file.name)),
        (
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ),
    ];
    Ok((headers, content.content).into_response())
}

/// Saves a new version of a file (everyone but guests who can see it). The
/// body is the whole new text; `base_version` is the version it was made
/// from, and someone else's save since then is a 409. Content equal to the
/// current version's saves nothing and answers the file as it is.
#[utoipa::path(put, path = "/artifact-files/{id}/content", tag = "artifact-files", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "File id"), ContentQuery),
    request_body(content = Vec<u8>, content_type = "application/octet-stream",
        description = "The new text (`text/plain` works as well)"),
    responses((status = 200, body = ArtifactFile), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "Someone else saved a version first", body = Problem),
        (status = 413, body = Problem), (status = 415, description = "Not text", body = Problem),
        (status = 422, body = Problem)))]
pub async fn update_content(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(query): Query<ContentQuery>,
    body: Bytes,
) -> Result<Json<ArtifactFile>, AppError> {
    let file = writable(&state, auth, id).await?;
    let text = checked_text(&body)?;
    let stale = |saved: i32| {
        AppError::Conflict(format!(
            "someone else saved v{saved} first; reload it and save again"
        ))
    };
    if query.base_version != file.current_version {
        return Err(stale(file.current_version));
    }
    let sha256 = sha256_hex(text);
    if sha256 == file.sha256 {
        return Ok(Json(file));
    }
    let mut tx = state.db.begin().await?;
    let version = repo::artifact_files::add_version(
        &mut tx,
        file.id,
        query.base_version,
        text,
        &sha256,
        auth.id,
    )
    .await?
    .ok_or_else(|| stale(query.base_version + 1))?;
    if let Some(issue) = &file.issue {
        let from = query.base_version.to_string();
        let to = format!("updated:v{version}");
        repo::issues::record_artifact(&mut *tx, issue.id, auth.id, &file.name, Some(&from), &to)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(readable(&state, auth, id).await?))
}

/// Renames a file, moves it to another folder, or attaches it to another
/// issue or none (everyone but guests who can see it).
#[utoipa::path(patch, path = "/artifact-files/{id}", tag = "artifact-files", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "File id")), request_body = UpdateFile,
    responses((status = 200, body = ArtifactFile), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "The name is taken in that folder", body = Problem),
        (status = 415, description = "Not a text file's name", body = Problem), (status = 422, body = Problem)))]
pub async fn update_file(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<UpdateFile>,
) -> Result<Json<ArtifactFile>, AppError> {
    let file = writable(&state, auth, id).await?;
    let name = match &req.name {
        Some(name) => checked_file_name(name)?,
        None => file.name.clone(),
    };
    let folder_id = req.folder_id.unwrap_or(file.folder_id);
    check_folder(&state.db, file.workspace_id, folder_id, "folder_id").await?;
    let issue_id = match req.issue_id {
        Some(issue) => {
            check_issue(&state, auth, file.workspace_id, issue).await?;
            issue
        }
        None => file.issue.as_ref().map(|i| i.id),
    };
    repo::artifact_files::save_file(&state.db, file.id, &name, folder_id, issue_id)
        .await
        .map_err(file_not_saved(issue_id.is_some()))?;
    Ok(Json(readable(&state, auth, id).await?))
}

/// Copies a file's current version into a new file of the same issue
/// (everyone but guests who can see it): in the same folder unless another
/// is given, named with ` copy` (` copy 2`…) unless a name is given.
#[utoipa::path(post, path = "/artifact-files/{id}/duplicate", tag = "artifact-files", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "File id")), request_body = DuplicateFile,
    responses((status = 201, body = ArtifactFile), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "The name is taken in that folder", body = Problem),
        (status = 415, description = "Not a text file's name", body = Problem), (status = 422, body = Problem)))]
pub async fn duplicate(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<DuplicateFile>,
) -> Result<(StatusCode, Json<ArtifactFile>), AppError> {
    let source = writable(&state, auth, id).await?;
    let folder_id = req.folder_id.unwrap_or(source.folder_id);
    check_folder(&state.db, source.workspace_id, folder_id, "folder_id").await?;
    let issue_id = source.issue.as_ref().map(|i| i.id);
    let name = match &req.name {
        Some(name) => checked_file_name(name)?,
        None => {
            free_name(
                &state,
                source.workspace_id,
                folder_id,
                issue_id,
                &source.name,
                true,
            )
            .await?
        }
    };
    let content = repo::artifact_files::content(&state.db, source.id, None)
        .await
        .or_not_found("version")?;
    let sha256 = sha256_hex(&content.content);
    let new = NewFile {
        workspace_id: source.workspace_id,
        folder_id,
        issue_id,
        name: &name,
        content: &content.content,
        sha256: &sha256,
        duplicated_from: Some(source.id),
        created_by: auth.id,
    };
    let copy = store(&state, &new, auth, &source.name, "duplicated").await?;
    Ok((
        StatusCode::CREATED,
        Json(readable(&state, auth, copy).await?),
    ))
}

/// Deletes a file with its versions (everyone but guests who can see it).
/// Audited as `artifact_file_deleted`.
#[utoipa::path(delete, path = "/artifact-files/{id}", tag = "artifact-files", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "File id")),
    responses((status = 204, description = "Deleted"), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn delete_file(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let file = writable(&state, auth, id).await?;
    let version = file.current_version.to_string();
    let mut tx = state.db.begin().await?;
    if let Some(issue) = &file.issue {
        repo::issues::record_artifact(
            &mut *tx,
            issue.id,
            auth.id,
            &file.name,
            Some(&version),
            "deleted",
        )
        .await?;
    }
    repo::artifact_files::delete_file(&mut *tx, file.id).await?;
    tx.commit().await?;
    let detail = match &file.issue {
        Some(issue) => format!("v{version} on {}", issue.identifier),
        None => format!("v{version}"),
    };
    let subject = Subject::Text(&file.name);
    audit(
        &state,
        file.workspace_id,
        auth.id,
        AuditAction::ArtifactFileDeleted,
        subject,
        &detail,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- what runs produced ----------

/// The files the runs of the issue's graph produced, newest run first, with
/// the run each came from; empty when the issue has no graph or the caller
/// cannot read its runs. They download from `/artifacts/{aid}/download`.
#[utoipa::path(get, path = "/issues/{iid}/run-artifacts", tag = "artifact-files", security(("bearer" = [])),
    params(("iid" = Uuid, Path, description = "Issue id")),
    responses((status = 200, body = [RunArtifact]), (status = 404, body = Problem)))]
pub async fn run_artifacts(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(iid): Path<Uuid>,
) -> Result<Json<Vec<RunArtifact>>, AppError> {
    let issue = repo::issues::find(&state.db, auth.id, iid)
        .await
        .or_not_found("issue")?;
    let Some(graph_id) = issue.graph_id else {
        return Ok(Json(Vec::new()));
    };
    Ok(Json(
        repo::artifacts::of_graph_runs(&state.db, auth.id, graph_id).await?,
    ))
}

/// Keeps a file a run of the issue's graph produced among the issue's files
/// (everyone but guests who can see the issue and its runs), named after
/// the file (with ` copy` when the name is taken). Only a text file of at
/// most 1 MiB can be kept, and not while it is quarantined.
#[utoipa::path(post, path = "/issues/{iid}/run-artifacts/{aid}/keep", tag = "artifact-files", security(("bearer" = [])),
    params(("iid" = Uuid, Path, description = "Issue id"), ("aid" = Uuid, Path, description = "Run artifact id")),
    request_body = KeepArtifact,
    responses((status = 201, body = ArtifactFile), (status = 403, description = "Guest, or quarantined", body = Problem),
        (status = 404, body = Problem), (status = 413, body = Problem),
        (status = 415, description = "Not a text file", body = Problem), (status = 422, body = Problem)))]
pub async fn keep(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((iid, aid)): Path<(Uuid, Uuid)>,
    ValidatedJson(req): ValidatedJson<KeepArtifact>,
) -> Result<(StatusCode, Json<ArtifactFile>), AppError> {
    let issue = repo::issues::find(&state.db, auth.id, iid)
        .await
        .or_not_found("issue")?;
    require_writer(&member_of(&state, auth, issue.workspace_id).await?)?;
    let graph_id = issue.graph_id.ok_or(AppError::NotFound("artifact"))?;
    let stored = repo::artifacts::find_of_graph(&state.db, auth.id, graph_id, aid)
        .await
        .or_not_found("artifact")?;
    let artifact: Artifact = stored.artifact.clone().into();
    if !artifact.is_downloadable() {
        return Err(AppError::Forbidden(format!(
            "the artifact is quarantined until an admin releases it ({})",
            artifact.quarantine_reason
        )));
    }
    let base = artifact.path.rsplit('/').next().unwrap_or_default();
    let base = checked_file_name(base)?;
    if usize::try_from(artifact.size).unwrap_or(usize::MAX) > FILE_MAX_BYTES {
        return Err(AppError::PayloadTooLarge);
    }
    check_folder(&state.db, issue.workspace_id, req.folder_id, "folder_id").await?;
    let path = artifacts::resolve(&state, &stored.storage_path)?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| AppError::NotFound("artifact"))?;
    let text = checked_text(&bytes)?;
    let name = free_name(
        &state,
        issue.workspace_id,
        req.folder_id,
        Some(issue.id),
        &base,
        false,
    )
    .await?;
    let sha256 = sha256_hex(text);
    let new = NewFile {
        workspace_id: issue.workspace_id,
        folder_id: req.folder_id,
        issue_id: Some(issue.id),
        name: &name,
        content: text,
        sha256: &sha256,
        duplicated_from: None,
        created_by: auth.id,
    };
    let id = store(&state, &new, auth, &name, "uploaded").await?;
    Ok((StatusCode::CREATED, Json(readable(&state, auth, id).await?)))
}
