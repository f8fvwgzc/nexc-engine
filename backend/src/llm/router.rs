//! Dispatches requests to the provider named in their target, behind the
//! circuit breaker of each network provider's endpoint.

use std::sync::Arc;
use std::time::Duration;

use async_stream::stream;
use futures::StreamExt;

use super::anthropic::AnthropicProvider;
use super::breaker::CircuitBreaker;
use super::claude_code::ClaudeCodeProvider;
use super::demo::DemoProvider;
use super::openai_compat::OpenAiCompatProvider;
use super::{LlmError, LlmProvider, LlmRequest, LlmStream};
use crate::domain::settings::LlmProviderKind;

/// The production provider: Anthropic, OpenAI-compatible, demo or Claude Code CLI per request.
pub struct ProviderRouter {
    anthropic: AnthropicProvider,
    openai: OpenAiCompatProvider,
    demo: DemoProvider,
    claude_code: ClaudeCodeProvider,
    /// The breaker of the network providers; none until one is given.
    breaker: Option<Arc<CircuitBreaker>>,
}

impl ProviderRouter {
    /// Router sharing one HTTP client; `fallbacks` enables Anthropic refusal fallbacks and
    /// `claude_code` is the CLI provider (binary + scratch working directory).
    pub fn new(http: reqwest::Client, fallbacks: bool, claude_code: ClaudeCodeProvider) -> Self {
        ProviderRouter {
            anthropic: AnthropicProvider::new(http.clone(), fallbacks),
            openai: OpenAiCompatProvider::new(http),
            demo: DemoProvider::new(Duration::from_millis(35)),
            claude_code,
            breaker: None,
        }
    }

    /// Puts the network providers behind `breaker` (`NEXC_LLM_BREAKER_*`).
    pub fn with_breaker(mut self, breaker: CircuitBreaker) -> Self {
        self.breaker = Some(Arc::new(breaker));
        self
    }

    /// The breaker, for the operator's status.
    pub fn breaker(&self) -> Option<&CircuitBreaker> {
        self.breaker.as_deref()
    }

    /// The circuit a request's target belongs to: the provider and its endpoint.
    pub fn circuit_key(request: &LlmRequest) -> String {
        format!(
            "{}@{}",
            request.target.provider,
            request.target.base_url.as_deref().unwrap_or("default")
        )
    }

    /// Streams `inner` through the breaker: refused at once while the
    /// circuit is open; otherwise the stream's first event closes the
    /// circuit, and a provider failure counts towards opening it.
    fn guarded(
        &self,
        request: LlmRequest,
        inner: impl FnOnce(LlmRequest) -> LlmStream,
    ) -> LlmStream {
        let Some(breaker) = self.breaker.clone() else {
            return inner(request);
        };
        let key = Self::circuit_key(&request);
        if let Err(refused) = breaker.admit(&key) {
            return Box::pin(futures::stream::iter([Err(LlmError::CircuitOpen {
                provider: key,
                retry_after_secs: refused.retry_after.as_secs().max(1),
            })]));
        }
        let mut events = inner(request);
        Box::pin(stream! {
            let mut decided = false;
            while let Some(event) = events.next().await {
                if !decided {
                    decided = true;
                    match &event {
                        Err(err) if err.is_provider_failure() => breaker.record_failure(&key),
                        // An answer, or a refused or malformed request, which says nothing
                        // about the provider's health.
                        _ => breaker.record_success(&key),
                    }
                }
                yield event;
            }
            if !decided {
                // A stream that ended without a word: the provider did not answer.
                breaker.record_failure(&key);
            }
        })
    }
}

impl LlmProvider for ProviderRouter {
    fn stream(&self, request: LlmRequest) -> LlmStream {
        match request.target.provider {
            LlmProviderKind::Anthropic => {
                let provider = self.anthropic.clone();
                self.guarded(request, move |r| provider.stream(r))
            }
            LlmProviderKind::OpenaiCompatible => {
                let provider = self.openai.clone();
                self.guarded(request, move |r| provider.stream(r))
            }
            LlmProviderKind::Demo => self.demo.stream(request),
            LlmProviderKind::ClaudeCode => self.claude_code.stream(request),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{LlmTarget, Message, collect};
    use crate::observability::metrics::Metrics;

    fn request(base_url: &str) -> LlmRequest {
        LlmRequest {
            target: LlmTarget {
                provider: LlmProviderKind::OpenaiCompatible,
                model: "m".into(),
                base_url: Some(base_url.into()),
                api_key: None,
            },
            system: String::new(),
            messages: vec![Message::user("q")],
            max_tokens: 10,
            json_schema: None,
            effort: None,
            cacheable: false,
        }
    }

    /// A dead provider fails fast once the circuit is open, instead of each
    /// call waiting on its own connection attempt.
    #[tokio::test]
    async fn a_dead_provider_opens_the_circuit_and_fails_fast() {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_millis(200))
            .build()
            .unwrap();
        let claude_code = ClaudeCodeProvider::new("claude", std::env::temp_dir());
        let router = ProviderRouter::new(http, false, claude_code).with_breaker(
            CircuitBreaker::new(2, Duration::from_secs(30), Arc::new(Metrics::default())),
        );
        // Nothing listens on port 9 (discard); the connection is refused at once.
        let dead = "http://127.0.0.1:9/v1";
        for _ in 0..2 {
            let err = collect(router.stream(request(dead)), |_| {})
                .await
                .unwrap_err();
            assert!(matches!(err, LlmError::Network(_)), "{err}");
        }
        let err = collect(router.stream(request(dead)), |_| {})
            .await
            .unwrap_err();
        assert!(matches!(err, LlmError::CircuitOpen { .. }), "{err}");
        assert!(err.is_retryable());
        assert_eq!(router.breaker().unwrap().open_circuits().len(), 1);
        assert_eq!(
            ProviderRouter::circuit_key(&request(dead)),
            "openai_compatible@http://127.0.0.1:9/v1"
        );
        // Another endpoint of the same provider kind has a circuit of its own.
        let other = collect(router.stream(request("http://127.0.0.1:9/other")), |_| {})
            .await
            .unwrap_err();
        assert!(matches!(other, LlmError::Network(_)), "{other}");
    }
}
