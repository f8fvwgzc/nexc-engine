//! Built-in executor: one streamed LLM call per node. Documents and outputs
//! (and every node in demo mode) are also saved as Markdown artifacts.

use std::time::Instant;

use chrono::Utc;
use futures::StreamExt;
use futures::future::BoxFuture;
use serde_json::json;
use uuid::Uuid;

use super::{ExecContext, ExecError, ExecOutput, NodeExecutor, slug, with_evidence};
use crate::domain::evidence;
use crate::domain::prompt::{NodePrompt, node_prompt};
use crate::domain::settings::LlmProviderKind;
use crate::domain::trace::{self, SpanKind, SpanStatus};
use crate::llm::{LlmEvent, LlmRequest, Message, StopReason};
use crate::realtime::events::LogLevel;

/// Executes nodes with `executor: "llm"`.
pub struct LlmExecutor;

impl NodeExecutor for LlmExecutor {
    fn execute<'a>(&'a self, ctx: &'a ExecContext) -> BoxFuture<'a, Result<ExecOutput, ExecError>> {
        Box::pin(run(ctx))
    }
}

fn stop_name(stop: StopReason) -> &'static str {
    match stop {
        StopReason::EndTurn => "end_turn",
        StopReason::MaxTokens => "max_tokens",
    }
}

async fn run(ctx: &ExecContext) -> Result<ExecOutput, ExecError> {
    let prompt = node_prompt(NodePrompt {
        goal: &ctx.goal,
        title: &ctx.node.title,
        kind: &ctx.node.kind,
        node_type: ctx.node_type.as_ref(),
        content: &ctx.node.content,
        upstream: &ctx.upstream,
        memories: &ctx.memories,
        documents: &ctx.documents,
    });
    let target = ctx.agent_target();
    let request = LlmRequest {
        target: target.clone(),
        system: ctx.system_prompt(),
        messages: vec![Message::user(prompt)],
        max_tokens: 64_000,
        json_schema: None,
        effort: None,
        cacheable: !ctx.force,
    };
    // The call is one `llm_call` span, whatever way it ends.
    let started_at = Utc::now();
    let clock = Instant::now();
    let mut out = ExecOutput {
        span_id: Some(Uuid::now_v7()),
        ..ExecOutput::default()
    };
    let streamed = consume(ctx, request, &mut out).await;
    let mut span = ctx.draft(SpanKind::LlmCall, target.model.clone(), started_at);
    span.id = out.span_id.unwrap_or(span.id);
    span.ended_at = Some(Utc::now());
    span.attributes = json!({
        "provider": target.provider.as_str(),
        "model": target.model,
        "tokens_in": out.tokens_in,
        "tokens_out": out.tokens_out,
        "tokens_cached": out.tokens_cached,
        "duration_ns": clock.elapsed().as_nanos() as i64,
    });
    match &streamed {
        Ok(stop) => {
            span.attributes["stop_reason"] = json!(stop_name(*stop));
            // The verdict on the output rides on the call that produced it.
            let evidence = evidence::assess(
                &[out.output.as_str()],
                &ctx.given,
                ctx.rules.evidence.min_coverage,
            );
            with_evidence(&mut span.attributes, &evidence);
            out.evidence = Some(evidence);
        }
        Err(err) => {
            span.status = SpanStatus::Error;
            span.attributes["error"] = json!(trace::clean_name(&err.message));
        }
    }
    ctx.record(span);
    match streamed? {
        StopReason::MaxTokens => ctx.log(LogLevel::Warn, "output truncated at max_tokens"),
        StopReason::EndTurn => {}
    }
    let wants_artifact = ctx.node_type.as_ref().is_some_and(|t| t.produces_artifact)
        || ctx.target.provider == LlmProviderKind::Demo;
    if wants_artifact && !out.output.trim().is_empty() {
        let path = format!("{}.md", slug(&ctx.node.title));
        if let Err(err) = ctx
            .artifact(&path, Some("text/markdown"), out.output.as_bytes())
            .await
        {
            ctx.log(LogLevel::Warn, format!("could not save artifact: {err}"));
        }
    }
    Ok(out)
}

/// Streams the completion into `out`; the stop reason once the model is done.
async fn consume(
    ctx: &ExecContext,
    request: LlmRequest,
    out: &mut ExecOutput,
) -> Result<StopReason, ExecError> {
    let mut stream = ctx.state.llm.stream(request);
    while let Some(event) = stream.next().await {
        match event? {
            LlmEvent::Text(delta) => {
                ctx.output(&delta);
                out.output.push_str(&delta);
            }
            LlmEvent::Usage(u) => {
                (out.tokens_in, out.tokens_out) = (u.input_tokens as i64, u.output_tokens as i64);
                out.tokens_cached = u.cached_tokens as i64;
                ctx.tokens(out.tokens_in, out.tokens_out);
            }
            LlmEvent::Done(reason) => return Ok(reason),
        }
    }
    Err(ExecError::transient("the LLM stream ended unexpectedly"))
}
