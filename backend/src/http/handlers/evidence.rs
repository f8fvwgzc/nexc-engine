//! Evidence that holds without nexc: a run's evidence bundle, the
//! verification of a workspace's audit chain, and the public keys that
//! everything nexc signs verifies with (docs/AGENTSECOPS.md §12).

use axum::Json;
use axum::extract::State;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use super::workspaces::{member_of, require};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::audit::AuditAction;
use crate::domain::audit_chain::{AuditVerification, ChainCheck};
use crate::domain::bundle::{BundleRun, EvidenceBundle};
use crate::domain::workspace::WorkspaceAction;
use crate::http::extract::{AuthUser, Path};
use crate::http::middleware::etag::PUBLIC_CACHE_CONTROL;
use crate::http::problem::Problem;
use crate::repo::{self, OrNotFound, audit::Subject};
use crate::security::signing::Jwks;

/// Audit entries about one run a bundle carries at most (the seal's, the
/// deletion's and the newest when there are more; the manifest says how many).
pub const BUNDLE_AUDIT_MAX: i64 = 10_000;
/// Audit entries read per page while verifying a chain.
const VERIFY_PAGE: i64 = 5000;

/// A run's evidence bundle: every span in chain order, the signed seal, the
/// payloads' metadata (never their content), the workspace's audit entries
/// about the run and its graph as chained, and the backend's signed
/// attestation over all of it. `nexc-verify` (or `make verify-bundle`)
/// checks it with the keys of `/.well-known/jwks.json` alone. For workspace
/// owners and admins (the readers of the audit log), deleted graphs' runs
/// included; the export is itself an `evidence_exported` entry, carried by
/// the bundle.
#[utoipa::path(get, path = "/runs/{rid}/evidence", tag = "runs", security(("bearer" = [])),
    params(("rid" = Uuid, Path, description = "Run id")),
    responses((status = 200, description = "The bundle, as an attachment", body = EvidenceBundle),
        (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn run_evidence(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(rid): Path<Uuid>,
) -> Result<Response, AppError> {
    let (row, graph) = repo::runs::find_evidence(&state.db, auth.id, rid)
        .await
        .or_not_found("run")?;
    let signer = state.signing_keys.current();
    if let Some(workspace) = graph.workspace_id {
        require(
            &member_of(&state, auth, workspace).await?,
            WorkspaceAction::UpdateSettings,
        )?;
        // On record before the bundle is made, so that the bundle carries it.
        repo::audit::record(
            &state.db,
            workspace,
            auth.id,
            AuditAction::EvidenceExported,
            Subject::Text(&rid.to_string()),
            &format!("evidence bundle attested by {}", signer.kid()),
        )
        .await?;
    }
    let spans = repo::spans::chain(&state.db, rid).await?;
    let payloads = repo::span_payloads::meta_for_run(&state.db, rid).await?;
    let (audit, matching) = match graph.workspace_id {
        Some(workspace) => {
            repo::audit::about_run(&state.db, workspace, rid, row.graph_id, BUNDLE_AUDIT_MAX)
                .await?
        }
        None => (Vec::new(), 0),
    };
    let run = BundleRun {
        id: row.id,
        graph_id: row.graph_id,
        graph_name: graph.name,
        graph_deleted_at: graph.deleted_at,
        workspace_id: graph.workspace_id,
        status: row.status,
        created_at: row.created_at,
        started_at: row.started_at,
        finished_at: row.finished_at,
        trace_hash: row.trace_hash.as_deref().map(hex::encode),
    };
    let seal = row.seal.as_ref();
    let bundle = EvidenceBundle::assemble(run, spans, payloads, seal, audit, matching, signer);
    let disposition = format!("attachment; filename=\"run-{rid}.evidence.json\"");
    Ok(([(header::CONTENT_DISPOSITION, disposition)], Json(bundle)).into_response())
}

/// Recomputes the workspace's audit chain from its first entry up to the
/// head recorded when the check began (owners and admins): every entry
/// hashes as stored, links to the one before, carries names that match
/// their digests (except names erased with their account), and the chain
/// ends at its recorded head, so a removed tail shows too.
#[utoipa::path(get, path = "/workspaces/{wid}/audit/verify", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, body = AuditVerification), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn audit_verify(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Json<AuditVerification>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    // Entries written while the check runs come after this head and are left for the next one.
    let head = repo::audit::head(&state.db, wid).await?;
    let mut check = ChainCheck::default();
    let mut after = None;
    if let Some((last, _)) = head {
        loop {
            let page = repo::audit::chain_page(&state.db, wid, after, VERIFY_PAGE).await?;
            let full = page.len() as i64 == VERIFY_PAGE;
            after = page.last().map(|entry| entry.seq);
            // Hashing is CPU work: keep it off the request threads.
            let (checked, more) = tokio::task::spawn_blocking(move || {
                let more = page.iter().all(|e| e.seq <= last && check.push(e));
                (check, more)
            })
            .await
            .map_err(|err| AppError::Internal(err.into()))?;
            check = checked;
            if !more || !full {
                break;
            }
        }
    }
    Ok(Json(check.finish(head)))
}

/// The public keys everything nexc signs verifies with, as a JWK set: the
/// current key and the keys it replaced, each under its `kid` (the RFC 7638
/// thumbprint of the key). Public, and kept by any cache for five minutes
/// (`PUBLIC_CACHE_CONTROL`). Run seals, firewall decisions and evidence
/// bundles name the key they are signed with.
#[utoipa::path(get, path = "/.well-known/jwks.json", tag = "runs",
    responses((status = 200, body = Jwks)))]
pub async fn jwks(State(state): State<AppState>) -> Response {
    (
        [(header::CACHE_CONTROL, PUBLIC_CACHE_CONTROL)],
        Json(state.signing_keys.jwks()),
    )
        .into_response()
}
