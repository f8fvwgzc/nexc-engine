//! SIEM pulls: alerts and audit entries of a workspace over a time window,
//! streamed oldest first as NDJSON (one object per line) or a JSON array,
//! with their stable ids, for a collector that polls rather than listens.
//! Admins only, rate limited per account.

use std::convert::Infallible;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use utoipa::IntoParams;
use uuid::Uuid;

use super::workspaces::{member_of, require};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::alert::Alert;
use crate::domain::audit::AuditEntry;
use crate::domain::string_enum;
use crate::domain::workspace::WorkspaceAction;
use crate::http::extract::{AuthUser, Path, Query};
use crate::http::problem::Problem;
use crate::repo;

/// Most entries one pull answers; a pull that answered this many has more
/// (ask again with `since` at the last `created_at`).
pub const PULL_MAX: usize = 10_000;
/// Entries read from the database at a time while streaming.
const PAGE: i64 = 1_000;
/// The window when `since` is not given: the last 24 hours.
const DEFAULT_WINDOW_HOURS: i64 = 24;

string_enum!(
    /// How an export is written.
    ExportFormat {
        /// `application/x-ndjson`: one JSON object per line.
        Ndjson => "ndjson",
        /// `application/json`: one array.
        Json => "json",
    }
);

/// Query of `GET /workspaces/{wid}/alerts/export` and `…/audit/export`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct ExportQuery {
    /// Entries after this time (exclusive); default: 24 hours before `until`.
    pub since: Option<DateTime<Utc>>,
    /// Entries up to this time (inclusive); default: now.
    pub until: Option<DateTime<Utc>>,
    /// `ndjson` (default) or `json`.
    pub format: Option<ExportFormat>,
}

/// The window a query asks for.
struct Window {
    since: DateTime<Utc>,
    until: DateTime<Utc>,
    format: ExportFormat,
}

impl ExportQuery {
    fn window(&self) -> Result<Window, AppError> {
        let until = self.until.unwrap_or_else(Utc::now);
        let since = self
            .since
            .unwrap_or(until - Duration::hours(DEFAULT_WINDOW_HOURS));
        if since >= until {
            return Err(AppError::field("since", "must be before until"));
        }
        Ok(Window {
            since,
            until,
            format: self.format.unwrap_or(ExportFormat::Ndjson),
        })
    }
}

/// A page of entries after a cursor.
trait Exported: Serialize + Send + 'static {
    fn cursor(&self) -> (DateTime<Utc>, Uuid);
}

impl Exported for Alert {
    fn cursor(&self) -> (DateTime<Utc>, Uuid) {
        (self.created_at, self.id)
    }
}

impl Exported for AuditEntry {
    fn cursor(&self) -> (DateTime<Utc>, Uuid) {
        (self.created_at, self.id)
    }
}

/// Streams pages from `fetch` in `format`, at most [`PULL_MAX`] entries.
fn stream<T, F, Fut>(format: ExportFormat, fetch: F) -> Response
where
    T: Exported,
    F: Fn(Option<(DateTime<Utc>, Uuid)>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<Vec<T>, sqlx::Error>> + Send,
{
    let body = async_stream::stream! {
        let mut after = None;
        let mut sent = 0usize;
        let mut open = false;
        if format == ExportFormat::Json {
            yield Ok::<Bytes, Infallible>(Bytes::from_static(b"["));
            open = true;
        }
        loop {
            let page = match fetch(after).await {
                Ok(page) => page,
                Err(err) => {
                    tracing::error!(error = %err, "export page not read; the stream ends short");
                    break;
                }
            };
            let last = page.last().map(Exported::cursor);
            let mut chunk = Vec::new();
            for entry in &page {
                match format {
                    ExportFormat::Ndjson => {
                        serde_json::to_writer(&mut chunk, entry).expect("an entry serialises");
                        chunk.push(b'\n');
                    }
                    ExportFormat::Json => {
                        if sent > 0 {
                            chunk.push(b',');
                        }
                        serde_json::to_writer(&mut chunk, entry).expect("an entry serialises");
                    }
                }
                sent += 1;
                if sent >= PULL_MAX {
                    break;
                }
            }
            if !chunk.is_empty() {
                yield Ok(Bytes::from(chunk));
            }
            if page.len() < PAGE as usize || sent >= PULL_MAX {
                break;
            }
            after = last;
        }
        if open {
            yield Ok(Bytes::from_static(b"]\n"));
        }
    };
    let content_type = match format {
        ExportFormat::Ndjson => "application/x-ndjson",
        ExportFormat::Json => "application/json",
    };
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-store"),
        ],
        Body::from_stream(body),
    )
        .into_response()
}

async fn admin_pull(state: &AppState, auth: AuthUser, wid: Uuid) -> Result<(), AppError> {
    require(
        &member_of(state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    state.limiters.check_export(auth.id)
}

/// The alerts of a workspace over a window, oldest first, for a SIEM that
/// pulls (admins; 30 pulls a minute per account). At most 10,000 per pull:
/// a pull that answered that many has more, from `since` = the last `created_at`.
#[utoipa::path(get, path = "/workspaces/{wid}/alerts/export", tag = "alerts", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ExportQuery),
    responses((status = 200, description = "The alerts, one JSON object per line (`application/x-ndjson`) or one array (`application/json`)",
        body = [Alert], content_type = "application/x-ndjson"),
        (status = 403, body = Problem), (status = 404, body = Problem), (status = 422, body = Problem),
        (status = 429, body = Problem)))]
pub async fn alerts(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    Query(query): Query<ExportQuery>,
) -> Result<Response, AppError> {
    let window = query.window()?;
    admin_pull(&state, auth, wid).await?;
    let db = state.db.clone();
    Ok(stream(window.format, move |after| {
        let db = db.clone();
        async move { repo::alerts::export_page(&db, wid, window.since, window.until, after, PAGE).await }
    }))
}

/// The audit log of a workspace over a window, oldest first, for a SIEM
/// that pulls (admins; 30 pulls a minute per account); same shape and
/// paging as the alerts export.
#[utoipa::path(get, path = "/workspaces/{wid}/audit/export", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ExportQuery),
    responses((status = 200, description = "The entries, one JSON object per line (`application/x-ndjson`) or one array (`application/json`)",
        body = [AuditEntry], content_type = "application/x-ndjson"),
        (status = 403, body = Problem), (status = 404, body = Problem), (status = 422, body = Problem),
        (status = 429, body = Problem)))]
pub async fn audit(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    Query(query): Query<ExportQuery>,
) -> Result<Response, AppError> {
    let window = query.window()?;
    admin_pull(&state, auth, wid).await?;
    let db = state.db.clone();
    Ok(stream(window.format, move |after| {
        let db = db.clone();
        async move { repo::audit::export_page(&db, wid, window.since, window.until, after, PAGE).await }
    }))
}
