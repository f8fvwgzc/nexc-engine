//! Keeps what a span said when the workspace records it: the guardrails'
//! secret scrubber and the label redaction run over the content, it is
//! sealed with the master key bound to the span id, and it is written
//! beside the span. Recording never fails the work being recorded.

use crate::domain::guardrails::redact_secrets;
use crate::domain::recording::{self, PayloadKind};
use crate::engine::executor::ExecContext;
use crate::repo;
use crate::repo::span_payloads::NewPayload;
use uuid::Uuid;

/// What to keep of a span.
#[derive(Debug, Clone)]
pub struct Capture {
    /// The span as stored.
    pub span_id: Uuid,
    pub kind: PayloadKind,
    pub content: String,
}

/// The content as kept: credentials scrubbed when the guardrails say so,
/// the passages and memories of a redacted label replaced by their
/// citation, cut at the limit. Returns it with how many pieces were replaced.
pub fn prepare(ctx: &ExecContext, content: String) -> (String, usize) {
    let (content, scrubbed) = if ctx.scrub_secrets {
        redact_secrets(&content)
    } else {
        (content, 0)
    };
    let (content, redacted) = recording::redact(&content, &ctx.redactions);
    (recording::bounded(content), scrubbed + redacted)
}

/// Keeps `capture` when the node's workspace records payloads of its kind.
pub async fn store(ctx: &ExecContext, capture: Capture) {
    if !ctx.recording.wants(capture.kind) {
        return;
    }
    let (content, redactions) = prepare(ctx, capture.content);
    let sealed = match ctx
        .state
        .secret_box
        .seal(content.as_bytes(), capture.span_id.as_bytes())
    {
        Ok(sealed) => sealed,
        Err(err) => {
            tracing::error!(span = %capture.span_id, error = %err, "payload not sealed; dropped");
            return;
        }
    };
    let new = NewPayload {
        span_id: capture.span_id,
        run_id: ctx.run_id,
        kind: capture.kind,
        sealed: &sealed,
        size: content.len().min(i32::MAX as usize) as i32,
        redactions: redactions.min(i32::MAX as usize) as i32,
    };
    if let Err(err) = repo::span_payloads::insert(&ctx.state.db, &new).await {
        tracing::error!(span = %capture.span_id, error = %err, "payload not written");
    }
}

/// Keeps the input and the output of a tool call the backend performed.
pub async fn store_tool_io(ctx: &ExecContext, span_id: Uuid, input: &str, output: &str) {
    if !ctx.recording.tool_io {
        return;
    }
    store(
        ctx,
        Capture {
            span_id,
            kind: PayloadKind::ToolInput,
            content: input.to_owned(),
        },
    )
    .await;
    store(
        ctx,
        Capture {
            span_id,
            kind: PayloadKind::ToolOutput,
            content: output.to_owned(),
        },
    )
    .await;
}
