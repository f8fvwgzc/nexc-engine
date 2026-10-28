//! Alert routing: the Defend function reaching out. A workspace registers
//! destinations (a signed JSON webhook, a Slack incoming webhook); when an
//! alert is raised or an approval is requested or decided, a delivery per
//! matching destination goes into the outbox (`alert_deliveries`); a worker
//! takes due deliveries with `SKIP LOCKED` (instances share the work),
//! sends them with `X-Nexc-Signature` and `X-Nexc-Timestamp`, retries
//! failures with exponential backoff for a day, and keeps the log the
//! console shows. A destination that fails fifty times in a row is switched
//! off, with an audit entry and a `policy` alert so the operator notices.
#![forbid(unsafe_code)]

pub mod payload;

use std::time::{Duration, Instant};

use chrono::Utc;
use futures::StreamExt;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::alert::{Alert, AlertKind};
use crate::domain::alert_routing::{
    AlertDelivery, DeliveryEvent, DestinationKind, DestinationTest, FAILURES_MAX, RETRY_WINDOW,
    backoff, check_url,
};
use crate::domain::approval::Approval;
use crate::domain::audit::AuditAction;
use crate::domain::policy_document::Severity;
use crate::engine::defend::{About, Finding};
use crate::gateway::fetch::is_private_host;
use crate::repo;
use crate::repo::alert_deliveries::{Claimed, NewDelivery};
use crate::repo::alert_destinations::AlertDestinationRow;
use crate::security::egress::EgressGuard;
use payload::{Subject, WorkspaceLabel};

/// How often the outbox is looked at.
pub const TICK: Duration = Duration::from_secs(2);
/// Deliveries taken per pass.
pub const BATCH: i64 = 50;
/// How long a taken delivery is held before another pass may take it again.
pub const LEASE: Duration = Duration::from_secs(300);
/// One send, connection included.
pub const SEND_TIMEOUT: Duration = Duration::from_secs(10);
/// Of a destination's answer, how much is kept in the log.
pub const ANSWER_MAX: usize = 1_000;
/// Sends in flight at once in one pass.
const PARALLEL: usize = 8;
/// The rule of the alert raised when a destination is switched off.
pub const SWITCHED_OFF_RULE: &str = "alert_destinations.failures";

/// What one pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    pub delivered: usize,
    /// Failed, to be tried again.
    pub retried: usize,
    /// Given up.
    pub failed: usize,
}

/// Whether `url` may be a destination: an http(s) URL without credentials
/// whose host is not this machine or a private network (unless the server
/// allows those, for development); the host when it may.
pub fn check_destination_url(
    url: &str,
    allow_private: bool,
    guard: &EgressGuard,
) -> Result<String, String> {
    let host = check_url(url)?;
    if !allow_private && is_private_host(&host) {
        return Err(format!(
            "{host} is on this machine or a private network; such a destination is not reached (NEXC_ALERT_ALLOW_PRIVATE_HOSTS=true allows it in development)"
        ));
    }
    // Deliveries go out behind the SSRF guard; an address it refuses is refused here already.
    guard.check_host(&host)?;
    Ok(host)
}

/// Starts the worker; it stops when `shutdown` is cancelled.
pub fn spawn(state: &AppState, shutdown: CancellationToken) {
    tokio::spawn(run(state.clone(), shutdown));
}

async fn run(state: AppState, shutdown: CancellationToken) {
    let mut ticker = tokio::time::interval(TICK);
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = ticker.tick() => {
                match pass(&state).await {
                    Ok(progress) if progress != Progress::default() => {
                        tracing::debug!(?progress, "alert routing pass");
                    }
                    Ok(_) => {}
                    Err(err) => tracing::warn!(error = %err, "alert routing pass failed"),
                }
            }
        }
    }
}

/// One pass: takes the due deliveries and sends them, a few at a time.
pub async fn pass(state: &AppState) -> anyhow::Result<Progress> {
    let claimed = repo::alert_deliveries::claim(&state.db, BATCH, LEASE.as_secs() as i64).await?;
    let outcomes: Vec<Outcome> = futures::stream::iter(claimed)
        .map(|delivery| deliver(state, delivery))
        .buffer_unordered(PARALLEL)
        .collect()
        .await;
    let mut progress = Progress::default();
    for outcome in outcomes {
        match outcome {
            Outcome::Delivered => progress.delivered += 1,
            Outcome::Retry => progress.retried += 1,
            Outcome::Failed => progress.failed += 1,
        }
    }
    Ok(progress)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Delivered,
    Retry,
    Failed,
}

/// What a send came back with.
#[derive(Debug, Clone)]
pub struct Sent {
    pub ok: bool,
    /// The HTTP status, when an answer came.
    pub status: Option<u16>,
    /// Whether trying again may help (no answer, 5xx, 408, 429).
    pub retryable: bool,
    /// The answer or the error, for the log.
    pub detail: String,
}

/// The sealed URL and secret of a destination, opened.
struct Opened {
    url: String,
    secret: Vec<u8>,
}

fn open(state: &AppState, row: &AlertDestinationRow) -> anyhow::Result<Opened> {
    let aad = row.destination.id.as_bytes();
    let url = state.secret_box.open(&row.url, aad)?;
    let secret = state.secret_box.open(&row.secret, aad)?;
    Ok(Opened {
        url: String::from_utf8(url)?,
        secret,
    })
}

/// The body sent to a destination: the envelope itself, or the Slack message made from it.
fn body_for(kind: DestinationKind, envelope: &Value) -> Vec<u8> {
    let value = match kind {
        DestinationKind::Webhook => envelope.clone(),
        DestinationKind::Slack => payload::slack_message(envelope),
    };
    serde_json::to_vec(&value).expect("a JSON value serialises")
}

/// Reads up to [`ANSWER_MAX`] bytes of an answer, for the log.
async fn answer(response: reqwest::Response) -> String {
    let mut stream = response.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else { break };
        buf.extend_from_slice(&chunk);
        if buf.len() >= ANSWER_MAX {
            break;
        }
    }
    buf.truncate(ANSWER_MAX);
    String::from_utf8_lossy(&buf)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Posts `body` to `url`, signed with `secret`. Redirects are not followed:
/// a destination that redirects is a failure, like any answer but 2xx.
pub async fn send(client: &reqwest::Client, url: &str, secret: &[u8], body: Vec<u8>) -> Sent {
    let signature = payload::sign(secret, &body);
    let result = client
        .post(url)
        .timeout(SEND_TIMEOUT)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header("x-nexc-signature", signature)
        .header("x-nexc-timestamp", Utc::now().timestamp().to_string())
        .body(body)
        .send()
        .await;
    match result {
        Err(err) => Sent {
            ok: false,
            status: None,
            retryable: true,
            detail: crate::gateway::fetch::describe(err),
        },
        Ok(response) => {
            let status = response.status();
            let text = answer(response).await;
            if status.is_success() {
                Sent {
                    ok: true,
                    status: Some(status.as_u16()),
                    retryable: false,
                    detail: text,
                }
            } else {
                Sent {
                    ok: false,
                    status: Some(status.as_u16()),
                    retryable: status.is_server_error()
                        || status == reqwest::StatusCode::REQUEST_TIMEOUT
                        || status == reqwest::StatusCode::TOO_MANY_REQUESTS,
                    detail: format!("HTTP {}: {text}", status.as_u16()),
                }
            }
        }
    }
}

/// Sends one taken delivery and records what became of it.
async fn deliver(state: &AppState, claimed: Claimed) -> Outcome {
    let delivery = &claimed.delivery;
    let row = match repo::alert_destinations::find_any(&state.db, delivery.destination_id).await {
        Ok(Some(row)) => row,
        Ok(None) => return give_up(state, delivery, None, "the destination is gone").await,
        Err(err) => {
            tracing::warn!(delivery = %delivery.id, error = %err, "destination not read; the delivery waits");
            return Outcome::Retry;
        }
    };
    if !row.destination.enabled {
        return give_up(state, delivery, None, "the destination is switched off").await;
    }
    let opened = match open(state, &row) {
        Ok(opened) => opened,
        Err(err) => {
            let detail = format!("the destination's URL or secret could not be read: {err}");
            return settle(
                state,
                &row,
                delivery,
                &Sent {
                    ok: false,
                    status: None,
                    retryable: false,
                    detail,
                },
            )
            .await;
        }
    };
    let body = body_for(row.destination.kind, &claimed.payload);
    let sent = send(&state.gateway_http, &opened.url, &opened.secret, body).await;
    settle(state, &row, delivery, &sent).await
}

/// Marks a delivery given up without a destination to blame.
async fn give_up(
    state: &AppState,
    delivery: &AlertDelivery,
    status: Option<i32>,
    detail: &str,
) -> Outcome {
    if let Err(err) =
        repo::alert_deliveries::mark_failed(&state.db, delivery.id, status, detail).await
    {
        tracing::error!(delivery = %delivery.id, error = %err, "delivery outcome not recorded");
    }
    Outcome::Failed
}

/// Records what a send came back with, on the delivery and on the destination.
async fn settle(
    state: &AppState,
    row: &AlertDestinationRow,
    delivery: &AlertDelivery,
    sent: &Sent,
) -> Outcome {
    let destination = &row.destination;
    let kind = destination.kind.as_str();
    let status = sent.status.map(i32::from);
    if sent.ok {
        if let Err(err) =
            repo::alert_deliveries::mark_delivered(&state.db, delivery.id, status).await
        {
            tracing::error!(delivery = %delivery.id, error = %err, "delivery not marked delivered");
        }
        if let Err(err) = repo::alert_destinations::record_success(&state.db, destination.id).await
        {
            tracing::error!(destination = %destination.id, error = %err, "delivery not counted");
        }
        state
            .metrics
            .alert_deliveries
            .add(&[("kind", kind), ("outcome", "delivered")], 1);
        return Outcome::Delivered;
    }
    let detail: String = sent.detail.chars().take(ANSWER_MAX).collect();
    let next = Utc::now() + backoff(delivery.attempts);
    let deadline = delivery.created_at + RETRY_WINDOW;
    let outcome = if sent.retryable && next <= deadline {
        if let Err(err) =
            repo::alert_deliveries::mark_retry(&state.db, delivery.id, next, status, &detail).await
        {
            tracing::error!(delivery = %delivery.id, error = %err, "delivery retry not recorded");
        }
        Outcome::Retry
    } else {
        let why = if sent.retryable {
            format!(
                "{detail} (given up after {} attempts in a day)",
                delivery.attempts
            )
        } else {
            detail.clone()
        };
        if let Err(err) =
            repo::alert_deliveries::mark_failed(&state.db, delivery.id, status, &why).await
        {
            tracing::error!(delivery = %delivery.id, error = %err, "delivery failure not recorded");
        }
        Outcome::Failed
    };
    state.metrics.alert_deliveries.add(
        &[
            ("kind", kind),
            (
                "outcome",
                if outcome == Outcome::Retry {
                    "retry"
                } else {
                    "failed"
                },
            ),
        ],
        1,
    );
    match repo::alert_destinations::record_failure(&state.db, destination.id, &detail).await {
        Ok(failures) if failures >= FAILURES_MAX => switch_off(state, row, failures, &detail).await,
        Ok(_) => {}
        Err(err) => {
            tracing::error!(destination = %destination.id, error = %err, "delivery failure not counted");
        }
    }
    outcome
}

/// Switches a destination off after too many failures in a row: audited,
/// and an alert of kind `policy` for the admins.
async fn switch_off(state: &AppState, row: &AlertDestinationRow, failures: i32, last: &str) {
    let destination = &row.destination;
    let reason = format!("{failures} deliveries in a row failed; last: {last}");
    match repo::alert_destinations::disable(&state.db, destination.id, &reason).await {
        Ok(true) => {}
        Ok(false) => return,
        Err(err) => {
            tracing::error!(destination = %destination.id, error = %err, "destination not switched off");
            return;
        }
    }
    tracing::warn!(destination = %destination.id, name = %destination.name, failures, "alert destination switched off");
    if let Err(err) = repo::audit::record_system(
        &state.db,
        destination.workspace_id,
        AuditAction::AlertDestinationChanged,
        &format!("alerts:{}", destination.name),
        &format!("switched off: {reason}"),
    )
    .await
    {
        tracing::error!(destination = %destination.id, error = %err, "switch-off not audited");
    }
    crate::engine::defend::raise(
        state,
        &About {
            workspace_id: destination.workspace_id,
            agent_id: None,
            principal: "system",
            run_id: None,
            node_id: None,
            graph_id: None,
            actor: None,
        },
        &Finding {
            span_id: None,
            kind: AlertKind::Policy,
            severity: Severity::High,
            rule: SWITCHED_OFF_RULE,
            title: &format!(
                "alert destination {} switched off ({})",
                destination.name, destination.host
            ),
            detail: &format!("{reason}. Fix the destination and enable it again under Settings → Alert destinations."),
        },
    )
    .await;
}

/// The workspace label of the envelope.
async fn workspace_label(state: &AppState, workspace_id: Uuid) -> Option<WorkspaceLabel> {
    match repo::alert_destinations::workspace_label(&state.db, workspace_id).await {
        Ok(Some((name, slug))) => Some(WorkspaceLabel {
            id: workspace_id,
            name,
            slug,
        }),
        Ok(None) => None,
        Err(err) => {
            tracing::error!(%workspace_id, error = %err, "workspace not read for alert routing");
            None
        }
    }
}

/// Something that happened, to be queued for the destinations that want it.
struct Event<'a> {
    workspace_id: Uuid,
    event: DeliveryEvent,
    /// When it happened.
    at: chrono::DateTime<Utc>,
    subject: Subject<'a>,
    alert_id: Option<Uuid>,
    approval_id: Option<Uuid>,
}

/// Puts a delivery in the outbox of every destination in `rows` that `wants`
/// it; returns how many. Never fails what raised the event.
async fn enqueue_for(
    state: &AppState,
    rows: &[AlertDestinationRow],
    wants: impl Fn(&AlertDestinationRow) -> bool,
    happened: Event<'_>,
) -> usize {
    let wanted: Vec<&AlertDestinationRow> = rows.iter().filter(|row| wants(row)).collect();
    if wanted.is_empty() {
        return 0;
    }
    let Some(workspace) = workspace_label(state, happened.workspace_id).await else {
        return 0;
    };
    let mut queued = 0;
    for row in wanted {
        let id = Uuid::now_v7();
        let envelope = payload::envelope(
            id,
            happened.event,
            happened.at,
            &workspace,
            happened.subject,
            &state.settings.public_url,
        );
        let new = NewDelivery {
            id,
            destination_id: row.destination.id,
            workspace_id: happened.workspace_id,
            event: happened.event,
            alert_id: happened.alert_id,
            approval_id: happened.approval_id,
            payload: &envelope,
        };
        match repo::alert_deliveries::enqueue(&state.db, &new).await {
            Ok(()) => queued += 1,
            Err(err) => {
                tracing::error!(destination = %row.destination.id, event = %happened.event, error = %err, "delivery not queued");
            }
        }
    }
    queued
}

async fn destinations_of(state: &AppState, workspace_id: Uuid) -> Vec<AlertDestinationRow> {
    match repo::alert_destinations::list(&state.db, workspace_id).await {
        Ok(rows) => rows,
        Err(err) => {
            tracing::error!(%workspace_id, error = %err, "alert destinations not read");
            Vec::new()
        }
    }
}

/// Queues an alert just raised for every destination that wants its kind
/// and severity; returns how many.
pub async fn enqueue_alert(state: &AppState, alert: &Alert) -> usize {
    let rows = destinations_of(state, alert.workspace_id).await;
    enqueue_for(
        state,
        &rows,
        |row| row.destination.wants_alert(alert.kind, alert.severity),
        Event {
            workspace_id: alert.workspace_id,
            event: DeliveryEvent::AlertRaised,
            at: alert.created_at,
            subject: Subject::Alert(alert),
            alert_id: Some(alert.id),
            approval_id: None,
        },
    )
    .await
}

/// Queues an approval requested or decided for every destination that takes
/// approvals; returns how many.
pub async fn enqueue_approval(
    state: &AppState,
    approval: &Approval,
    event: DeliveryEvent,
) -> usize {
    let rows = destinations_of(state, approval.workspace_id).await;
    let at = match event {
        DeliveryEvent::ApprovalDecided => approval.decided_at.unwrap_or(approval.requested_at),
        _ => approval.requested_at,
    };
    enqueue_for(
        state,
        &rows,
        |row| row.destination.wants_approvals(),
        Event {
            workspace_id: approval.workspace_id,
            event,
            at,
            subject: Subject::Approval(approval),
            alert_id: None,
            approval_id: Some(approval.id),
        },
    )
    .await
}

/// Sends a test event to a destination at once and records it in its log.
pub async fn send_test(
    state: &AppState,
    row: &AlertDestinationRow,
) -> anyhow::Result<DestinationTest> {
    let destination = &row.destination;
    let workspace = workspace_label(state, destination.workspace_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("workspace not found"))?;
    let opened = open(state, row)?;
    let id = Uuid::now_v7();
    let envelope = payload::envelope(
        id,
        DeliveryEvent::Test,
        Utc::now(),
        &workspace,
        Subject::Test,
        &state.settings.public_url,
    );
    let started = Instant::now();
    let sent = send(
        &state.gateway_http,
        &opened.url,
        &opened.secret,
        body_for(destination.kind, &envelope),
    )
    .await;
    let latency_ms = started.elapsed().as_millis() as u64;
    let detail: String = sent.detail.chars().take(ANSWER_MAX).collect();
    repo::alert_deliveries::insert_settled(
        &state.db,
        &NewDelivery {
            id,
            destination_id: destination.id,
            workspace_id: destination.workspace_id,
            event: DeliveryEvent::Test,
            alert_id: None,
            approval_id: None,
            payload: &envelope,
        },
        sent.ok,
        sent.status.map(i32::from),
        if sent.ok { "" } else { &detail },
    )
    .await?;
    state.metrics.alert_deliveries.add(
        &[
            ("kind", destination.kind.as_str()),
            ("outcome", if sent.ok { "delivered" } else { "failed" }),
        ],
        1,
    );
    Ok(DestinationTest {
        ok: sent.ok,
        status: sent.status,
        detail: if sent.ok {
            format!("the destination answered HTTP {}", sent.status.unwrap_or(0))
        } else {
            detail
        },
        latency_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_urls_keep_private_hosts_out_unless_allowed() {
        let guard = EgressGuard::default();
        let check = |url, allow| check_destination_url(url, allow, &guard);
        assert_eq!(
            check("https://hooks.slack.com/services/T/B/x", false).unwrap(),
            "hooks.slack.com"
        );
        assert!(check("http://127.0.0.1:9/hook", false).is_err());
        assert!(check("http://localhost/hook", false).is_err());
        assert!(check("http://10.0.0.5/hook", false).is_err());
        assert!(check("ftp://x.example", true).is_err());
        // The development switch lets a private host through only where the operator opened
        // its range for the SSRF guard too, and never the metadata service.
        assert!(check("http://127.0.0.1:9/hook", true).is_err());
        let open = EgressGuard::new(crate::security::egress::parse_allowed("127.0.0.0/8").unwrap());
        assert_eq!(
            check_destination_url("http://127.0.0.1:9/hook", true, &open).unwrap(),
            "127.0.0.1"
        );
        assert!(check_destination_url("http://169.254.169.254/", true, &open).is_err());
    }
}
