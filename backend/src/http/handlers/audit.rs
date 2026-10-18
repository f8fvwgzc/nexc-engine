//! Ask the audit: questions about what agents did, answered from the facts
//! exported to onto-intelli (`ontology::ask`).

use std::convert::Infallible;

use axum::Json;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::workspaces::member_of;
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::validation::{FieldErrors, Validate};
use crate::http::extract::{AuthUser, Path, ValidatedJson};
use crate::http::problem::Problem;
use crate::ontology::ask::{
    self, Asker, AuditAnswer, AuditStreamEvent, CannedQuestion, QUESTION_MAX,
};

/// `POST /workspaces/{wid}/audit/ask` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AskAudit {
    /// The question, at most 1,000 characters. The canned questions are answered
    /// from the graph alone; anything else goes to onto-intelli's `Ask`.
    pub question: String,
}

impl Validate for AskAudit {
    fn validate(&self, errors: &mut FieldErrors) {
        if self.question.trim().is_empty() {
            errors.add("question", "must not be empty");
        } else if self.question.chars().count() > QUESTION_MAX {
            errors.add(
                "question",
                format!("must be at most {QUESTION_MAX} characters"),
            );
        }
    }
}

async fn asker(state: &AppState, auth: AuthUser, wid: Uuid) -> Result<Asker, AppError> {
    let workspace = member_of(state, auth, wid).await?;
    Ok(Asker {
        user_id: auth.id,
        workspace_id: wid,
        role: workspace.role,
    })
}

/// Asks the audit. Members and above; guests are refused. The answer names
/// the facts it rests on as things in nexc; one that cites none is
/// `unverified`. Members do not see confidential or restricted data ids.
/// 503 when no onto-intelli is configured or it does not answer.
#[utoipa::path(post, path = "/workspaces/{wid}/audit/ask", tag = "audit", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    request_body = AskAudit,
    responses((status = 200, body = AuditAnswer), (status = 403, body = Problem), (status = 404, body = Problem),
              (status = 422, body = Problem), (status = 503, description = "Ask the audit is off, or onto-intelli does not answer", body = Problem)))]
pub async fn ask(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<AskAudit>,
) -> Result<Json<AuditAnswer>, AppError> {
    let asker = asker(&state, auth, wid).await?;
    Ok(Json(
        ask::answer(&state, &asker, req.question.trim()).await?,
    ))
}

/// Asks the audit and streams the answer as server-sent events: `stage`
/// (`{text}`) and `delta` (`{text}`) as onto-intelli explains, then `done`
/// (`AuditAnswer`), or `error` (`{detail}`). A canned question, and any
/// question from a member, arrives as one `done` event.
#[utoipa::path(post, path = "/workspaces/{wid}/audit/ask/stream", tag = "audit", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    request_body = AskAudit,
    responses((status = 200, description = "SSE stream", content_type = "text/event-stream", body = String),
              (status = 403, body = Problem), (status = 404, body = Problem), (status = 422, body = Problem),
              (status = 503, description = "Ask the audit is off, or onto-intelli does not answer", body = Problem)))]
pub async fn ask_stream(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<AskAudit>,
) -> Result<Response, AppError> {
    let asker = asker(&state, auth, wid).await?;
    let events = ask::answer_stream(state, asker, req.question.trim().to_owned()).await?;
    let frames = events.map(|event| {
        let frame = match event {
            AuditStreamEvent::Stage(text) => Event::default()
                .event("stage")
                .data(serde_json::json!({"text": text}).to_string()),
            AuditStreamEvent::Delta(text) => Event::default()
                .event("delta")
                .data(serde_json::json!({"text": text}).to_string()),
            AuditStreamEvent::Done(answer) => Event::default()
                .event("done")
                .data(serde_json::to_string(&answer).unwrap_or_default()),
            AuditStreamEvent::Error(detail) => Event::default()
                .event("error")
                .data(serde_json::json!({"detail": detail}).to_string()),
        };
        Ok::<Event, Infallible>(frame)
    });
    Ok(Sse::new(frames)
        .keep_alive(KeepAlive::default())
        .into_response())
}

/// The canned questions, in the order the console offers them. `{run}` and
/// `{agent}` in a label are slots the asker fills.
#[utoipa::path(get, path = "/workspaces/{wid}/audit/questions", tag = "audit", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, body = [CannedQuestion]), (status = 404, body = Problem)))]
pub async fn questions(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Json<Vec<CannedQuestion>>, AppError> {
    member_of(&state, auth, wid).await?;
    Ok(Json(ask::CANNED.to_vec()))
}
