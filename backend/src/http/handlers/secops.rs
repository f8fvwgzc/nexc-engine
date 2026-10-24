//! The AgentSecOps console: the overview its top of page needs in one call,
//! the trace explorer's search, and a live feed of the workspace's spans
//! and alerts. Read-only; members and above.

use std::collections::HashSet;
use std::convert::Infallible;
use std::time::Duration;

use async_stream::stream;
use axum::Json;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Duration as ChronoDuration, NaiveTime, Utc};
use futures::{Stream, StreamExt};
use serde::Deserialize;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use utoipa::IntoParams;
use uuid::Uuid;

use super::workspaces::{member_of, require};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::secops::{SecopsOverview, SecopsSpanPage};
use crate::domain::trace::SpanKind;
use crate::domain::workspace::WorkspaceAction;
use crate::http::extract::{AuthUser, Path, Query};
use crate::http::problem::Problem;
use crate::realtime::hub::SseFrame;
use crate::realtime::sse::{HEARTBEAT, RETRY};
use crate::repo;
use crate::repo::secops::SpanFilter;

/// Agents listed as the highest risk.
pub const TOP_RISK: i64 = 5;
/// Spans per page of the explorer by default.
pub const SPANS_DEFAULT: i64 = 100;
/// Most spans in one page of the explorer.
pub const SPANS_MAX: i64 = 500;
/// Longest tool, host or decision name searched for.
pub const FILTER_MAX: usize = 200;
/// How often the live feed looks for graphs created since it started.
pub const RESCAN: Duration = Duration::from_secs(10);
/// The graph events the live feed forwards.
const FORWARDED: [&str; 2] = ["trace.span", "alert"];

/// Midnight UTC of today.
fn today(now: DateTime<Utc>) -> DateTime<Utc> {
    now.date_naive().and_time(NaiveTime::MIN).and_utc()
}

/// Everything the top of the console shows: agents by status, runs today
/// and this week, spans and denials today, open approvals and alerts,
/// quarantined artifacts, the riskiest agents, the policy versions in force
/// and the last sealed trace (members and above).
#[utoipa::path(get, path = "/workspaces/{wid}/secops/overview", tag = "secops", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, body = SecopsOverview), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn overview(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Json<SecopsOverview>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::ViewMembers,
    )?;
    let now = Utc::now();
    let day = today(now);
    let week = now - ChronoDuration::days(7);
    let fortnight = now - ChronoDuration::days(14);
    let db = &state.db;
    let user = auth.id;
    let (agents, runs_today, runs_week, spans_today, denials_today, denials_fortnight) = tokio::try_join!(
        repo::secops::agent_counts(db, wid),
        repo::secops::run_counts(db, user, wid, day),
        repo::secops::run_counts(db, user, wid, week),
        repo::secops::span_count(db, user, wid, day),
        repo::secops::denials_by_rule(db, user, wid, day),
        repo::secops::denials_by_rule(db, user, wid, fortnight),
    )?;
    let (
        approvals_open,
        alerts_open,
        artifacts_quarantined,
        evidence_today,
        top_risk,
        policy_versions,
        last_seal,
        risk_history,
    ) = tokio::try_join!(
        repo::secops::approvals_open(db, wid),
        repo::secops::alerts_open(db, wid),
        repo::secops::artifacts_quarantined(db, user, wid),
        repo::secops::evidence_counts(db, user, wid, day),
        repo::secops::top_risk(db, wid, TOP_RISK),
        repo::secops::policies_in_force(db, wid),
        repo::secops::last_seal(db, user, wid),
        repo::secops::risk_history(db, wid, fortnight),
    )?;
    Ok(Json(SecopsOverview {
        agents,
        runs_today,
        runs_week,
        spans_today,
        denials_today,
        denials_fortnight,
        approvals_open,
        alerts_open,
        artifacts_quarantined,
        evidence_today,
        top_risk,
        policy_versions,
        last_seal,
        risk_history,
        at: now,
    }))
}

/// Query of `GET /workspaces/{wid}/secops/spans`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct SpanQuery {
    /// Only spans of this agent.
    pub agent: Option<Uuid>,
    /// Only spans of this kind (a `SpanKind`), or `inbound`: the tool calls that are tasks
    /// received over A2A.
    pub kind: Option<String>,
    /// Only tool calls of this tool, and policy decisions about it.
    pub tool: Option<String>,
    /// Only tool calls that reached this host.
    pub host: Option<String>,
    /// Only spans of this run.
    pub run: Option<Uuid>,
    /// Only spans started at or after this time.
    pub from: Option<DateTime<Utc>>,
    /// Only spans started before this time.
    pub to: Option<DateTime<Utc>>,
    /// Only policy and approval spans with this decision (`allow`, `deny`, `require_approval`,
    /// `alert`), or `evidence`: the `policy` spans named `evidence`, the verdict on each
    /// node's output, whatever was decided on it.
    pub decision: Option<String>,
    /// Spans per page, 1–500 (default 100).
    pub limit: Option<i64>,
    /// Only spans older than this one (the `next` of the previous page).
    pub before: Option<Uuid>,
}

const DECISIONS: [&str; 5] = [
    "allow",
    "deny",
    "require_approval",
    "alert",
    repo::secops::EVIDENCE_DECISION,
];
/// The pseudo-kind of the explorer: tool calls marked `inbound` (A2A tasks received).
const INBOUND: &str = "inbound";

impl SpanQuery {
    /// The kind asked for: a span kind, or the inbound pseudo-kind.
    fn kind(&self) -> Result<(Option<SpanKind>, bool), AppError> {
        match self.kind.as_deref() {
            None => Ok((None, false)),
            Some(INBOUND) => Ok((Some(SpanKind::ToolCall), true)),
            Some(raw) => raw.parse::<SpanKind>().map(|k| (Some(k), false)).map_err(|_| {
                AppError::field(
                    "kind",
                    "one of llm_call, tool_call, spawn, data_read, egress, policy, approval, inbound",
                )
            }),
        }
    }

    fn check(&self) -> Result<(), AppError> {
        self.kind()?;
        for (field, value) in [("tool", &self.tool), ("host", &self.host)] {
            if let Some(value) = value
                && (value.trim().is_empty()
                    || value.len() > FILTER_MAX
                    || value.chars().any(char::is_control))
            {
                return Err(AppError::field(
                    field,
                    "must be 1–200 characters without control characters when given",
                ));
            }
        }
        if let Some(decision) = &self.decision
            && !DECISIONS.contains(&decision.as_str())
        {
            return Err(AppError::field(
                "decision",
                "one of allow, deny, require_approval, alert, evidence",
            ));
        }
        if let (Some(from), Some(to)) = (self.from, self.to)
            && from >= to
        {
            return Err(AppError::field("to", "must be after `from`"));
        }
        Ok(())
    }
}

/// The trace explorer: spans across every run the caller may open, newest
/// first, narrowed by agent, kind, tool, host, run, time and decision;
/// paged with `before` (members and above).
#[utoipa::path(get, path = "/workspaces/{wid}/secops/spans", tag = "secops", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), SpanQuery),
    responses((status = 200, body = SecopsSpanPage), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 422, body = Problem)))]
pub async fn spans(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    Query(query): Query<SpanQuery>,
) -> Result<Json<SecopsSpanPage>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::ViewMembers,
    )?;
    query.check()?;
    let (kind, inbound) = query.kind()?;
    let limit = query.limit.unwrap_or(SPANS_DEFAULT).clamp(1, SPANS_MAX);
    let filter = SpanFilter {
        agent_id: query.agent,
        kind: kind.map(SpanKind::as_str),
        inbound,
        tool: query.tool.as_deref().map(str::trim),
        host: query.host.as_deref().map(str::trim),
        run_id: query.run,
        from: query.from,
        to: query.to,
        decision: query.decision.as_deref(),
        before: query.before,
    };
    let mut spans = repo::secops::search_spans(&state.db, auth.id, wid, &filter, limit + 1).await?;
    let next = (spans.len() as i64 > limit).then(|| {
        spans.truncate(limit as usize);
        spans.last().map(|s| s.id)
    });
    Ok(Json(SecopsSpanPage {
        spans,
        next: next.flatten(),
    }))
}

fn heartbeat(id: u64) -> Event {
    let data = serde_json::json!({ "at": Utc::now() }).to_string();
    Event::default()
        .id(id.to_string())
        .event("heartbeat")
        .data(data)
}

/// The workspace's `trace.span` and `alert` events across every graph the
/// member may open, as one stream: what the console's live feed shows.
/// Graphs created while the stream is open are picked up within ten seconds.
fn feed(
    state: AppState,
    user: Uuid,
    workspace: Uuid,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let admitted_at = state.sessions.floor(user);
    let events = stream! {
        yield Ok(Event::default().retry(RETRY));
        let mut seq: u64 = 0;
        let mut subscribed: HashSet<Uuid> = HashSet::new();
        let mut streams = futures::stream::SelectAll::<BroadcastStream<SseFrame>>::new();
        let mut subscribe = |streams: &mut futures::stream::SelectAll<BroadcastStream<SseFrame>>, ids: Vec<Uuid>| {
            for id in ids {
                if subscribed.insert(id) {
                    streams.push(BroadcastStream::new(state.hub.subscribe_sse(id)));
                }
            }
        };
        match repo::graphs::accessible_ids(&state.db, user, workspace).await {
            Ok(ids) => subscribe(&mut streams, ids),
            Err(err) => tracing::error!(%workspace, error = %err, "live feed could not list the graphs"),
        }
        seq += 1;
        yield Ok(heartbeat(seq));
        let mut beat = tokio::time::interval(HEARTBEAT);
        beat.tick().await;
        let mut rescan = tokio::time::interval(RESCAN);
        rescan.tick().await;
        loop {
            let event = tokio::select! {
                _ = beat.tick() => {
                    seq += 1;
                    heartbeat(seq)
                }
                _ = rescan.tick() => {
                    if let Ok(ids) = repo::graphs::accessible_ids(&state.db, user, workspace).await {
                        subscribe(&mut streams, ids);
                    }
                    continue;
                }
                next = streams.next(), if !streams.is_empty() => match next {
                    Some(Ok(frame)) if FORWARDED.contains(&&*frame.event) => {
                        seq += 1;
                        Event::default().id(seq.to_string()).event(&*frame.event).data(&*frame.data)
                    }
                    Some(Err(BroadcastStreamRecvError::Lagged(missed))) => {
                        tracing::warn!(%workspace, missed, "slow live-feed client skipped events");
                        continue;
                    }
                    _ => continue,
                },
            };
            if state.sessions.floor(user) > admitted_at {
                break;
            }
            yield Ok(event);
        }
    };
    Sse::new(events).keep_alive(KeepAlive::new().interval(HEARTBEAT))
}

/// Server-sent events of the whole workspace for the console's live feed:
/// every `trace.span` and `alert` of every graph the member may open
/// (contract §6 frames), with a `heartbeat` every 15 seconds. Bearer
/// authenticated, so it is read with `fetch`, not `EventSource`.
#[utoipa::path(get, path = "/workspaces/{wid}/secops/events", tag = "secops", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses(
        (status = 200, description = "SSE stream", content_type = "text/event-stream", body = String),
        (status = 401, body = Problem), (status = 403, body = Problem), (status = 404, body = Problem),
    ))]
pub async fn events(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Response, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::ViewMembers,
    )?;
    Ok(feed(state, auth.id, wid).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_query_is_checked() {
        let ok = SpanQuery {
            agent: None,
            kind: None,
            tool: Some("write_file".into()),
            host: None,
            run: None,
            from: None,
            to: None,
            decision: Some("deny".into()),
            limit: None,
            before: None,
        };
        assert!(ok.check().is_ok());
        let inbound = SpanQuery {
            kind: Some("inbound".into()),
            tool: None,
            decision: None,
            ..ok
        };
        assert_eq!(inbound.kind().unwrap(), (Some(SpanKind::ToolCall), true));
        let bad_kind = SpanQuery {
            kind: Some("teleport".into()),
            ..inbound
        };
        assert!(bad_kind.check().is_err());
        let ok = SpanQuery {
            kind: Some("policy".into()),
            tool: Some("write_file".into()),
            decision: Some("deny".into()),
            ..bad_kind
        };
        assert_eq!(ok.kind().unwrap(), (Some(SpanKind::Policy), false));
        let evidence = SpanQuery {
            decision: Some("evidence".into()),
            ..ok
        };
        assert!(evidence.check().is_ok(), "the verdict filter");
        let bad_decision = SpanQuery {
            decision: Some("maybe".into()),
            ..evidence
        };
        assert!(bad_decision.check().is_err());
        let now = Utc::now();
        let inverted = SpanQuery {
            decision: None,
            from: Some(now),
            to: Some(now),
            ..bad_decision
        };
        assert!(inverted.check().is_err());
        assert_eq!(today(now).time(), NaiveTime::MIN);
    }
}
