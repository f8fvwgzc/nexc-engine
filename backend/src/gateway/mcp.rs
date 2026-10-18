//! A small MCP client: JSON-RPC 2.0 to a server over the standard streams
//! of a child process, or over streamable HTTP. One session per call:
//! `initialize`, `notifications/initialized`, then `tools/list` or
//! `tools/call`, so a server's state never crosses calls and a crashed
//! server costs one call.
//!
//! A child process is started with an empty environment (`PATH` and a
//! scratch `HOME` only), in a scratch directory of its own, with its
//! standard error discarded, and is killed when the session ends. Every
//! message read is bounded in size.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use futures::StreamExt;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::domain::mcp::{McpTool, TOOL_NAME_MAX};

/// The protocol revision this client speaks.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
/// Pages of `tools/list` followed at most.
const LIST_PAGES_MAX: usize = 10;
/// Tools kept from one server.
const TOOLS_MAX: usize = 256;

/// Why a session failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum McpError {
    #[error("the server could not be started: {0}")]
    Spawn(String),
    #[error("the server is not reachable: {0}")]
    Transport(String),
    #[error("the server answered badly: {0}")]
    Protocol(String),
    #[error("the server's answer is larger than {0} bytes")]
    Oversized(usize),
    #[error("the server refused ({code}): {message}")]
    Rpc { code: i64, message: String },
}

/// What `tools/call` returned, as text for the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallOutput {
    pub text: String,
    pub is_error: bool,
    /// Size of the server's whole answer on the wire.
    pub bytes: usize,
}

/// Where a session goes.
pub enum Target<'a> {
    Stdio {
        command: &'a str,
        args: &'a [String],
        /// A directory of the session's own, the child's working directory and `HOME`.
        scratch: &'a Path,
    },
    Http {
        client: &'a reqwest::Client,
        url: &'a str,
        auth_header: Option<&'a str>,
    },
}

enum Transport {
    Stdio {
        child: Child,
        stdin: ChildStdin,
        stdout: BufReader<ChildStdout>,
        scratch: PathBuf,
    },
    Http {
        client: reqwest::Client,
        url: String,
        auth_header: Option<String>,
        session_id: Option<String>,
    },
}

/// An open session with a server.
pub struct Session {
    transport: Transport,
    next_id: u64,
    max_bytes: usize,
    /// What the server said it is.
    pub server_info: String,
}

impl Session {
    /// Opens a session: starts the child or the HTTP exchange, then
    /// `initialize` and `notifications/initialized`.
    pub async fn open(target: Target<'_>, max_bytes: usize) -> Result<Self, McpError> {
        let transport = match target {
            Target::Stdio {
                command,
                args,
                scratch,
            } => {
                tokio::fs::create_dir_all(scratch)
                    .await
                    .map_err(|e| McpError::Spawn(format!("scratch directory: {e}")))?;
                let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
                let mut child = Command::new(command)
                    .args(args)
                    .env_clear()
                    .env("PATH", path)
                    .env("HOME", scratch)
                    .env("TMPDIR", scratch)
                    .env("LANG", "C.UTF-8")
                    .current_dir(scratch)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .kill_on_drop(true)
                    .spawn()
                    .map_err(|e| McpError::Spawn(format!("{command}: {e}")))?;
                let stdin = child
                    .stdin
                    .take()
                    .ok_or_else(|| McpError::Spawn("no stdin".into()))?;
                let stdout = child
                    .stdout
                    .take()
                    .ok_or_else(|| McpError::Spawn("no stdout".into()))?;
                Transport::Stdio {
                    child,
                    stdin,
                    stdout: BufReader::new(stdout),
                    scratch: scratch.to_path_buf(),
                }
            }
            Target::Http {
                client,
                url,
                auth_header,
            } => Transport::Http {
                client: client.clone(),
                url: url.to_owned(),
                auth_header: auth_header.map(str::to_owned),
                session_id: None,
            },
        };
        let mut session = Session {
            transport,
            next_id: 0,
            max_bytes,
            server_info: String::new(),
        };
        let reply = session
            .request(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {"name": "nexc-engine", "version": env!("CARGO_PKG_VERSION")},
                }),
            )
            .await?;
        let info = &reply["serverInfo"];
        session.server_info = format!(
            "{} {}",
            info["name"].as_str().unwrap_or("server"),
            info["version"].as_str().unwrap_or("")
        )
        .trim()
        .to_owned();
        session
            .notify("notifications/initialized", json!({}))
            .await?;
        Ok(session)
    }

    /// The tools the server lists.
    pub async fn list_tools(&mut self) -> Result<Vec<McpTool>, McpError> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..LIST_PAGES_MAX {
            let params = match &cursor {
                Some(cursor) => json!({"cursor": cursor}),
                None => json!({}),
            };
            let reply = self.request("tools/list", params).await?;
            for tool in reply["tools"].as_array().into_iter().flatten() {
                let Some(name) = tool["name"].as_str() else {
                    continue;
                };
                if name.is_empty() || name.len() > TOOL_NAME_MAX || tools.len() >= TOOLS_MAX {
                    continue;
                }
                tools.push(McpTool {
                    name: name.to_owned(),
                    description: tool["description"]
                        .as_str()
                        .unwrap_or_default()
                        .chars()
                        .take(2_000)
                        .collect(),
                    input_schema: match &tool["inputSchema"] {
                        Value::Object(schema) => Value::Object(schema.clone()),
                        _ => json!({"type": "object", "properties": {}}),
                    },
                });
            }
            cursor = reply["nextCursor"].as_str().map(str::to_owned);
            if cursor.is_none() {
                break;
            }
        }
        Ok(tools)
    }

    /// Calls a tool; its content as text.
    pub async fn call_tool(
        &mut self,
        name: &str,
        arguments: &Value,
    ) -> Result<ToolCallOutput, McpError> {
        let (reply, bytes) = self
            .request_sized("tools/call", json!({"name": name, "arguments": arguments}))
            .await?;
        let mut text = String::new();
        for item in reply["content"].as_array().into_iter().flatten() {
            if !text.is_empty() {
                text.push('\n');
            }
            match item["type"].as_str() {
                Some("text") => text.push_str(item["text"].as_str().unwrap_or_default()),
                Some(other) => text.push_str(&format!(
                    "[{other} content omitted: the gateway passes text only]"
                )),
                None => text.push_str(&item.to_string()),
            }
        }
        if text.is_empty()
            && let Some(structured) = reply.get("structuredContent")
        {
            text = structured.to_string();
        }
        Ok(ToolCallOutput {
            text,
            is_error: reply["isError"].as_bool().unwrap_or(false),
            bytes,
        })
    }

    /// Ends the session; a child is killed.
    pub async fn close(self) {
        if let Transport::Stdio {
            mut child, scratch, ..
        } = self.transport
        {
            let _ = child.start_kill();
            let _ = child.wait().await;
            let _ = tokio::fs::remove_dir_all(scratch).await;
        }
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, McpError> {
        self.request_sized(method, params).await.map(|(v, _)| v)
    }

    /// Sends a request and waits for its reply; the reply's size on the wire too.
    async fn request_sized(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<(Value, usize), McpError> {
        self.next_id += 1;
        let id = self.next_id;
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let (reply, bytes) = match &mut self.transport {
            Transport::Stdio { stdin, stdout, .. } => {
                write_line(stdin, &message).await?;
                read_reply(stdin, stdout, id, self.max_bytes).await?
            }
            Transport::Http {
                client,
                url,
                auth_header,
                session_id,
            } => {
                let mut request = client
                    .post(url.as_str())
                    .header("content-type", "application/json")
                    .header("accept", "application/json, text/event-stream")
                    .header("mcp-protocol-version", PROTOCOL_VERSION)
                    .json(&message);
                if let Some(auth) = auth_header {
                    request = request.header("authorization", auth.as_str());
                }
                if let Some(session) = session_id {
                    request = request.header("mcp-session-id", session.as_str());
                }
                let response = request
                    .send()
                    .await
                    .map_err(|e| McpError::Transport(e.without_url().to_string()))?;
                if let Some(session) = response
                    .headers()
                    .get("mcp-session-id")
                    .and_then(|v| v.to_str().ok())
                {
                    *session_id = Some(session.to_owned());
                }
                let status = response.status();
                if !status.is_success() {
                    return Err(McpError::Transport(format!("HTTP {status}")));
                }
                let event_stream = response
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|ct| ct.starts_with("text/event-stream"));
                let (body, bytes) = read_body(response, self.max_bytes, event_stream, id).await?;
                (body, bytes)
            }
        };
        if let Some(error) = reply.get("error") {
            return Err(McpError::Rpc {
                code: error["code"].as_i64().unwrap_or(0),
                message: error["message"]
                    .as_str()
                    .unwrap_or("unknown error")
                    .chars()
                    .take(500)
                    .collect(),
            });
        }
        match reply.get("result") {
            Some(result) => Ok((result.clone(), bytes)),
            None => Err(McpError::Protocol("a reply without a result".into())),
        }
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), McpError> {
        let message = json!({"jsonrpc": "2.0", "method": method, "params": params});
        match &mut self.transport {
            Transport::Stdio { stdin, .. } => write_line(stdin, &message).await,
            Transport::Http {
                client,
                url,
                auth_header,
                session_id,
            } => {
                let mut request = client
                    .post(url.as_str())
                    .header("content-type", "application/json")
                    .header("accept", "application/json, text/event-stream")
                    .header("mcp-protocol-version", PROTOCOL_VERSION)
                    .json(&message);
                if let Some(auth) = auth_header {
                    request = request.header("authorization", auth.as_str());
                }
                if let Some(session) = session_id {
                    request = request.header("mcp-session-id", session.as_str());
                }
                let response = request
                    .send()
                    .await
                    .map_err(|e| McpError::Transport(e.without_url().to_string()))?;
                if !response.status().is_success() {
                    return Err(McpError::Transport(format!(
                        "HTTP {} to a notification",
                        response.status()
                    )));
                }
                Ok(())
            }
        }
    }
}

async fn write_line(stdin: &mut ChildStdin, message: &Value) -> Result<(), McpError> {
    let mut line = serde_json::to_vec(message).expect("a JSON-RPC message serialises");
    line.push(b'\n');
    stdin
        .write_all(&line)
        .await
        .map_err(|e| McpError::Transport(format!("writing to the server: {e}")))?;
    stdin
        .flush()
        .await
        .map_err(|e| McpError::Transport(format!("writing to the server: {e}")))
}

/// Reads lines until the reply to `id`; notifications are ignored and
/// requests of the server are refused politely.
async fn read_reply(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    id: u64,
    max_bytes: usize,
) -> Result<(Value, usize), McpError> {
    loop {
        let mut line = Vec::new();
        let read = {
            let mut bounded = stdout.take(max_bytes as u64 + 1);
            bounded
                .read_until(b'\n', &mut line)
                .await
                .map_err(|e| McpError::Transport(format!("reading from the server: {e}")))?
        };
        if read == 0 {
            return Err(McpError::Transport("the server closed its output".into()));
        }
        if line.len() > max_bytes {
            return Err(McpError::Oversized(max_bytes));
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(text) else {
            // Servers that log to stdout break the protocol; skip the noise.
            continue;
        };
        match (message.get("id"), message.get("method")) {
            (Some(reply_id), None) if reply_id.as_u64() == Some(id) => {
                return Ok((message, line.len()));
            }
            (Some(request_id), Some(_)) => {
                let refusal = json!({"jsonrpc": "2.0", "id": request_id,
                    "error": {"code": -32601, "message": "not supported by this client"}});
                write_line(stdin, &refusal).await?;
            }
            _ => {}
        }
    }
}

/// Reads an HTTP reply (JSON, or an event stream carrying JSON-RPC
/// messages) up to `max_bytes`, until the reply to `id`.
async fn read_body(
    response: reqwest::Response,
    max_bytes: usize,
    event_stream: bool,
    id: u64,
) -> Result<(Value, usize), McpError> {
    let mut stream = response.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| McpError::Transport(e.without_url().to_string()))?;
        buf.extend_from_slice(&chunk);
        if buf.len() > max_bytes {
            return Err(McpError::Oversized(max_bytes));
        }
        if event_stream && let Some(found) = find_in_events(&buf, id) {
            return Ok((found, buf.len()));
        }
    }
    if event_stream {
        return find_in_events(&buf, id)
            .map(|v| (v, buf.len()))
            .ok_or_else(|| McpError::Protocol("the event stream ended without the reply".into()));
    }
    let message: Value = serde_json::from_slice(&buf)
        .map_err(|e| McpError::Protocol(format!("the reply is not JSON: {e}")))?;
    let bytes = buf.len();
    // A batch: the reply is the element with our id.
    if let Value::Array(items) = &message {
        return items
            .iter()
            .find(|m| m["id"].as_u64() == Some(id))
            .cloned()
            .map(|v| (v, bytes))
            .ok_or_else(|| McpError::Protocol("the batch holds no reply to the request".into()));
    }
    Ok((message, bytes))
}

/// The JSON-RPC reply to `id` among the complete events of an SSE body.
fn find_in_events(buf: &[u8], id: u64) -> Option<Value> {
    let text = String::from_utf8_lossy(buf);
    for event in text.split("\n\n") {
        let data: String = event
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim_start)
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        let matches = |m: &Value| m.get("method").is_none() && m["id"].as_u64() == Some(id);
        match &message {
            Value::Array(items) => {
                if let Some(found) = items.iter().find(|m| matches(m)) {
                    return Some(found.clone());
                }
            }
            m if matches(m) => return Some(m.clone()),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_bodies_are_scanned_for_the_reply() {
        let body =
            b"event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/x\"}\n\n\
            data: {\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"ok\":true}}\n\n";
        assert_eq!(find_in_events(body, 3).unwrap()["result"]["ok"], true);
        assert!(find_in_events(body, 4).is_none());
        let partial = b"data: {\"jsonrpc\":\"2.0\",\"id\":3,\"res";
        assert!(find_in_events(partial, 3).is_none());
    }
}
