//! Per-user LLM settings.

use serde::Serialize;
use utoipa::ToSchema;

use super::string_enum;

string_enum!(
    /// LLM backend family. `demo` is a deterministic offline provider; `claude_code` runs the
    /// local Claude Code CLI with the operator's own login (no API key).
    LlmProviderKind {
        Anthropic => "anthropic",
        OpenaiCompatible => "openai_compatible",
        Demo => "demo",
        ClaudeCode => "claude_code",
    }
);

impl LlmProviderKind {
    /// Whether requests to this provider need an API key.
    pub fn requires_api_key(self) -> bool {
        matches!(self, LlmProviderKind::Anthropic)
    }
}

string_enum!(
    /// Where the effective API key comes from.
    KeySource {
        User => "user",
        Server => "server",
        None => "none",
    }
);

/// LLM configuration as seen by the user (the key itself is never returned).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LlmSettings {
    pub provider: LlmProviderKind,
    pub model: String,
    #[schema(required = true)]
    pub base_url: Option<String>,
    pub has_api_key: bool,
    /// Last four characters of the key, prefixed with `…`.
    #[schema(required = true)]
    pub key_hint: Option<String>,
    pub source: KeySource,
}

/// `…` followed by the last four characters of `key`.
pub fn key_hint(key: &str) -> String {
    let tail: String = key
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_shows_last_four() {
        assert_eq!(key_hint("sk-ant-123456a1b2"), "…a1b2");
        assert_eq!(key_hint("ab"), "…ab");
    }
}
