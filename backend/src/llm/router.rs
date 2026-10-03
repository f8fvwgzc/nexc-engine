//! Dispatches requests to the provider named in their target.

use std::time::Duration;

use super::anthropic::AnthropicProvider;
use super::claude_code::ClaudeCodeProvider;
use super::demo::DemoProvider;
use super::openai_compat::OpenAiCompatProvider;
use super::{LlmProvider, LlmRequest, LlmStream};
use crate::domain::settings::LlmProviderKind;

/// The production provider: Anthropic, OpenAI-compatible, demo or Claude Code CLI per request.
pub struct ProviderRouter {
    anthropic: AnthropicProvider,
    openai: OpenAiCompatProvider,
    demo: DemoProvider,
    claude_code: ClaudeCodeProvider,
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
        }
    }
}

impl LlmProvider for ProviderRouter {
    fn stream(&self, request: LlmRequest) -> LlmStream {
        match request.target.provider {
            LlmProviderKind::Anthropic => self.anthropic.stream(request),
            LlmProviderKind::OpenaiCompatible => self.openai.stream(request),
            LlmProviderKind::Demo => self.demo.stream(request),
            LlmProviderKind::ClaudeCode => self.claude_code.stream(request),
        }
    }
}
