//! `claude_code` provider: runs the local [Claude Code](https://code.claude.com) CLI in headless
//! mode (`claude -p`) so nexc can use the operator's Claude subscription instead of an API key.
//!
//! Every request is one isolated CLI invocation: built-in tools, MCP servers, settings files
//! (hooks, plugins) and session persistence are all disabled, the prompt goes over stdin, and the
//! child gets a minimal environment (no nexc secrets, no `ANTHROPIC_API_KEY`, so the CLI's own
//! login is used). Text is streamed from `--output-format stream-json`; structured output uses
//! `--json-schema` and is returned as the JSON text the callers already parse.

use std::path::PathBuf;
use std::process::Stdio;

use async_stream::stream;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use super::{ChatRole, LlmError, LlmEvent, LlmProvider, LlmRequest, LlmStream, StopReason, Usage};

/// Model used when the configured one is empty.
pub const DEFAULT_MODEL: &str = "sonnet";

/// Bounds for `CLAUDE_CODE_MAX_OUTPUT_TOKENS`: the CLI's default cap is lower than what a long
/// streamed node output may need, so it is raised to the request's `max_tokens`.
const OUTPUT_TOKENS: std::ops::RangeInclusive<u32> = 8_192..=64_000;

/// Failures a retry cannot fix; anything else from the CLI is treated as transient.
const FATAL_MARKERS: &[&str] = &[
    "login",
    "log in",
    "api key",
    "credit",
    "billing",
    "unauthorized",
    "forbidden",
];

/// Environment variables passed through to the CLI; everything else is dropped.
const PASSTHROUGH_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TMPDIR",
    "LANG",
    "LC_ALL",
    "CLAUDE_CONFIG_DIR",
    "NODE_EXTRA_CA_CERTS",
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "NO_PROXY",
];

/// Runs `claude -p` per request.
#[derive(Debug, Clone)]
pub struct ClaudeCodeProvider {
    bin: String,
    workdir: PathBuf,
}

impl ClaudeCodeProvider {
    /// Provider invoking `bin`; `workdir` is an empty scratch directory used as the CLI's cwd so
    /// no project `CLAUDE.md` or settings are picked up.
    pub fn new(bin: impl Into<String>, workdir: PathBuf) -> Self {
        ClaudeCodeProvider {
            bin: bin.into(),
            workdir,
        }
    }

    fn command(&self, req: &LlmRequest) -> Command {
        let model = if req.target.model.trim().is_empty() {
            DEFAULT_MODEL
        } else {
            req.target.model.trim()
        };
        let mut cmd = Command::new(&self.bin);
        cmd.args(["-p", "--output-format", "stream-json", "--verbose"])
            .args(["--include-partial-messages", "--model", model])
            .args(["--tools", "", "--setting-sources", ""])
            .args(["--strict-mcp-config", "--no-session-persistence"])
            .args(["--system-prompt", &req.system]);
        if let Some(schema) = &req.json_schema {
            cmd.args(["--json-schema", &schema.schema.to_string()]);
        }
        cmd.current_dir(&self.workdir)
            .env_clear()
            .envs(
                PASSTHROUGH_ENV
                    .iter()
                    .filter_map(|k| std::env::var(k).ok().map(|v| (*k, v))),
            )
            .env(
                "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
                req.max_tokens
                    .clamp(*OUTPUT_TOKENS.start(), *OUTPUT_TOKENS.end())
                    .to_string(),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        cmd
    }
}

/// The conversation as one prompt: a single user message verbatim, otherwise a labelled
/// transcript (the CLI takes one prompt per invocation).
fn render_prompt(req: &LlmRequest) -> String {
    if let [only] = req.messages.as_slice() {
        return only.content.clone();
    }
    req.messages
        .iter()
        .map(|m| {
            let who = match m.role {
                ChatRole::User => "User",
                ChatRole::Assistant => "Assistant",
            };
            format!("### {who}\n\n{}", m.content)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// What one line of `stream-json` output means for us.
#[derive(Debug)]
enum Line {
    Text(String),
    Finished {
        text: Option<String>,
        usage: Usage,
        stop: StopReason,
    },
    Failed(LlmError),
    Ignore,
}

fn parse_line(line: &str, structured: bool) -> Line {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Line::Ignore;
    };
    match v["type"].as_str() {
        Some("stream_event") if !structured => {
            let delta = &v["event"]["delta"];
            match (v["event"]["type"].as_str(), delta["type"].as_str()) {
                (Some("content_block_delta"), Some("text_delta")) => {
                    Line::Text(delta["text"].as_str().unwrap_or_default().to_owned())
                }
                _ => Line::Ignore,
            }
        }
        Some("result") => parse_result(&v, structured),
        _ => Line::Ignore,
    }
}

/// Message of a failed result: its `subtype` plus the `errors` list (or the `result` text).
fn failure_message(v: &Value) -> String {
    let subtype = v["subtype"].as_str().unwrap_or("error");
    let detail = match v["errors"].as_array() {
        Some(errors) if !errors.is_empty() => errors
            .iter()
            .map(|e| e.as_str().map_or_else(|| e.to_string(), str::to_owned))
            .collect::<Vec<_>>()
            .join("; "),
        _ => v["result"].as_str().unwrap_or_default().to_owned(),
    };
    if detail.is_empty() {
        subtype.to_owned()
    } else {
        format!("{subtype}: {detail}")
    }
}

fn parse_result(v: &Value, structured: bool) -> Line {
    if v["is_error"].as_bool().unwrap_or(false) || v["subtype"] != "success" {
        let message = failure_message(v);
        let lower = message.to_lowercase();
        let retryable = !FATAL_MARKERS.iter().any(|m| lower.contains(m));
        return Line::Failed(LlmError::Http {
            status: if retryable { 503 } else { 400 },
            message: format!("claude CLI: {}", truncate(&message, 300)),
            retryable,
        });
    }
    let u = &v["usage"];
    let n = |k: &str| u[k].as_u64().unwrap_or(0);
    let usage = Usage {
        input_tokens: n("input_tokens")
            + n("cache_creation_input_tokens")
            + n("cache_read_input_tokens"),
        output_tokens: n("output_tokens"),
        cached_tokens: n("cache_read_input_tokens"),
    };
    let stop = if v["stop_reason"] == "max_tokens" {
        StopReason::MaxTokens
    } else {
        StopReason::EndTurn
    };
    let text = if structured {
        match &v["structured_output"] {
            Value::Null => {
                return Line::Failed(LlmError::Protocol(
                    "claude CLI returned no structured output".into(),
                ));
            }
            out => Some(out.to_string()),
        }
    } else {
        None
    };
    Line::Finished { text, usage, stop }
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

impl LlmProvider for ClaudeCodeProvider {
    fn stream(&self, req: LlmRequest) -> LlmStream {
        let mut cmd = self.command(&req);
        let prompt = render_prompt(&req);
        let structured = req.json_schema.is_some();
        let workdir = self.workdir.clone();
        let bin = self.bin.clone();
        Box::pin(stream! {
            if let Err(err) = tokio::fs::create_dir_all(&workdir).await {
                yield Err(LlmError::Protocol(format!("cannot create {}: {err}", workdir.display())));
                return;
            }
            let mut child = match cmd.spawn() {
                Ok(child) => child,
                Err(err) => {
                    yield Err(LlmError::Protocol(format!(
                        "cannot start `{bin}` ({err}); install Claude Code and run `claude` once to log in, or set NEXC_CLAUDE_BIN"
                    )));
                    return;
                }
            };
            let (Some(mut stdin), Some(stdout), Some(mut stderr)) =
                (child.stdin.take(), child.stdout.take(), child.stderr.take())
            else {
                yield Err(LlmError::Protocol("claude CLI pipes unavailable".into()));
                return;
            };
            // Write the prompt concurrently so a large prompt can't deadlock against stdout.
            let writer = tokio::spawn(async move {
                let _ = stdin.write_all(prompt.as_bytes()).await;
                let _ = stdin.shutdown().await;
            });
            let mut lines = BufReader::new(stdout).lines();
            let mut finished = false;
            while let Ok(Some(line)) = lines.next_line().await {
                match parse_line(&line, structured) {
                    Line::Text(t) if !t.is_empty() => yield Ok(LlmEvent::Text(t)),
                    Line::Finished { text, usage, stop } => {
                        if let Some(text) = text {
                            yield Ok(LlmEvent::Text(text));
                        }
                        yield Ok(LlmEvent::Usage(usage));
                        yield Ok(LlmEvent::Done(stop));
                        finished = true;
                        break;
                    }
                    Line::Failed(err) => {
                        yield Err(err);
                        finished = true;
                        break;
                    }
                    _ => {}
                }
            }
            let _ = writer.await;
            let status = child.wait().await;
            if !finished {
                let mut err_text = String::new();
                let _ = stderr.read_to_string(&mut err_text).await;
                let code = status.map(|s| s.to_string()).unwrap_or_else(|e| e.to_string());
                yield Err(LlmError::Network(format!(
                    "claude CLI exited ({code}) without a result: {}",
                    truncate(err_text.trim(), 300)
                )));
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::settings::LlmProviderKind;
    use crate::llm::{LlmTarget, Message};

    fn request(messages: Vec<Message>) -> LlmRequest {
        LlmRequest {
            target: LlmTarget {
                provider: LlmProviderKind::ClaudeCode,
                model: String::new(),
                base_url: None,
                api_key: None,
            },
            system: "sys".into(),
            messages,
            max_tokens: 100,
            json_schema: None,
            effort: None,
            cacheable: false,
        }
    }

    #[test]
    fn single_message_prompt_is_verbatim_and_transcripts_are_labelled() {
        assert_eq!(render_prompt(&request(vec![Message::user("hi")])), "hi");
        let mut two = request(vec![Message::user("a")]);
        two.messages.push(Message {
            role: ChatRole::Assistant,
            content: "b".into(),
        });
        assert_eq!(render_prompt(&two), "### User\n\na\n\n### Assistant\n\nb");
    }

    #[test]
    fn parses_text_deltas_and_results() {
        let delta = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"po"}}}"#;
        assert!(matches!(parse_line(delta, false), Line::Text(t) if t == "po"));
        assert!(
            matches!(parse_line(delta, true), Line::Ignore),
            "schema output arrives in the result"
        );
        let thinking = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"x"}}}"#;
        assert!(matches!(parse_line(thinking, false), Line::Ignore));

        let result = r#"{"type":"result","subtype":"success","is_error":false,"result":"pong","stop_reason":"end_turn",
            "usage":{"input_tokens":3,"cache_creation_input_tokens":10,"cache_read_input_tokens":5,"output_tokens":4},
            "structured_output":{"colors":["blue"]}}"#;
        let usage = Usage {
            input_tokens: 18,
            output_tokens: 4,
            cached_tokens: 5,
        };
        assert!(matches!(
            parse_line(result, false),
            Line::Finished { text: None, usage: u, stop: StopReason::EndTurn } if u == usage
        ));
        assert!(matches!(
            parse_line(result, true),
            Line::Finished { text: Some(t), usage: u, stop: StopReason::EndTurn }
                if u == usage && t == r#"{"colors":["blue"]}"#
        ));
    }

    #[test]
    fn classifies_errors() {
        let limited = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"API Error: rate limit reached"}"#;
        assert!(matches!(parse_line(limited, false), Line::Failed(e) if e.is_retryable()));
        let auth = r#"{"type":"result","subtype":"success","is_error":true,"result":"Invalid API key · Please run /login"}"#;
        assert!(matches!(parse_line(auth, false), Line::Failed(e) if !e.is_retryable()));
        // Error results carry `subtype` + `errors` and no `result` text.
        let max_turns = r#"{"type":"result","subtype":"error_max_turns","is_error":true,"errors":["Reached maximum number of turns (1)"]}"#;
        match parse_line(max_turns, false) {
            Line::Failed(e) => {
                assert!(e.is_retryable());
                let msg = e.to_string();
                assert!(
                    msg.contains("error_max_turns: Reached maximum number of turns"),
                    "{msg}"
                );
            }
            other => panic!("{other:?}"),
        }
        let no_schema =
            r#"{"type":"result","subtype":"success","is_error":false,"result":"x","usage":{}}"#;
        assert!(matches!(
            parse_line(no_schema, true),
            Line::Failed(LlmError::Protocol(_))
        ));
        assert!(matches!(parse_line("not json", false), Line::Ignore));
    }

    #[tokio::test]
    async fn missing_binary_is_a_clear_error() {
        use futures::StreamExt;
        let provider = ClaudeCodeProvider::new(
            "/nonexistent/claude-binary",
            std::env::temp_dir().join("nexc-claude-code-test"),
        );
        let first = provider
            .stream(request(vec![Message::user("hi")]))
            .next()
            .await;
        let err = first.unwrap().unwrap_err();
        assert!(err.to_string().contains("install Claude Code"), "{err}");
        assert!(!err.is_retryable());
    }
}
