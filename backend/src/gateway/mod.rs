//! The tools gateway (AgentSecOps phase 4): the external calls an agent
//! makes go through the backend, never out of the runtime. A workspace
//! registers MCP servers; their tools are offered to agents as
//! `mcp:<server>/<tool>` beside the built-in `fetch_url`; a call comes up the
//! runtime stream as a `tool_request`, the backend decides it under the
//! policy (the tool, the host, the data labels the node was given, the
//! DLP rules, the agent's pace), performs it with a timeout and size
//! limits, records a `tool_call` span and an `egress` span naming the host,
//! and hands the result back down.
#![forbid(unsafe_code)]

pub mod fetch;
pub mod mcp;

use std::time::{Duration, Instant};

use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::alert::AlertKind;
use crate::domain::capabilities::{Capabilities, DataLabel};
use crate::domain::dlp::{self, Destination};
use crate::domain::mcp::{
    FETCH_URL, GatewayTool, McpCheck, McpServer, McpTransport, split_tool_name,
};
use crate::domain::policy::{Action, Decision, Effect, Rules, decide_with};
use crate::domain::policy_document::Severity;
use crate::domain::trace::{SpanDraft, SpanKind, SpanStatus};
use crate::engine::approvals::Granted;
use crate::engine::defend::Finding;
use crate::engine::executor::ExecContext;
use crate::realtime::events::LogLevel;
use crate::repo;
use crate::repo::mcp_servers::McpServerRow;

/// Opening a connection for a gateway call.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// A connection test of a server.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
/// Longest tool output handed to the model, in characters.
const OUTPUT_MAX_CHARS: usize = 200_000;

/// The gateway's own HTTP client: no redirects of its own (every hop's host
/// is decided), short connect timeout, no cookies.
pub fn http_client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("nexc-engine-gateway/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

/// What the runtime asked the backend to do.
#[derive(Debug, Clone)]
pub struct ToolRequest {
    pub request_id: String,
    pub tool: String,
    pub arguments: Value,
    /// The acting principal as the runtime reports it.
    pub agent: String,
    /// The `llm_call` span the call belongs to, as stored.
    pub parent: Option<Uuid>,
    /// The approval that let a held tool go ahead, if any.
    pub approval_id: Option<Uuid>,
}

/// What goes back to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutcome {
    pub output: String,
    pub is_error: bool,
}

impl ToolOutcome {
    fn refused(decision: &Decision) -> Self {
        ToolOutcome {
            output: format!(
                "not allowed by this agent's policy ({}): {}",
                decision.rule, decision.reason
            ),
            is_error: true,
        }
    }

    fn failed(message: impl Into<String>) -> Self {
        ToolOutcome {
            output: message.into(),
            is_error: true,
        }
    }
}

/// The gateway tools an agent with `caps` and `rules` may be offered in
/// `workspace`: `fetch_url` and every allowed tool of every registered
/// server, less what the policy denies outright. Tools a policy holds for
/// an approval are offered; the runtime asks before calling.
pub async fn tools_for(
    state: &AppState,
    workspace: Option<Uuid>,
    caps: &Capabilities,
    rules: &Rules,
    labels: &[DataLabel],
) -> Vec<GatewayTool> {
    let Some(workspace) = workspace else {
        return Vec::new();
    };
    let mut tools = vec![GatewayTool::fetch_url()];
    match repo::mcp_servers::list(&state.db, workspace).await {
        Ok(servers) => {
            for row in servers {
                tools.extend(row.server.gateway_tools());
            }
        }
        Err(err) => tracing::error!(%workspace, error = %err, "MCP servers not read; none offered"),
    }
    tools.retain(|t| {
        let decision = decide_with(caps, rules, labels, &Action::Tool(&t.name));
        decision.allowed || matches!(decision.effect, Effect::RequireApproval { .. })
    });
    tools
}

/// The `Authorization` header of a server, decrypted (bound to the server's id).
fn auth_header_of(state: &AppState, row: &McpServerRow) -> Option<String> {
    let sealed = row.auth_header.as_deref()?;
    match state.secret_box.open(sealed, row.server.id.as_bytes()) {
        Ok(bytes) => String::from_utf8(bytes).ok(),
        Err(err) => {
            tracing::error!(server = %row.server.id, error = %err, "MCP auth header not decrypted");
            None
        }
    }
}

fn scratch_dir(state: &AppState) -> std::path::PathBuf {
    state
        .settings
        .data_dir
        .join("gateway")
        .join(Uuid::now_v7().to_string())
}

/// Opens a session with a registered server.
async fn open_session(
    state: &AppState,
    row: &McpServerRow,
    max_bytes: usize,
) -> Result<mcp::Session, mcp::McpError> {
    let server = &row.server;
    match server.transport {
        McpTransport::Stdio => {
            let scratch = scratch_dir(state);
            mcp::Session::open(
                mcp::Target::Stdio {
                    command: server.command.as_deref().unwrap_or_default(),
                    args: &server.args,
                    scratch: &scratch,
                },
                max_bytes,
            )
            .await
        }
        McpTransport::Http => {
            let auth = auth_header_of(state, row);
            mcp::Session::open(
                mcp::Target::Http {
                    client: &state.gateway_http,
                    url: server.url.as_deref().unwrap_or_default(),
                    auth_header: auth.as_deref(),
                },
                max_bytes,
            )
            .await
        }
    }
}

/// Connects to a server, lists its tools and records what it found.
pub async fn check_server(state: &AppState, row: &McpServerRow) -> McpCheck {
    let started = Instant::now();
    let max_bytes = state.settings.gateway_max_response_bytes;
    let attempt = async {
        let mut session = open_session(state, row, max_bytes).await?;
        let tools = session.list_tools().await;
        let info = session.server_info.clone();
        session.close().await;
        tools.map(|t| (t, info))
    };
    let found = match tokio::time::timeout(CHECK_TIMEOUT, attempt).await {
        Ok(Ok((tools, info))) => McpCheck {
            ok: true,
            detail: format!(
                "{}: {} tool(s) listed",
                if info.is_empty() { "server" } else { &info },
                tools.len()
            ),
            tools,
            latency_ms: started.elapsed().as_millis() as u64,
        },
        Ok(Err(err)) => McpCheck {
            ok: false,
            detail: err.to_string(),
            tools: Vec::new(),
            latency_ms: started.elapsed().as_millis() as u64,
        },
        Err(_) => McpCheck {
            ok: false,
            detail: format!("no answer within {} seconds", CHECK_TIMEOUT.as_secs()),
            tools: Vec::new(),
            latency_ms: started.elapsed().as_millis() as u64,
        },
    };
    if let Err(err) = repo::mcp_servers::set_check(
        &state.db,
        row.server.id,
        found.ok,
        &found.detail,
        &found.tools,
    )
    .await
    {
        tracing::error!(server = %row.server.id, error = %err, "MCP check not recorded");
    }
    found
}

/// Whether `reported` is `principal` itself or a sub-agent of it.
fn is_principal_or_descendant(reported: &str, principal: &str) -> bool {
    reported == principal
        || reported
            .strip_prefix(principal)
            .is_some_and(|rest| rest.starts_with('/') && rest.len() > 1)
}

/// The arguments as canonical JSON (sorted keys, no whitespace) and its digest.
fn args_digest(arguments: &Value) -> (String, usize, String) {
    let text = serde_json::to_string(arguments).unwrap_or_default();
    (crate::kernel::sha256_hex(text.as_bytes()), text.len(), text)
}

/// What one call touched, for its spans.
struct Call<'a> {
    ctx: &'a ExecContext,
    req: &'a ToolRequest,
    principal: String,
    started: Instant,
    started_at: chrono::DateTime<Utc>,
    args_sha256: String,
    args_bytes: usize,
    server: Option<String>,
}

impl Call<'_> {
    /// Records a refusal as a `policy` span; `mismatch` also puts it on the
    /// record as a disagreement with the runtime's own decision.
    async fn refuse(
        &self,
        action: &str,
        subject: &str,
        decision: &Decision,
        mismatch: bool,
    ) -> ToolOutcome {
        let span_id = self.ctx.policy_span(
            subject,
            decision,
            json!({
                "action": action,
                "subject": subject,
                "tool": self.req.tool,
                "server": self.server,
                "request_id": self.req.request_id,
                "gateway": true,
                "args_sha256": self.args_sha256,
            }),
        );
        self.ctx.log(
            LogLevel::Warn,
            format!(
                "gateway refused {} for {} ({}): {}",
                self.req.tool, self.principal, decision.rule, decision.reason
            ),
        );
        if mismatch {
            self.ctx
                .policy_mismatch(
                    span_id,
                    decision.rule,
                    format!(
                        "the runtime asked the gateway to call {}; {}",
                        self.req.tool, decision.reason
                    ),
                )
                .await;
        }
        if decision.rule.starts_with("dlp.") || decision.rule.starts_with("egress.") {
            self.ctx
                .alert(&Finding {
                    span_id: Some(span_id),
                    kind: AlertKind::Dlp,
                    severity: Severity::High,
                    rule: decision.rule,
                    title: &format!("{} refused for {subject}", self.req.tool),
                    detail: &decision.reason,
                })
                .await;
        }
        ToolOutcome::refused(decision)
    }

    /// Records the call as a `tool_call` span and, when it went out, an
    /// `egress` span naming the host.
    fn record(&self, outcome: &ToolOutcome, egress: Option<Egress<'_>>) {
        let ctx = self.ctx;
        let now = Utc::now();
        let duration_ns = self.started.elapsed().as_nanos() as u64;
        let mut tool = ctx.draft(SpanKind::ToolCall, self.req.tool.clone(), self.started_at);
        tool.principal = self.principal.clone();
        tool.parent_id = self.req.parent;
        tool.ended_at = Some(now);
        tool.status = if outcome.is_error {
            SpanStatus::Error
        } else {
            SpanStatus::Ok
        };
        tool.attributes = json!({
            "tool": self.req.tool,
            "args_sha256": self.args_sha256,
            "args_bytes": self.args_bytes,
            "result_bytes": outcome.output.len(),
            "is_error": outcome.is_error,
            "duration_ns": duration_ns,
            "gateway": true,
            "server": self.server,
            "request_id": self.req.request_id,
            "approval_id": self.req.approval_id,
            "policy_version": ctx.policy_version,
            "egress_host": egress.as_ref().map(|e| e.host),
            "payload_sha256": self.args_sha256,
            "payload_bytes": self.args_bytes,
        });
        let tool_span_id = tool.id;
        ctx.record(tool);
        if let Some(egress) = egress {
            let mut span = ctx.draft(SpanKind::Egress, egress.host.to_owned(), self.started_at);
            span.principal = self.principal.clone();
            span.parent_id = Some(tool_span_id);
            span.ended_at = Some(now);
            span.status = if outcome.is_error {
                SpanStatus::Error
            } else {
                SpanStatus::Ok
            };
            span.attributes = json!({
                "host": egress.host,
                "transport": egress.transport,
                "tool": self.req.tool,
                "server": self.server,
                "bytes_out": self.args_bytes,
                "bytes_in": egress.bytes_in,
                "status": egress.status,
                "url": egress.url,
                "duration_ns": duration_ns,
                "label": ctx.strictest_label(),
                "policy_versions": ctx.rules.versions,
            });
            ctx.record(span);
        }
    }
}

/// What left the platform during a call.
struct Egress<'a> {
    host: &'a str,
    transport: &'a str,
    bytes_in: usize,
    /// An HTTP status, when there was one.
    status: Option<u16>,
    /// The URL fetched, without its query (which may carry data).
    url: Option<String>,
}

/// Decides the host of an external call: the egress rules, then the DLP
/// rules for the labels the node was given, then the private-network rule.
fn decide_host(ctx: &ExecContext, host: &str) -> Decision {
    let hosts = ctx.decide(&Action::Egress(host));
    if !hosts.allowed {
        return hosts;
    }
    let label = ctx.strictest_label();
    let labels = dlp::decide(
        &ctx.rules.dlp,
        label,
        &dlp::Findings::default(),
        Destination::Egress(host),
    );
    if !labels.allowed {
        return labels;
    }
    let named = ctx
        .capabilities
        .egress_hosts
        .as_ref()
        .is_some_and(|hosts| hosts.iter().any(|h| h.eq_ignore_ascii_case(host)));
    if fetch::is_private_host(host) && !named {
        return Decision::deny(
            "egress.private",
            format!(
                "{host} is on this machine or a private network; only a host the allow-list names is reached there"
            ),
        );
    }
    hosts
}

/// The DLP screen over what is about to be sent: credentials, personal data
/// and the workspace's keywords in the arguments.
fn screen_arguments(ctx: &ExecContext, text: &str, host: &str) -> Decision {
    let rules = &ctx.rules.dlp;
    let findings = dlp::classify(text, &rules.pii.kinds, &rules.pii.keywords);
    dlp::decide(
        rules,
        ctx.strictest_label(),
        &findings,
        Destination::Egress(host),
    )
}

/// Performs a gateway tool call for the node of `ctx`: decides it, does it,
/// records it. Never fails the node; the model sees an error instead.
pub async fn perform(ctx: &ExecContext, req: &ToolRequest, granted: &Granted) -> ToolOutcome {
    let (args_sha256, args_bytes, args_text) = args_digest(&req.arguments);
    let own = ctx.principal();
    let principal = if is_principal_or_descendant(&req.agent, &own) {
        req.agent.clone()
    } else {
        own.clone()
    };
    let call = Call {
        ctx,
        req,
        principal: principal.clone(),
        started: Instant::now(),
        started_at: Utc::now(),
        args_sha256,
        args_bytes,
        server: split_tool_name(&req.tool).map(|(server, _)| server.to_owned()),
    };
    // The agent's pace, with the process-wide bucket.
    if let Err(err) = ctx.state.limiters.check_gateway(&own) {
        let wait = match err {
            crate::domain::AppError::RateLimited { retry_after_secs } => retry_after_secs,
            _ => 0,
        };
        let decision = Decision::deny(
            "limits.gateway_calls_per_minute",
            format!(
                "the agent made its {} gateway calls for this minute; next in {wait}s",
                ctx.state.settings.gateway_calls_per_minute
            ),
        );
        return call.refuse("tool_call", &req.tool, &decision, false).await;
    }
    // The tool itself, as the runtime should have decided it already.
    let decision = ctx.decide(&Action::Tool(&req.tool));
    match &decision.effect {
        Effect::Allow | Effect::Alert { .. } => {}
        Effect::RequireApproval { .. } => {
            let approved = req
                .approval_id
                .is_some_and(|id| granted.lock().expect("granted set").contains(&id));
            if !approved {
                let denied = Decision::deny(
                    decision.rule,
                    format!("{} without an approval; {}", req.tool, decision.reason),
                );
                return call.refuse("tool_call", &req.tool, &denied, true).await;
            }
        }
        Effect::Deny => return call.refuse("tool_call", &req.tool, &decision, true).await,
    }
    let settings = &ctx.state.settings;
    let max_bytes = settings.gateway_max_response_bytes;
    let timeout = settings.gateway_timeout;

    if req.tool == FETCH_URL {
        let Some(url) = req.arguments.get("url").and_then(Value::as_str) else {
            let outcome = ToolOutcome::failed("fetch_url needs a `url` argument");
            call.record(&outcome, None);
            return outcome;
        };
        let (_, host) = match fetch::host_of(url) {
            Ok(parsed) => parsed,
            Err(err) => {
                let outcome = ToolOutcome::failed(err.to_string());
                call.record(&outcome, None);
                return outcome;
            }
        };
        let host_decision = decide_host(ctx, &host);
        if !host_decision.allowed {
            return call.refuse("egress", &host, &host_decision, false).await;
        }
        // The URL itself may carry data (its query): screened like any payload.
        let screened = screen_arguments(ctx, url, &host);
        if !screened.allowed {
            return call.refuse("egress", &host, &screened, false).await;
        }
        let allow = |candidate: &str| -> Result<(), (&'static str, String)> {
            let decision = decide_host(ctx, candidate);
            if decision.allowed {
                Ok(())
            } else {
                Err((decision.rule, decision.reason))
            }
        };
        let fetched = tokio::time::timeout(
            timeout,
            fetch::fetch(&ctx.state.gateway_http, url, max_bytes, &allow),
        )
        .await;
        let (outcome, egress) = match fetched {
            Ok(Ok(fetched)) => {
                let outcome = ToolOutcome {
                    output: bounded(format!(
                        "HTTP {} {}\n\n{}",
                        fetched.status, fetched.content_type, fetched.body
                    )),
                    is_error: fetched.status >= 400,
                };
                let final_host = fetched.host.clone();
                let final_url = without_query(&fetched.final_url);
                call.record(
                    &outcome,
                    Some(Egress {
                        host: &final_host,
                        transport: "https",
                        bytes_in: fetched.bytes,
                        status: Some(fetched.status),
                        url: Some(final_url),
                    }),
                );
                return outcome;
            }
            Ok(Err(fetch::FetchError::Refused { rule, reason })) => {
                let decision = Decision::deny(rule, reason);
                return call.refuse("egress", &host, &decision, false).await;
            }
            Ok(Err(err)) => (ToolOutcome::failed(err.to_string()), true),
            Err(_) => (
                ToolOutcome::failed(format!("no answer within {} seconds", timeout.as_secs())),
                true,
            ),
        };
        let url_shown = without_query(url);
        call.record(
            &outcome,
            egress.then_some(Egress {
                host: &host,
                transport: "https",
                bytes_in: 0,
                status: None,
                url: Some(url_shown),
            }),
        );
        return outcome;
    }

    let Some((server_name, tool_name)) = split_tool_name(&req.tool) else {
        let decision = Decision::deny("tools.allow", format!("{} is not a gateway tool", req.tool));
        return call.refuse("tool_call", &req.tool, &decision, true).await;
    };
    let Some(workspace) = ctx.workspace_id else {
        let outcome = ToolOutcome::failed("the graph is in no workspace: no MCP servers");
        call.record(&outcome, None);
        return outcome;
    };
    let row = match repo::mcp_servers::find_by_name(&ctx.state.db, workspace, server_name).await {
        Ok(Some(row)) => row,
        Ok(None) => {
            let decision = Decision::deny(
                "tools.allow",
                format!("no MCP server named {server_name} is registered in the workspace"),
            );
            return call.refuse("tool_call", &req.tool, &decision, false).await;
        }
        Err(err) => {
            tracing::error!(error = %err, "MCP server not read");
            let outcome = ToolOutcome::failed("the server's registration could not be read");
            call.record(&outcome, None);
            return outcome;
        }
    };
    let server: &McpServer = &row.server;
    if !server.allows_tool(tool_name) {
        let decision = Decision::deny(
            "mcp.allowed_tools",
            format!("{tool_name} is not among the tools of {server_name} agents may call"),
        );
        return call.refuse("tool_call", &req.tool, &decision, false).await;
    }
    let host = server.host();
    let label = ctx.strictest_label();
    if label.rank() > server.max_label.rank() {
        let decision = Decision::deny(
            "mcp.max_label",
            format!(
                "the node read {label} data and {server_name} is cleared for {} at most",
                server.max_label
            ),
        );
        return call.refuse("egress", &host, &decision, false).await;
    }
    if server.transport == McpTransport::Http {
        let host_decision = decide_host(ctx, &host);
        if !host_decision.allowed && host_decision.rule != "egress.private" {
            return call.refuse("egress", &host, &host_decision, false).await;
        }
    } else {
        let labels = dlp::decide(
            &ctx.rules.dlp,
            label,
            &dlp::Findings::default(),
            Destination::Egress(&host),
        );
        if !labels.allowed {
            return call.refuse("egress", &host, &labels, false).await;
        }
    }
    let screened = screen_arguments(ctx, &args_text, &host);
    if !screened.allowed {
        return call.refuse("egress", &host, &screened, false).await;
    }
    let transport = server.transport.as_str();
    let work = async {
        let mut session = open_session(&ctx.state, &row, max_bytes).await?;
        let result = session.call_tool(tool_name, &req.arguments).await;
        session.close().await;
        result
    };
    let outcome = match tokio::time::timeout(timeout, work).await {
        Ok(Ok(output)) => {
            let outcome = ToolOutcome {
                output: bounded(output.text),
                is_error: output.is_error,
            };
            call.record(
                &outcome,
                Some(Egress {
                    host: &host,
                    transport,
                    bytes_in: output.bytes,
                    status: None,
                    url: server.url.as_deref().map(without_query),
                }),
            );
            return outcome;
        }
        Ok(Err(err)) => ToolOutcome::failed(format!("{server_name}: {err}")),
        Err(_) => ToolOutcome::failed(format!(
            "{server_name} did not answer within {} seconds",
            timeout.as_secs()
        )),
    };
    call.record(
        &outcome,
        Some(Egress {
            host: &host,
            transport,
            bytes_in: 0,
            status: None,
            url: server.url.as_deref().map(without_query),
        }),
    );
    outcome
}

fn bounded(text: String) -> String {
    if text.chars().count() <= OUTPUT_MAX_CHARS {
        return text;
    }
    let mut out: String = text.chars().take(OUTPUT_MAX_CHARS).collect();
    out.push_str("\n\n[truncated by the gateway]");
    out
}

/// A URL without its query and fragment (which may carry data).
fn without_query(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(mut parsed) => {
            parsed.set_query(None);
            parsed.set_fragment(None);
            parsed.to_string()
        }
        Err(_) => url.split(['?', '#']).next().unwrap_or_default().to_owned(),
    }
}

/// The `policy` span a gateway refusal leaves is of this run's node: the
/// draft helpers of [`ExecContext`] give it the right ids.
#[allow(dead_code)]
fn _spans_are_drafted_by_the_context(_: &SpanDraft) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn principals_digests_and_urls() {
        assert!(is_principal_or_descendant("agent://w/A", "agent://w/A"));
        assert!(is_principal_or_descendant("agent://w/A/r-1", "agent://w/A"));
        assert!(!is_principal_or_descendant("agent://w/AB", "agent://w/A"));
        let (digest, bytes, text) = args_digest(&json!({"b": 1, "a": "x"}));
        assert_eq!(text, "{\"a\":\"x\",\"b\":1}");
        assert_eq!(bytes, text.len());
        assert_eq!(digest.len(), 64);
        assert_eq!(
            without_query("https://api.example.com/v1/search?q=secret#frag"),
            "https://api.example.com/v1/search"
        );
        assert!(bounded("x".repeat(OUTPUT_MAX_CHARS + 5)).ends_with("[truncated by the gateway]"));
    }
}
