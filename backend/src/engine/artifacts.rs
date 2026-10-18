//! Artifact storage under `<data>/artifacts/<run_id>/<node_id>/<write_id>/<path>`.
//!
//! Paths come from LLM / agent output and are therefore untrusted: they must
//! be relative, made of plain file-name characters, contain no `..` and
//! stay inside the node's directory after resolution.
//!
//! Every write has a file of its own, and the artifact's record is pointed
//! at it in a transaction fenced by the run's lease (docs/HA.md): a former
//! holder of the run that writes late never replaces the bytes or the record
//! the holder wrote.

use std::path::{Component, Path, PathBuf};

use uuid::Uuid;

use super::lease::{self, Lease, WriteError};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::run::Artifact;
use crate::realtime::events::SseEvent;
use crate::repo;

/// Largest artifact accepted (contract §8).
pub const MAX_ARTIFACT_BYTES: usize = 20 * 1024 * 1024;
const MAX_PATH_LEN: usize = 200;
const MAX_DEPTH: usize = 6;

/// Validates an untrusted relative artifact path and normalises it.
pub fn sanitize_path(raw: &str) -> Result<String, String> {
    if raw.is_empty() || raw.len() > MAX_PATH_LEN {
        return Err("artifact path must be 1-200 characters".into());
    }
    if raw.contains('\\') || raw.contains('\0') {
        return Err("artifact path contains forbidden characters".into());
    }
    let mut parts = Vec::new();
    for component in Path::new(raw).components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_str().ok_or("artifact path is not UTF-8")?;
                let ok_chars = part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ' '));
                if part.starts_with('.') || !ok_chars {
                    return Err(format!("invalid artifact path component `{part}`"));
                }
                parts.push(part);
            }
            Component::CurDir => {}
            _ => return Err("artifact path must be relative and must not contain `..`".into()),
        }
    }
    if parts.is_empty() || parts.len() > MAX_DEPTH {
        return Err("artifact path has no file name or is nested too deeply".into());
    }
    Ok(parts.join("/"))
}

/// MIME type for a path when the producer did not provide a usable one.
pub fn mime_for(path: &str, declared: Option<&str>) -> String {
    match declared.map(str::trim) {
        Some(m)
            if !m.is_empty() && m.len() < 128 && m.contains('/') && !m.contains(['\r', '\n']) =>
        {
            m.to_owned()
        }
        _ => mime_guess::from_path(path)
            .first_or_octet_stream()
            .essence_str()
            .to_owned(),
    }
}

/// Where one write of an artifact is kept, relative to the artifacts directory.
fn storage_rel(run_id: Uuid, node_id: Uuid, write_id: Uuid, path: &str) -> String {
    format!("{run_id}/{node_id}/{write_id}/{path}")
}

/// An artifact as stored: its record and where its bytes are.
#[derive(Debug, Clone)]
pub struct Stored {
    pub artifact: Artifact,
    /// Relative to the artifacts directory.
    pub storage_path: String,
}

/// Removes the stored files of runs (each run has its own directory). Called
/// after the runs' rows are gone: a file that cannot be removed is logged,
/// not an error, because nothing can be rolled back any more.
pub async fn remove_runs(state: &AppState, run_ids: &[Uuid]) {
    let root = state.settings.artifacts_dir();
    for run_id in run_ids {
        let dir = root.join(run_id.to_string());
        match tokio::fs::remove_dir_all(&dir).await {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => tracing::warn!(%run_id, error = %err, "cannot remove a run's artifacts"),
        }
    }
}

/// Absolute location of a stored artifact, verified to be inside the
/// artifacts directory.
pub fn resolve(state: &AppState, storage_path: &str) -> Result<PathBuf, AppError> {
    let root = state.settings.artifacts_dir();
    let rel = storage_path.split('/').collect::<Vec<_>>();
    if rel.iter().any(|p| p.is_empty() || *p == ".." || *p == ".") {
        return Err(AppError::NotFound("artifact"));
    }
    let full = root.join(storage_path);
    let canonical_root = root
        .canonicalize()
        .map_err(|_| AppError::NotFound("artifact"))?;
    let canonical = full
        .canonicalize()
        .map_err(|_| AppError::NotFound("artifact"))?;
    if !canonical.starts_with(&canonical_root) {
        return Err(AppError::NotFound("artifact"));
    }
    Ok(canonical)
}

/// Writes an artifact of a node of a run executed here under `lease`,
/// records it and publishes `artifact.created`. The bytes go to a file of
/// their own; the record is pointed at it in a transaction fenced by the
/// lease, and the file it pointed at before is removed after. A write the
/// fence refuses (another instance executes the run now) leaves nothing.
#[allow(clippy::too_many_arguments)]
pub async fn store(
    state: &AppState,
    lease: &Lease,
    graph_id: Uuid,
    run_id: Uuid,
    node_id: Uuid,
    raw_path: &str,
    declared_mime: Option<&str>,
    bytes: &[u8],
) -> Result<Stored, String> {
    if bytes.len() > MAX_ARTIFACT_BYTES {
        return Err(format!(
            "artifact exceeds {} MiB",
            MAX_ARTIFACT_BYTES / 1024 / 1024
        ));
    }
    let path = sanitize_path(raw_path)?;
    if lease.is_lost() {
        return Err(REFUSED.into());
    }
    let rel = storage_rel(run_id, node_id, Uuid::now_v7(), &path);
    let full = state.settings.artifacts_dir().join(&rel);
    write_atomic(&full, bytes)
        .await
        .map_err(|e| format!("cannot write artifact: {e}"))?;
    let mime = mime_for(&path, declared_mime);
    let recorded = async {
        let mut tx = lease::begin(state, lease, "artifact").await?;
        let replaced = repo::artifacts::storage_path_of(&mut *tx, run_id, node_id, &path).await?;
        let artifact = repo::artifacts::upsert(
            &mut *tx,
            run_id,
            node_id,
            &path,
            bytes.len() as i64,
            &mime,
            &rel,
        )
        .await?;
        tx.commit().await?;
        Ok::<_, WriteError>((artifact, replaced))
    }
    .await;
    let (artifact, replaced) = match recorded {
        Ok(recorded) => recorded,
        Err(err) => {
            discard(state, &rel).await;
            return Err(match err {
                WriteError::Fenced => REFUSED.into(),
                WriteError::Db(err) => format!("cannot record artifact: {err}"),
            });
        }
    };
    if let Some(replaced) = replaced.filter(|old| *old != rel) {
        discard(state, &replaced).await;
    }
    state.hub.publish(
        graph_id,
        SseEvent::ArtifactCreated {
            artifact: artifact.clone(),
        },
    );
    Ok(Stored {
        artifact,
        storage_path: rel,
    })
}

/// Why a write of a former holder of the run was not kept.
const REFUSED: &str = "artifact not kept: another instance executes the run now";

/// Removes the file of one write (`rel`), with the directories it leaves
/// empty inside its node's directory. Nothing refers to it any more, so a
/// failure is logged, not an error.
async fn discard(state: &AppState, rel: &str) {
    let root = state.settings.artifacts_dir();
    let full = root.join(rel);
    match tokio::fs::remove_file(&full).await {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            tracing::warn!(storage_path = rel, error = %err, "cannot remove an artifact's file");
            return;
        }
    }
    let node_dir = rel
        .splitn(3, '/')
        .take(2)
        .fold(root, |dir, part| dir.join(part));
    let mut dir = full.parent();
    while let Some(d) = dir.filter(|d| *d != node_dir && d.starts_with(&node_dir)) {
        if tokio::fs::remove_dir(d).await.is_err() {
            break;
        }
        dir = d.parent();
    }
}

/// Copies a node's artifacts from an earlier run (cache hits keep their
/// files) into a run executed here under `lease`.
pub async fn copy_from_run(
    state: &AppState,
    lease: &Lease,
    graph_id: Uuid,
    from_run: Uuid,
    to_run: Uuid,
    node_id: Uuid,
) -> anyhow::Result<()> {
    for stored in repo::artifacts::list_for_node(&state.db, from_run, node_id).await? {
        let Ok(source) = resolve(state, &stored.storage_path) else {
            continue;
        };
        let bytes = tokio::fs::read(&source).await?;
        let a = &stored.artifact;
        store(
            state,
            lease,
            graph_id,
            to_run,
            node_id,
            &a.path,
            Some(&a.mime),
            &bytes,
        )
        .await
        .map_err(anyhow::Error::msg)?;
    }
    Ok(())
}

async fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("artifact path has no parent"))?;
    tokio::fs::create_dir_all(dir).await?;
    let tmp = dir.join(format!(".tmp-{}", Uuid::now_v7()));
    tokio::fs::write(&tmp, bytes).await?;
    tokio::fs::rename(&tmp, path).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_safe_relative_paths() {
        assert_eq!(sanitize_path("report.docx").unwrap(), "report.docx");
        assert_eq!(
            sanitize_path("./out/data set_v2.csv").unwrap(),
            "out/data set_v2.csv"
        );
    }

    #[test]
    fn rejects_traversal_and_absolute_paths() {
        for bad in [
            "",
            "../etc/passwd",
            "a/../../b",
            "/etc/passwd",
            "a\\b",
            ".env",
            "a/.git/config",
            "a\0b",
            "x$y",
            "a/b/c/d/e/f/g.txt",
            "./",
        ] {
            assert!(sanitize_path(bad).is_err(), "{bad:?} must be rejected");
        }
        assert!(sanitize_path(&"a".repeat(201)).is_err());
    }

    #[test]
    fn mime_detection() {
        assert_eq!(mime_for("a.md", None), "text/markdown");
        assert_eq!(mime_for("a.bin", Some("")), "application/octet-stream");
        assert_eq!(
            mime_for("a.docx", Some("application/x-custom")),
            "application/x-custom"
        );
        assert_eq!(mime_for("a.txt", Some("text/html\r\nX: y")), "text/plain");
    }
}
