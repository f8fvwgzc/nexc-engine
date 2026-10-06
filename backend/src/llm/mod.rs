//! LLM providers behind one streaming trait.
//!
//! * [`anthropic`] – Claude Messages API over raw HTTP (Rust has no official SDK).
//! * [`openai_compat`] – OpenAI-compatible `/chat/completions` (Ollama, vLLM, …).
//! * [`demo`] – deterministic offline provider for trying the product without a key.
//! * [`claude_code`] – the local Claude Code CLI (`claude -p`), using its own login.
//!
//! [`router::ProviderRouter`] dispatches on the request's provider kind and
//! [`service::LlmService`] adds the response cache and metrics. Tests inject
//! their own [`LlmProvider`] so they never touch the network.
#![forbid(unsafe_code)]

pub mod anthropic;
pub mod catalog;
pub mod claude_code;
pub mod demo;
pub mod openai_compat;
pub mod pricing;
pub mod retry;
pub mod router;
pub mod service;
mod sse;

use std::pin::Pin;

use futures::{Stream, StreamExt};
use serde::Serialize;

use crate::config::Secret;
use crate::domain::settings::LlmProviderKind;

/// Which provider, model and credentials a request uses.
#[derive(Debug, Clone)]
pub struct LlmTarget {
    pub provider: LlmProviderKind,
    pub model: String,
    pub base_url: Option<String>,
    pub api_key: Option<Secret<String>>,
}

/// Author of a chat message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    User,
    Assistant,
}

/// One chat message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Message {
    pub role: ChatRole,
    pub content: String,
}

impl Message {
    /// A user message.
    pub fn user(content: impl Into<String>) -> Self {
        Message {
            role: ChatRole::User,
            content: content.into(),
        }
    }
}

/// A JSON schema the response must follow (structured output).
#[derive(Debug, Clone, Serialize)]
pub struct JsonSchema {
    /// Identifier of the schema (`plan_proposal`, `memory_extraction`, …).
    pub name: &'static str,
    pub schema: serde_json::Value,
}

/// A completion request.
#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub target: LlmTarget,
    pub system: String,
    pub messages: Vec<Message>,
    pub max_tokens: u32,
    pub json_schema: Option<JsonSchema>,
    /// Anthropic `output_config.effort` (`low` … `max`); `None` = model default.
    pub effort: Option<&'static str>,
    /// Whether an identical earlier response may be served from the cache.
    pub cacheable: bool,
}

/// Token usage reported by a provider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Why generation stopped (refusals are reported as [`LlmError::Refused`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    MaxTokens,
}

/// One event of a streamed completion.
#[derive(Debug, Clone, PartialEq)]
pub enum LlmEvent {
    /// A chunk of visible text.
    Text(String),
    /// Usage so far (cumulative).
    Usage(Usage),
    /// End of the response.
    Done(StopReason),
}

/// A finished completion.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Completion {
    pub text: String,
    pub usage: Usage,
    /// True when generation hit `max_tokens`.
    pub truncated: bool,
}

/// LLM failures, classified for retrying.
#[derive(Debug, Clone, thiserror::Error)]
pub enum LlmError {
    #[error("no LLM API key configured")]
    MissingKey,
    #[error("the model declined the request (refusal)")]
    Refused,
    #[error("LLM provider returned HTTP {status}: {message}")]
    Http {
        status: u16,
        message: String,
        retryable: bool,
    },
    #[error("LLM connection error: {0}")]
    Network(String),
    #[error("unexpected LLM response: {0}")]
    Protocol(String),
}

impl LlmError {
    /// Whether retrying the same request may succeed.
    pub fn is_retryable(&self) -> bool {
        match self {
            LlmError::Http { retryable, .. } => *retryable,
            LlmError::Network(_) => true,
            LlmError::MissingKey | LlmError::Refused | LlmError::Protocol(_) => false,
        }
    }

    /// Classification of an HTTP status: 408/409/429/5xx (incl. 529) retry, others do not.
    pub fn from_status(status: u16, message: String) -> Self {
        let retryable = matches!(status, 408 | 409 | 429) || status >= 500;
        LlmError::Http {
            status,
            message,
            retryable,
        }
    }
}

impl From<reqwest::Error> for LlmError {
    fn from(err: reqwest::Error) -> Self {
        match err.status() {
            Some(status) => LlmError::from_status(status.as_u16(), "request failed".into()),
            None => LlmError::Network(err.without_url().to_string()),
        }
    }
}

/// A stream of completion events.
pub type LlmStream = Pin<Box<dyn Stream<Item = Result<LlmEvent, LlmError>> + Send>>;

/// A streaming completion backend.
pub trait LlmProvider: Send + Sync {
    /// Starts streaming a completion. Errors are reported in the stream.
    fn stream(&self, request: LlmRequest) -> LlmStream;
}

/// Drains `stream`, calling `on_text` for every text chunk.
pub async fn collect(
    mut stream: LlmStream,
    mut on_text: impl FnMut(&str),
) -> Result<Completion, LlmError> {
    let mut done = Completion::default();
    while let Some(event) = stream.next().await {
        match event? {
            LlmEvent::Text(t) => {
                on_text(&t);
                done.text.push_str(&t);
            }
            LlmEvent::Usage(u) => done.usage = u,
            LlmEvent::Done(reason) => {
                done.truncated = reason == StopReason::MaxTokens;
                return Ok(done);
            }
        }
    }
    Err(LlmError::Protocol(
        "stream ended without a stop event".into(),
    ))
}

/// Estimated tokens of `text` (≈ 4 characters per token) for offline providers.
pub fn estimate_tokens(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_classification() {
        for s in [408, 409, 429, 500, 503, 529] {
            assert!(
                LlmError::from_status(s, String::new()).is_retryable(),
                "{s}"
            );
        }
        for s in [400, 401, 403, 404, 413, 422] {
            assert!(
                !LlmError::from_status(s, String::new()).is_retryable(),
                "{s}"
            );
        }
        assert!(!LlmError::Refused.is_retryable());
        assert!(LlmError::Network("reset".into()).is_retryable());
    }

    #[tokio::test]
    async fn collect_accumulates_and_flags_truncation() {
        let events = vec![
            Ok(LlmEvent::Usage(Usage {
                input_tokens: 10,
                output_tokens: 0,
            })),
            Ok(LlmEvent::Text("Hello ".into())),
            Ok(LlmEvent::Text("world".into())),
            Ok(LlmEvent::Usage(Usage {
                input_tokens: 10,
                output_tokens: 2,
            })),
            Ok(LlmEvent::Done(StopReason::MaxTokens)),
        ];
        let mut seen = 0;
        let c = collect(Box::pin(futures::stream::iter(events)), |_| seen += 1)
            .await
            .unwrap();
        assert_eq!(
            (c.text.as_str(), c.usage.output_tokens, c.truncated, seen),
            ("Hello world", 2, true, 2)
        );
        let err = collect(
            Box::pin(futures::stream::iter(vec![Ok(LlmEvent::Text("x".into()))])),
            |_| {},
        )
        .await;
        assert!(matches!(err, Err(LlmError::Protocol(_))));
        assert_eq!(estimate_tokens("abcdefgh"), 2);
    }
}
