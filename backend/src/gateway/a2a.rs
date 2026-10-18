//! The built-in tool `a2a:<counterpart>`: a local agent sends a task to a
//! remote A2A agent through the gateway. Decided like any egress (the
//! counterpart list and the delegation depth across hops, no loops, the
//! counterpart's own allow-list of agents, the host under the egress rules,
//! the data labels against the counterpart's clearance, the DLP screen of
//! the message), performed with the A2A client under the A2A timeout and the
//! gateway's size cap, recorded as a `tool_call` and an `egress` span that
//! name the remote task; the hops travel in the task's metadata.

use std::time::Instant;

use serde_json::json;

use super::{Call, Egress, Overhead, ToolOutcome, decide_host, screen_arguments, without_query};
use crate::a2a::client::{self, Outbound};
use crate::domain::a2a::TaskState;
use crate::domain::policy::{Action, Decision};
use crate::engine::artifacts;
use crate::engine::executor::ExecContext;
use crate::realtime::events::LogLevel;
use crate::repo;

/// Performs `a2a:<name>` for the node of `ctx`.
pub(super) async fn perform(
    ctx: &ExecContext,
    call: &Call<'_>,
    name: &str,
    overhead: &mut Overhead,
) -> ToolOutcome {
    let req = call.req;
    let Some(workspace) = ctx.workspace_id else {
        let outcome = ToolOutcome::failed("the graph is in no workspace: no A2A counterparts");
        call.record(&outcome, None);
        return outcome;
    };
    let row = match repo::a2a::find_counterpart_by_name(&ctx.state.db, workspace, name).await {
        Ok(Some(row)) => row,
        Ok(None) => {
            let decision = Decision::deny(
                "tools.allow",
                format!("no A2A counterpart named {name} is registered in the workspace"),
            );
            return call.refuse("tool_call", &req.tool, &decision, false).await;
        }
        Err(err) => {
            tracing::error!(error = %err, "A2A counterpart not read");
            let outcome = ToolOutcome::failed("the counterpart's registration could not be read");
            call.record(&outcome, None);
            return outcome;
        }
    };
    let counterpart = &row.counterpart;
    if !counterpart.allows_agent(ctx.agent.as_ref().map(|a| a.id)) {
        let decision = Decision::deny(
            "a2a.allowed_agents",
            format!("{name} does not take tasks from this agent"),
        );
        return call.refuse("tool_call", &req.tool, &decision, false).await;
    }
    // The hops: one more than the inbound task this run answers, if it answers one.
    let inbound = repo::a2a::hops_of_run(&ctx.state.db, ctx.run_id)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    let own = ctx.principal();
    let next = inbound.next(&own);
    if inbound.contains(&counterpart.principal_id) || next.contains(&counterpart.principal_id) {
        let decision = Decision::deny(
            "delegation.loop",
            format!(
                "the task already passed through {}: a loop between agents",
                counterpart.principal_id
            ),
        );
        return call.refuse("counterpart", name, &decision, false).await;
    }
    let decision = ctx.decide(&Action::Counterpart {
        name,
        depth: next.hop,
    });
    if !decision.allowed {
        return call.refuse("counterpart", name, &decision, false).await;
    }
    let host = counterpart.endpoint_host();
    let host_decision = decide_host(ctx, &host);
    if !host_decision.allowed {
        return call.refuse("egress", &host, &host_decision, false).await;
    }
    let label = ctx.strictest_label();
    if label.rank() > counterpart.max_label.rank() {
        let decision = Decision::deny(
            "a2a.max_label",
            format!(
                "the node read {label} data and {name} is cleared for {} at most",
                counterpart.max_label
            ),
        );
        return call.refuse("egress", &host, &decision, false).await;
    }
    let Some(message) = req
        .arguments
        .get("message")
        .and_then(serde_json::Value::as_str)
        .filter(|m| !m.trim().is_empty())
    else {
        let outcome = ToolOutcome::failed(format!("{} needs a `message` argument", req.tool));
        call.record(&outcome, None);
        return outcome;
    };
    let session_id = req
        .arguments
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty());
    let screened = screen_arguments(ctx, message, &host);
    if !screened.allowed {
        return call.refuse("egress", &host, &screened, false).await;
    }
    let settings = &ctx.state.settings;
    let deadline = Instant::now() + settings.a2a_timeout;
    let outbound = Outbound {
        message,
        session_id,
        hops: &next,
        principal: &own,
        max_bytes: settings.gateway_max_response_bytes,
        deadline,
    };
    ctx.log(
        LogLevel::Info,
        format!(
            "{own} sends a task to A2A counterpart {name} (hop {})",
            next.hop
        ),
    );
    let sent = overhead
        .tool(tokio::time::timeout(
            settings.a2a_timeout,
            client::send_task(&ctx.state, &row, &outbound),
        ))
        .await;
    let endpoint = without_query(&counterpart.endpoint());
    let (outcome, extra, egress) = match sent {
        Ok(Ok(remote)) => {
            let mut output = format!(
                "A2A task {} to {name}: {}",
                remote.task_id,
                remote.state.as_str()
            );
            if let Some(message) = &remote.message {
                output.push_str(&format!("\n{message}"));
            }
            for text in &remote.texts {
                output.push_str("\n\n");
                output.push_str(text);
            }
            let mut kept = 0usize;
            for file in &remote.files {
                match &file.bytes {
                    Some(bytes) => {
                        let path = format!(
                            "a2a/{name}/{}",
                            artifacts::sanitize_path(&file.name).unwrap_or_else(|_| "file".into())
                        );
                        match ctx.artifact(&path, Some(&file.mime), bytes).await {
                            Ok(artifact) => {
                                kept += 1;
                                output.push_str(&format!(
                                    "\n\n[file {} ({} bytes) kept as artifact {}]",
                                    file.name,
                                    bytes.len(),
                                    artifact.path
                                ));
                            }
                            Err(err) => output
                                .push_str(&format!("\n\n[file {} not kept: {err}]", file.name)),
                        }
                    }
                    None => output.push_str(&format!(
                        "\n\n[file {} at {}]",
                        file.name,
                        file.uri.as_deref().unwrap_or("no address")
                    )),
                }
            }
            let is_error = remote.state != TaskState::Completed;
            let outcome = ToolOutcome {
                output: super::bounded(output),
                is_error,
                overhead_ns: 0,
            };
            let extra = json!({
                "a2a": true,
                "direction": "outbound",
                "counterpart": counterpart.principal_id,
                "counterpart_name": name,
                "remote_task_id": remote.task_id,
                "remote_state": remote.state.as_str(),
                "hop": next.hop,
                "chain": next.chain,
                "streamed": remote.streamed,
                "files": remote.files.len(),
                "artifacts": kept,
            });
            ctx.state.metrics.a2a_tasks.add(
                &[
                    ("direction", "outbound"),
                    ("outcome", if is_error { "failed" } else { "completed" }),
                ],
                1,
            );
            (
                outcome,
                extra,
                Egress {
                    host: &host,
                    transport: "a2a",
                    bytes_in: remote.bytes_in,
                    status: remote.status,
                    url: Some(endpoint),
                },
            )
        }
        Ok(Err(err)) => {
            ctx.state
                .metrics
                .a2a_tasks
                .add(&[("direction", "outbound"), ("outcome", "failed")], 1);
            (
                ToolOutcome::failed(format!("{name}: {err}")),
                json!({"a2a": true, "direction": "outbound", "counterpart": counterpart.principal_id,
                    "counterpart_name": name, "hop": next.hop, "chain": next.chain}),
                Egress {
                    host: &host,
                    transport: "a2a",
                    bytes_in: 0,
                    status: err.status(),
                    url: Some(endpoint),
                },
            )
        }
        Err(_) => {
            ctx.state
                .metrics
                .a2a_tasks
                .add(&[("direction", "outbound"), ("outcome", "failed")], 1);
            (
                ToolOutcome::failed(format!(
                    "{name} did not finish within {} seconds",
                    settings.a2a_timeout.as_secs()
                )),
                json!({"a2a": true, "direction": "outbound", "counterpart": counterpart.principal_id,
                    "counterpart_name": name, "hop": next.hop, "chain": next.chain}),
                Egress {
                    host: &host,
                    transport: "a2a",
                    bytes_in: 0,
                    status: None,
                    url: Some(endpoint),
                },
            )
        }
    };
    call.record_with(&outcome, Some(egress), extra);
    outcome
}
