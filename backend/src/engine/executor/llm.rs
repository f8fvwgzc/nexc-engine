//! Built-in executor: one streamed LLM call per node. Documents and outputs
//! (and every node in demo mode) are also saved as Markdown artifacts.

use futures::StreamExt;
use futures::future::BoxFuture;

use super::{ExecContext, ExecError, ExecOutput, NodeExecutor, slug};
use crate::domain::prompt::{NodePrompt, node_prompt};
use crate::domain::settings::LlmProviderKind;
use crate::llm::{LlmEvent, LlmRequest, Message, StopReason};
use crate::realtime::events::LogLevel;

/// Executes nodes with `executor: "llm"`.
pub struct LlmExecutor;

impl NodeExecutor for LlmExecutor {
    fn execute<'a>(&'a self, ctx: &'a ExecContext) -> BoxFuture<'a, Result<ExecOutput, ExecError>> {
        Box::pin(run(ctx))
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
    });
    let request = LlmRequest {
        target: ctx.agent_target(),
        system: ctx.system_prompt(),
        messages: vec![Message::user(prompt)],
        max_tokens: 64_000,
        json_schema: None,
        effort: None,
        cacheable: !ctx.force,
    };
    let mut stream = ctx.state.llm.stream(request);
    let mut out = ExecOutput::default();
    let mut stop = None;
    while let Some(event) = stream.next().await {
        match event? {
            LlmEvent::Text(delta) => {
                ctx.output(&delta);
                out.output.push_str(&delta);
            }
            LlmEvent::Usage(u) => {
                (out.tokens_in, out.tokens_out) = (u.input_tokens as i64, u.output_tokens as i64);
                ctx.tokens(out.tokens_in, out.tokens_out);
            }
            LlmEvent::Done(reason) => {
                stop = Some(reason);
                break;
            }
        }
    }
    match stop {
        None => return Err(ExecError::transient("the LLM stream ended unexpectedly")),
        Some(StopReason::MaxTokens) => ctx.log(LogLevel::Warn, "output truncated at max_tokens"),
        Some(StopReason::EndTurn) => {}
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
