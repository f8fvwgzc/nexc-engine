//! Artifact listing and downloads (single file or zip of a run).

use std::io::Write as _;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use uuid::Uuid;
use zip::write::SimpleFileOptions;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::run::Artifact;
use crate::engine::artifacts;
use crate::http::extract::{AuthUser, Path};
use crate::http::problem::Problem;
use crate::repo::{self, OrNotFound};

/// Largest zip built in memory.
const MAX_ZIP_BYTES: i64 = 256 * 1024 * 1024;

/// `Content-Disposition: attachment` with an ASCII fallback and an RFC 5987 name.
fn attachment(file_name: &str) -> HeaderValue {
    let ascii: String = file_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded: String = file_name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    HeaderValue::from_str(&format!(
        "attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}"
    ))
    .unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}

/// Artifacts of a run.
#[utoipa::path(get, path = "/runs/{rid}/artifacts", tag = "artifacts", security(("bearer" = [])),
    params(("rid" = Uuid, Path, description = "Run id")),
    responses((status = 200, body = [Artifact]), (status = 404, body = Problem)))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(rid): Path<Uuid>,
) -> Result<Json<Vec<Artifact>>, AppError> {
    repo::runs::find_owned(&state.db, auth.id, rid)
        .await
        .or_not_found("run")?;
    let stored = repo::artifacts::list_stored(&state.db, rid).await?;
    Ok(Json(
        stored.into_iter().map(|s| s.artifact.into()).collect(),
    ))
}

/// All artifacts of a run as a zip (`<node_id>/<path>` entries).
#[utoipa::path(get, path = "/runs/{rid}/artifacts.zip", tag = "artifacts", security(("bearer" = [])),
    params(("rid" = Uuid, Path, description = "Run id")),
    responses(
        (status = 200, description = "Zip archive", content_type = "application/zip", body = Vec<u8>),
        (status = 404, body = Problem),
        (status = 413, description = "Archive too large", body = Problem),
    ))]
pub async fn zip(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(rid): Path<Uuid>,
) -> Result<Response, AppError> {
    repo::runs::find_owned(&state.db, auth.id, rid)
        .await
        .or_not_found("run")?;
    let stored = repo::artifacts::list_stored(&state.db, rid).await?;
    if stored.iter().map(|s| s.artifact.size).sum::<i64>() > MAX_ZIP_BYTES {
        return Err(AppError::PayloadTooLarge);
    }
    let mut files = Vec::with_capacity(stored.len());
    for s in stored {
        let path = artifacts::resolve(&state, &s.storage_path)?;
        let bytes = tokio::fs::read(&path).await.map_err(anyhow::Error::from)?;
        files.push((format!("{}/{}", s.artifact.node_id, s.artifact.path), bytes));
    }
    let archive = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<u8>> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in files {
            writer.start_file(name, options)?;
            writer.write_all(&bytes)?;
        }
        Ok(writer.finish()?.into_inner())
    })
    .await
    .map_err(anyhow::Error::from)??;
    let headers = [
        (
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/zip"),
        ),
        (
            header::CONTENT_DISPOSITION,
            attachment(&format!("run-{rid}.zip")),
        ),
    ];
    Ok((headers, archive).into_response())
}

/// Downloads one artifact as an attachment.
#[utoipa::path(get, path = "/artifacts/{aid}/download", tag = "artifacts", security(("bearer" = [])),
    params(("aid" = Uuid, Path, description = "Artifact id")),
    responses(
        (status = 200, description = "The file", content_type = "application/octet-stream", body = Vec<u8>),
        (status = 404, body = Problem),
    ))]
pub async fn download(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(aid): Path<Uuid>,
) -> Result<Response, AppError> {
    let stored = repo::artifacts::find_owned(&state.db, auth.id, aid)
        .await
        .or_not_found("artifact")?;
    let path = artifacts::resolve(&state, &stored.storage_path)?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| AppError::NotFound("artifact"))?;
    let file_name = stored
        .artifact
        .path
        .rsplit('/')
        .next()
        .unwrap_or("artifact");
    let mime = HeaderValue::from_str(&stored.artifact.mime)
        .unwrap_or(HeaderValue::from_static("application/octet-stream"));
    let headers = [
        (header::CONTENT_TYPE, mime),
        (header::CONTENT_DISPOSITION, attachment(file_name)),
        (
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ),
    ];
    Ok((headers, bytes).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_disposition_is_safe() {
        let v = attachment("rapport \"final\"\r\n.docx");
        let s = v.to_str().unwrap();
        assert!(s.starts_with("attachment; filename=\"rapport__final___.docx\""));
        assert!(!s.contains('\n'));
        assert!(s.contains("filename*=UTF-8''rapport%20%22final%22%0D%0A.docx"));
    }
}
