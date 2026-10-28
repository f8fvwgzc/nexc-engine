//! The outbound side: fetching a counterpart's agent card, and sending it a
//! task over JSON-RPC (`tasks/sendSubscribe` read as server-sent events when
//! its card says it streams, else `tasks/send` and `tasks/get` until the
//! task ended), bounded in time and size by the gateway.

use std::time::{Duration, Instant};

use base64::Engine as _;
use futures::StreamExt;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::a2a::{A2aCheck, Hops, TaskState, validate_card};
use crate::repo;
use crate::repo::a2a::CounterpartRow;

/// Fetching a card.
pub const CARD_TIMEOUT: Duration = Duration::from_secs(15);
/// Largest card read.
pub const CARD_MAX_BYTES: usize = 1024 * 1024;
/// How often a task sent without streaming is looked at.
const POLL: Duration = Duration::from_secs(1);

/// Why a task could not be exchanged.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OutboundError {
    #[error("the request failed: {0}")]
    Transport(String),
    #[error("the remote agent answered HTTP {0}")]
    Status(u16),
    #[error("the remote agent's reply does not fit the protocol: {0}")]
    Protocol(String),
    #[error("the remote agent refused the task ({code}): {message}")]
    Remote { code: i64, message: String },
    #[error("the reply is larger than {0} bytes")]
    Oversized(usize),
}

impl OutboundError {
    /// The HTTP status the remote answered, when it answered one.
    pub fn status(&self) -> Option<u16> {
        match self {
            OutboundError::Status(status) => Some(*status),
            _ => None,
        }
    }
}

/// A file the remote agent answered with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFile {
    pub name: String,
    pub mime: String,
    pub bytes: Option<Vec<u8>>,
    pub uri: Option<String>,
}

/// What came back from a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub task_id: Uuid,
    pub state: TaskState,
    /// The text and data parts of its artifacts, in order.
    pub texts: Vec<String>,
    pub files: Vec<RemoteFile>,
    /// What its status said, when it said something.
    pub message: Option<String>,
    pub bytes_in: usize,
    pub status: Option<u16>,
    pub streamed: bool,
}

/// What to send.
pub struct Outbound<'a> {
    pub message: &'a str,
    pub session_id: Option<&'a str>,
    pub hops: &'a Hops,
    pub principal: &'a str,
    pub max_bytes: usize,
    pub deadline: Instant,
}

fn text_of(parts: &[Value]) -> impl Iterator<Item = String> + '_ {
    parts.iter().filter_map(|part| {
        match part
            .get("type")
            .or_else(|| part.get("kind"))
            .and_then(Value::as_str)
        {
            Some("text") => part.get("text").and_then(Value::as_str).map(str::to_owned),
            Some("data") => part
                .get("data")
                .map(|d| serde_json::to_string_pretty(d).unwrap_or_default()),
            _ => None,
        }
    })
}

fn files_of(parts: &[Value]) -> impl Iterator<Item = RemoteFile> + '_ {
    parts.iter().filter_map(|part| {
        let file = part.get("file")?.as_object()?;
        let bytes = file.get("bytes").and_then(Value::as_str).and_then(|b| {
            base64::engine::general_purpose::STANDARD
                .decode(b.trim())
                .ok()
        });
        let uri = file
            .get("uri")
            .and_then(Value::as_str)
            .map(|u| u.chars().take(2_000).collect());
        Some(RemoteFile {
            name: file
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("file")
                .chars()
                .take(200)
                .collect(),
            mime: file
                .get("mimeType")
                .or_else(|| file.get("mime_type"))
                .and_then(Value::as_str)
                .unwrap_or("application/octet-stream")
                .chars()
                .take(127)
                .collect(),
            bytes,
            uri,
        })
    })
}

fn status_message(status: &Value) -> Option<String> {
    let parts = status.get("message")?.get("parts")?.as_array()?;
    let text: Vec<String> = text_of(parts).collect();
    (!text.is_empty()).then(|| text.join("\n"))
}

/// Takes what a `Task` object carries into `remote`.
fn absorb_task(remote: &mut Remote, task: &Value) {
    if let Some(state) = task
        .get("status")
        .and_then(|s| s.get("state"))
        .and_then(Value::as_str)
        .and_then(TaskState::parse)
    {
        remote.state = state;
    }
    if let Some(message) = task.get("status").and_then(status_message) {
        remote.message = Some(message);
    }
    remote.texts.clear();
    remote.files.clear();
    for artifact in task
        .get("artifacts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let parts = artifact
            .get("parts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        remote.texts.extend(text_of(&parts));
        remote.files.extend(files_of(&parts));
    }
}

/// The reply to a JSON-RPC call, or the error it carried.
fn rpc_result(reply: &Value) -> Result<Value, OutboundError> {
    if let Some(error) = reply.get("error").filter(|e| !e.is_null()) {
        return Err(OutboundError::Remote {
            code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
            message: error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("no message")
                .chars()
                .take(500)
                .collect(),
        });
    }
    reply
        .get("result")
        .cloned()
        .ok_or_else(|| OutboundError::Protocol("no result and no error".into()))
}

fn transport(err: reqwest::Error) -> OutboundError {
    OutboundError::Transport(crate::gateway::fetch::describe(err))
}

/// Reads a whole response up to `max_bytes`.
async fn read_all(response: reqwest::Response, max_bytes: usize) -> Result<Vec<u8>, OutboundError> {
    let mut stream = response.bytes_stream();
    let mut buf = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(transport)?;
        buf.extend_from_slice(&chunk);
        if buf.len() > max_bytes {
            return Err(OutboundError::Oversized(max_bytes));
        }
    }
    Ok(buf)
}

/// Fetches and validates a counterpart's agent card, and records what it found.
pub async fn fetch_card(state: &AppState, row: &CounterpartRow) -> A2aCheck {
    let started = Instant::now();
    let counterpart = &row.counterpart;
    let attempt = async {
        let mut request = state
            .gateway_http
            .get(&counterpart.card_url)
            .header("accept", "application/json");
        if let Some(header) = auth_header(state, row) {
            request = request.header("authorization", header);
        }
        let response = request.send().await.map_err(transport)?;
        let status = response.status().as_u16();
        if status != 200 {
            return Err(OutboundError::Status(status));
        }
        let body = read_all(response, CARD_MAX_BYTES).await?;
        let card: Value = serde_json::from_slice(&body)
            .map_err(|e| OutboundError::Protocol(format!("the card is not JSON: {e}")))?;
        validate_card(&card).map_err(OutboundError::Protocol)
    };
    let found = match tokio::time::timeout(CARD_TIMEOUT, attempt).await {
        Ok(Ok(card)) => A2aCheck {
            ok: true,
            detail: format!(
                "{}: {} skill(s), {}",
                card["name"].as_str().unwrap_or("agent"),
                card["skills"].as_array().map_or(0, Vec::len),
                if card["capabilities"]["streaming"].as_bool() == Some(true) {
                    "streams"
                } else {
                    "does not stream"
                }
            ),
            card: Some(card),
            latency_ms: started.elapsed().as_millis() as u64,
        },
        Ok(Err(err)) => A2aCheck {
            ok: false,
            detail: err.to_string(),
            card: None,
            latency_ms: started.elapsed().as_millis() as u64,
        },
        Err(_) => A2aCheck {
            ok: false,
            detail: format!("no answer within {} seconds", CARD_TIMEOUT.as_secs()),
            card: None,
            latency_ms: started.elapsed().as_millis() as u64,
        },
    };
    if let Err(err) = repo::a2a::set_counterpart_check(
        &state.db,
        counterpart.id,
        found.ok,
        &found.detail,
        found.card.as_ref(),
    )
    .await
    {
        tracing::error!(counterpart = %counterpart.id, error = %err, "A2A card check not recorded");
    }
    found
}

/// The `Authorization` header of a counterpart, decrypted (bound to its id).
pub fn auth_header(state: &AppState, row: &CounterpartRow) -> Option<String> {
    let sealed = row.auth_header.as_deref()?;
    match state.secret_box.open(sealed, row.counterpart.id.as_bytes()) {
        Ok(bytes) => String::from_utf8(bytes).ok(),
        Err(err) => {
            tracing::error!(counterpart = %row.counterpart.id, error = %err, "A2A auth header not decrypted");
            None
        }
    }
}

/// Sends a task to the counterpart and waits for its end: streamed when the
/// card says so, else sent and polled. Bounded by `outbound.deadline` and
/// `outbound.max_bytes` (the caller also bounds the whole call in time).
pub async fn send_task(
    state: &AppState,
    row: &CounterpartRow,
    outbound: &Outbound<'_>,
) -> Result<Remote, OutboundError> {
    let counterpart = &row.counterpart;
    let endpoint = counterpart.endpoint();
    let task_id = Uuid::now_v7();
    let mut metadata = outbound.hops.metadata();
    metadata["nexc.principal"] = json!(outbound.principal);
    let params = json!({
        "id": task_id,
        "sessionId": outbound.session_id,
        "message": {"role": "user", "parts": [{"type": "text", "text": outbound.message}]},
        "metadata": metadata,
    });
    let header = auth_header(state, row);
    let post = |method: &str, params: Value| {
        let mut request = state
            .gateway_http
            .post(&endpoint)
            .header("content-type", "application/json")
            .header(
                "accept",
                if method == "tasks/sendSubscribe" {
                    "text/event-stream"
                } else {
                    "application/json"
                },
            )
            .json(&json!({"jsonrpc": "2.0", "id": Uuid::now_v7().to_string(), "method": method, "params": params}));
        if let Some(header) = &header {
            request = request.header("authorization", header);
        }
        request.send()
    };
    let mut remote = Remote {
        task_id,
        state: TaskState::Submitted,
        texts: Vec::new(),
        files: Vec::new(),
        message: None,
        bytes_in: 0,
        status: None,
        streamed: counterpart.streams(),
    };
    if counterpart.streams() {
        let response = post("tasks/sendSubscribe", params)
            .await
            .map_err(transport)?;
        let status = response.status().as_u16();
        remote.status = Some(status);
        if status != 200 {
            let body = read_all(response, outbound.max_bytes)
                .await
                .unwrap_or_default();
            if let Ok(reply) = serde_json::from_slice::<Value>(&body) {
                rpc_result(&reply)?;
            }
            return Err(OutboundError::Status(status));
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        if !content_type.starts_with("text/event-stream") {
            // A plain reply: the task as it stands.
            let body = read_all(response, outbound.max_bytes).await?;
            remote.bytes_in = body.len();
            let reply: Value = serde_json::from_slice(&body)
                .map_err(|e| OutboundError::Protocol(format!("the reply is not JSON: {e}")))?;
            let task = rpc_result(&reply)?;
            absorb_task(&mut remote, &task);
            remote.streamed = false;
            if !remote.state.is_final() {
                poll(&post, &mut remote, outbound).await?;
            }
            return Ok(remote);
        }
        let mut stream = response.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        let mut done = false;
        while !done {
            let chunk = tokio::time::timeout_at(outbound.deadline.into(), stream.next())
                .await
                .map_err(|_| OutboundError::Transport("the stream did not end in time".into()))?;
            let Some(chunk) = chunk else { break };
            let chunk = chunk.map_err(transport)?;
            remote.bytes_in += chunk.len();
            if remote.bytes_in > outbound.max_bytes {
                return Err(OutboundError::Oversized(outbound.max_bytes));
            }
            buf.extend_from_slice(&chunk);
            // Complete events end with a blank line; the rest waits for more.
            while let Some(at) = find_event_end(&buf) {
                let event: Vec<u8> = buf.drain(..at + 2).collect();
                let data: String = String::from_utf8_lossy(&event)
                    .lines()
                    .filter_map(|line| line.strip_prefix("data:"))
                    .map(str::trim_start)
                    .collect::<Vec<_>>()
                    .join("\n");
                if data.is_empty() {
                    continue;
                }
                let Ok(reply) = serde_json::from_str::<Value>(&data) else {
                    continue;
                };
                let result = rpc_result(&reply)?;
                if let Some(artifact) = result.get("artifact") {
                    let parts = artifact
                        .get("parts")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    remote.texts.extend(text_of(&parts));
                    remote.files.extend(files_of(&parts));
                }
                if let Some(status) = result.get("status") {
                    if let Some(state) = status
                        .get("state")
                        .and_then(Value::as_str)
                        .and_then(TaskState::parse)
                    {
                        remote.state = state;
                    }
                    if let Some(message) = status_message(status) {
                        remote.message = Some(message);
                    }
                    if result.get("final").and_then(Value::as_bool) == Some(true) {
                        done = true;
                        break;
                    }
                }
            }
        }
        if !remote.state.is_final() {
            // The stream ended early (or without a final event): ask where the task stands.
            poll(&post, &mut remote, outbound).await?;
        }
        return Ok(remote);
    }
    let response = post("tasks/send", params).await.map_err(transport)?;
    let status = response.status().as_u16();
    remote.status = Some(status);
    let body = read_all(response, outbound.max_bytes).await?;
    remote.bytes_in = body.len();
    let reply: Value = serde_json::from_slice(&body).map_err(|e| {
        if status == 200 {
            OutboundError::Protocol(format!("the reply is not JSON: {e}"))
        } else {
            OutboundError::Status(status)
        }
    })?;
    let task = rpc_result(&reply)?;
    if status != 200 {
        return Err(OutboundError::Status(status));
    }
    absorb_task(&mut remote, &task);
    if !remote.state.is_final() {
        poll(&post, &mut remote, outbound).await?;
    }
    Ok(remote)
}

/// Where a complete SSE event ends in `buf` (the blank line), if one does.
fn find_event_end(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n")
}

/// `tasks/get` until the task ended or the deadline passed.
async fn poll<F, Fut>(
    post: &F,
    remote: &mut Remote,
    outbound: &Outbound<'_>,
) -> Result<(), OutboundError>
where
    F: Fn(&str, Value) -> Fut,
    Fut: Future<Output = Result<reqwest::Response, reqwest::Error>>,
{
    while Instant::now() < outbound.deadline && !remote.state.is_final() {
        tokio::time::sleep(POLL).await;
        let response = post(
            "tasks/get",
            json!({"id": remote.task_id, "historyLength": 0}),
        )
        .await
        .map_err(transport)?;
        let status = response.status().as_u16();
        if status != 200 {
            return Err(OutboundError::Status(status));
        }
        let body = read_all(response, outbound.max_bytes).await?;
        remote.bytes_in += body.len();
        let reply: Value = serde_json::from_slice(&body)
            .map_err(|e| OutboundError::Protocol(format!("the reply is not JSON: {e}")))?;
        let task = rpc_result(&reply)?;
        absorb_task(remote, &task);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_and_tasks_are_read() {
        let task = json!({
            "status": {"state": "completed", "message": {"parts": [{"type": "text", "text": "done"}]}},
            "artifacts": [
                {"name": "answer", "parts": [{"type": "text", "text": "the figures"}, {"type": "data", "data": {"n": 1}}]},
                {"name": "report.docx", "parts": [{"type": "file", "file": {"name": "report.docx", "mimeType": "application/x", "bytes": "UEs="}}]},
                {"name": "big.bin", "parts": [{"type": "file", "file": {"name": "big.bin", "uri": "https://remote.example/big"}}]}
            ]
        });
        let mut remote = Remote {
            task_id: Uuid::now_v7(),
            state: TaskState::Submitted,
            texts: vec![],
            files: vec![],
            message: None,
            bytes_in: 0,
            status: None,
            streamed: false,
        };
        absorb_task(&mut remote, &task);
        assert_eq!(remote.state, TaskState::Completed);
        assert_eq!(remote.message.as_deref(), Some("done"));
        assert_eq!(remote.texts.len(), 2);
        assert_eq!(remote.files[0].bytes.as_deref(), Some(b"PK".as_slice()));
        assert_eq!(
            remote.files[1].uri.as_deref(),
            Some("https://remote.example/big")
        );
        assert!(rpc_result(&json!({"jsonrpc": "2.0", "id": 1, "result": {"ok": true}})).is_ok());
        let refused = rpc_result(
            &json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32011, "message": "no"}}),
        );
        assert_eq!(
            refused,
            Err(OutboundError::Remote {
                code: -32011,
                message: "no".into()
            })
        );
        assert!(rpc_result(&json!({"jsonrpc": "2.0", "id": 1})).is_err());
        assert_eq!(find_event_end(b"data: x\n\nrest"), Some(7));
        assert_eq!(find_event_end(b"data: x\n"), None);
        assert_eq!(OutboundError::Status(502).status(), Some(502));
    }
}
