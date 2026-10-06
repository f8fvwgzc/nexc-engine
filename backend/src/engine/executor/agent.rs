//! Executor backed by the Python agent runtime (`POST /v1/execute`, NDJSON
//! stream; contract §8).

use base64::Engine as _;
use futures::StreamExt;
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::json;

use super::super::artifacts::MAX_ARTIFACT_BYTES;
use super::{ExecContext, ExecError, ExecOutput, NodeExecutor};
use crate::realtime::events::LogLevel;

/// Longest NDJSON line accepted (a base64 artifact of 20 MiB plus envelope).
const MAX_LINE_BYTES: usize = MAX_ARTIFACT_BYTES / 3 * 4 + 64 * 1024;
const MAX_TURNS: u32 = 12;

/// Executes nodes with `executor: "agent"`.
pub struct AgentExecutor;

impl NodeExecutor for AgentExecutor {
    fn execute<'a>(&'a self, ctx: &'a ExecContext) -> BoxFuture<'a, Result<ExecOutput, ExecError>> {
        Box::pin(run(ctx))
    }
}

#[derive(Debug, Deserialize)]
struct SpawnedAgent {
    name: String,
    role: String,
}

/// One line of the runtime's NDJSON stream.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RuntimeLine {
    Log {
        #[serde(default)]
        level: String,
        message: String,
    },
    Delta {
        text: String,
    },
    Tokens {
        #[serde(default)]
        input: i64,
        #[serde(default)]
        output: i64,
    },
    Spawn {
        agent: SpawnedAgent,
    },
    Artifact {
        path: String,
        #[serde(default)]
        mime: Option<String>,
        content_b64: String,
    },
    Result {
        output: String,
        #[serde(default)]
        tokens_in: i64,
        #[serde(default)]
        tokens_out: i64,
    },
    Error {
        message: String,
        #[serde(default)]
        retryable: bool,
    },
    #[serde(other)]
    Unknown,
}

fn request_body(ctx: &ExecContext) -> serde_json::Value {
    let target = ctx.agent_target();
    let agent = ctx.agent.as_ref();
    let settings = &ctx.state.settings;
    json!({
        "run_id": ctx.run_id,
        "node_id": ctx.node.id,
        "agent": {
            "name": agent.map_or("assistant", |a| a.name.as_str()),
            "role": agent.map_or("assistant", |a| a.role.as_str()),
            "system_prompt": ctx.system_prompt(),
            "model": target.model,
            "budget_tokens": agent.map_or(0, |a| (a.budget_tokens - a.spent_tokens).max(0)),
        },
        "task": {
            "title": ctx.node.title,
            "content": ctx.node.content,
            "kind": ctx.node.kind,
            "kind_description": ctx.node_type.as_ref().map_or("", |t| t.description.as_str()),
            "produces_artifact": ctx.node_type.as_ref().is_some_and(|t| t.produces_artifact),
        },
        "context": {
            "goal": ctx.goal,
            "upstream": ctx.upstream.iter().map(|u| json!({
                "node_id": u.node_id, "title": u.title, "output": u.output,
            })).collect::<Vec<_>>(),
            "memories": ctx.memories,
        },
        "llm": {
            "provider": target.provider,
            "api_key": target.api_key.as_ref().map(|k| k.expose().as_str()),
            "model": target.model,
            "base_url": target.base_url,
        },
        "limits": {
            "max_turns": MAX_TURNS,
            "timeout_s": settings.node_timeout.as_secs(),
            "allow_code_exec": ctx.node_type.as_ref().is_some_and(|t| t.allow_code_exec),
        },
    })
}

async fn run(ctx: &ExecContext) -> Result<ExecOutput, ExecError> {
    let settings = &ctx.state.settings;
    let url = format!("{}/v1/execute", settings.runtime_url.trim_end_matches('/'));
    let resp = ctx
        .state
        .http
        .post(&url)
        .bearer_auth(settings.runtime_token.expose())
        .json(&request_body(ctx))
        .send()
        .await
        .map_err(|e| {
            ExecError::transient(format!("agent runtime unreachable: {}", e.without_url()))
        })?;
    let status = resp.status();
    if !status.is_success() {
        let message = format!("agent runtime returned HTTP {status}");
        return Err(if status.is_server_error() {
            ExecError::transient(message)
        } else {
            ExecError::fatal(message)
        });
    }
    let mut bytes = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    let mut usage = (0i64, 0i64);
    while let Some(chunk) = bytes.next().await {
        let chunk = chunk.map_err(|e| {
            ExecError::transient(format!("agent runtime stream failed: {}", e.without_url()))
        })?;
        buf.extend_from_slice(&chunk);
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buf.drain(..=pos).collect();
            if let Some(done) = handle_line(ctx, &line, &mut usage).await? {
                return Ok(done);
            }
        }
        if buf.len() > MAX_LINE_BYTES {
            return Err(ExecError::fatal("agent runtime sent an oversized line"));
        }
    }
    if let Some(done) = handle_line(ctx, &buf, &mut usage).await? {
        return Ok(done);
    }
    Err(ExecError::transient(
        "agent runtime stream ended without a result",
    ))
}

/// Handles one NDJSON line; returns the output once the `result` line arrives.
async fn handle_line(
    ctx: &ExecContext,
    line: &[u8],
    usage: &mut (i64, i64),
) -> Result<Option<ExecOutput>, ExecError> {
    let text = String::from_utf8_lossy(line);
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let parsed: RuntimeLine = serde_json::from_str(text)
        .map_err(|e| ExecError::fatal(format!("agent runtime sent invalid JSON: {e}")))?;
    match parsed {
        RuntimeLine::Log { level, message } => ctx.log(parse_level(&level), message),
        RuntimeLine::Delta { text } => ctx.output(&text),
        RuntimeLine::Tokens { input, output } => {
            usage.0 += input.max(0);
            usage.1 += output.max(0);
            ctx.tokens(usage.0, usage.1);
        }
        RuntimeLine::Spawn { agent } => {
            ctx.log(
                LogLevel::Info,
                format!("spawned sub-agent {} ({})", agent.name, agent.role),
            );
        }
        RuntimeLine::Artifact {
            path,
            mime,
            content_b64,
        } => {
            let saved = base64::engine::general_purpose::STANDARD
                .decode(content_b64.as_bytes())
                .map_err(|e| format!("invalid base64: {e}"));
            match saved {
                Ok(bytes) => {
                    if let Err(err) = ctx.artifact(&path, mime.as_deref(), &bytes).await {
                        ctx.log(LogLevel::Warn, format!("artifact `{path}` rejected: {err}"));
                    }
                }
                Err(err) => ctx.log(LogLevel::Warn, format!("artifact `{path}` rejected: {err}")),
            }
        }
        RuntimeLine::Result {
            output,
            tokens_in,
            tokens_out,
        } => {
            let (tokens_in, tokens_out) = (tokens_in.max(usage.0), tokens_out.max(usage.1));
            ctx.tokens(tokens_in, tokens_out);
            return Ok(Some(ExecOutput {
                output,
                tokens_in,
                tokens_out,
            }));
        }
        RuntimeLine::Error { message, retryable } => {
            return Err(ExecError {
                message: format!("agent runtime: {message}"),
                retryable,
            });
        }
        RuntimeLine::Unknown => {}
    }
    Ok(None)
}

fn parse_level(level: &str) -> LogLevel {
    match level {
        "debug" => LogLevel::Debug,
        "warn" | "warning" => LogLevel::Warn,
        "error" => LogLevel::Error,
        _ => LogLevel::Info,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_runtime_lines() {
        let lines = [
            r#"{"type":"log","level":"info","message":"m"}"#,
            r#"{"type":"delta","text":"t"}"#,
            r#"{"type":"tokens","input":1,"output":2}"#,
            r#"{"type":"spawn","agent":{"name":"n","role":"r"}}"#,
            r#"{"type":"artifact","path":"a.md","mime":"text/markdown","content_b64":"aGk="}"#,
            r#"{"type":"result","output":"o","tokens_in":0,"tokens_out":0}"#,
            r#"{"type":"error","message":"boom","retryable":true}"#,
            r#"{"type":"future_thing","x":1}"#,
        ];
        let parsed: Vec<RuntimeLine> = lines
            .iter()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert!(matches!(parsed[5], RuntimeLine::Result { .. }));
        assert!(matches!(
            parsed[6],
            RuntimeLine::Error {
                retryable: true,
                ..
            }
        ));
        assert!(matches!(parsed[7], RuntimeLine::Unknown));
        assert_eq!(parse_level("warning"), LogLevel::Warn);
    }
}
