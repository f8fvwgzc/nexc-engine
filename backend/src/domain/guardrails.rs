//! Guardrails: the policy a workspace puts on its use of LLMs. Everything
//! here is a pure decision; the engine asks before it spends.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::settings::LlmProviderKind;

/// What stands in for a secret that was removed from a prompt.
pub const REDACTED: &str = "[redacted secret]";

/// A workspace's policy. The defaults allow everything and scrub secrets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Guardrails {
    /// Tokens (in + out) the workspace may spend per calendar month (UTC);
    /// `null` for no limit. New plans, runs and assistant calls are refused
    /// once it is reached; work already running finishes.
    #[serde(default)]
    #[schema(required = true)]
    pub monthly_token_budget: Option<i64>,
    /// The same limit for each member.
    #[serde(default)]
    #[schema(required = true)]
    pub member_monthly_token_budget: Option<i64>,
    /// Providers work may run on; empty allows all of them.
    #[serde(default)]
    #[schema(required = true)]
    pub allowed_providers: Vec<LlmProviderKind>,
    /// Whether agents may execute code where a node's type asks for it.
    #[serde(default = "yes")]
    #[schema(required = true)]
    pub allow_code_exec: bool,
    /// Whether API keys, tokens and private keys are removed from the context
    /// (upstream outputs, memories) a node sends to a model.
    #[serde(default = "yes")]
    #[schema(required = true)]
    pub redact_secrets: bool,
}

fn yes() -> bool {
    true
}

impl Default for Guardrails {
    fn default() -> Self {
        Guardrails {
            monthly_token_budget: None,
            member_monthly_token_budget: None,
            allowed_providers: Vec::new(),
            allow_code_exec: true,
            redact_secrets: true,
        }
    }
}

impl Guardrails {
    /// Why work may not run on `provider`, if the policy forbids it.
    pub fn provider_refusal(&self, provider: LlmProviderKind) -> Option<String> {
        if self.allowed_providers.is_empty() || self.allowed_providers.contains(&provider) {
            return None;
        }
        let allowed: Vec<&str> = self.allowed_providers.iter().map(|p| p.as_str()).collect();
        Some(format!(
            "this workspace only allows the providers {} (yours is {provider})",
            allowed.join(", ")
        ))
    }

    /// `text` as it may be sent to a model under this policy.
    pub fn scrub(&self, text: String) -> String {
        if self.redact_secrets {
            redact_secrets(&text).0
        } else {
            text
        }
    }

    /// Why no more tokens may be spent this month, given what the workspace
    /// and the member have used so far.
    pub fn budget_refusal(&self, workspace_used: i64, member_used: i64) -> Option<String> {
        if let Some(budget) = self.monthly_token_budget
            && workspace_used >= budget
        {
            return Some(format!(
                "the workspace has used its monthly budget of {budget} tokens"
            ));
        }
        if let Some(budget) = self.member_monthly_token_budget
            && member_used >= budget
        {
            return Some(format!(
                "you have used your monthly budget of {budget} tokens in this workspace"
            ));
        }
        None
    }
}

/// Shapes of credentials worth never sending to a model: `(prefix, minimum
/// length of the token including the prefix)`.
const SECRET_PREFIXES: [(&str, usize); 10] = [
    ("sk-", 24),
    ("ghp_", 24),
    ("gho_", 24),
    ("github_pat_", 30),
    ("glpat-", 20),
    ("xoxb-", 20),
    ("xoxp-", 20),
    ("AKIA", 20),
    ("AIza", 30),
    ("eyJhbGciOi", 40),
];

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '+' | '=')
}

fn looks_secret(token: &str) -> bool {
    SECRET_PREFIXES.iter().any(|(prefix, min)| {
        token.starts_with(prefix)
            && token.len() >= *min
            && token[prefix.len()..].chars().any(|c| c.is_ascii_digit())
    })
}

/// Replaces credentials in `text` with [`REDACTED`]: tokens of well-known
/// shapes (API keys, access tokens, JWTs) and PEM private key blocks.
/// Returns the text and how many were replaced.
pub fn redact_secrets(text: &str) -> (String, usize) {
    const PEM_BEGIN: &str = "-----BEGIN ";
    const PEM_KEY: &str = "PRIVATE KEY-----";
    let mut out = String::with_capacity(text.len());
    let mut count = 0;
    let mut rest = text;
    while !rest.is_empty() {
        // A private key block goes as a whole, up to its END line.
        if rest.starts_with(PEM_BEGIN)
            && let Some(header_end) = rest.find(PEM_KEY)
            && header_end < 40
        {
            let after_header = header_end + PEM_KEY.len();
            let end = rest[after_header..]
                .find(PEM_KEY)
                .map_or(rest.len(), |i| after_header + i + PEM_KEY.len());
            out.push_str(REDACTED);
            count += 1;
            rest = &rest[end..];
            continue;
        }
        let token_len: usize = rest
            .chars()
            .take_while(|c| is_token_char(*c))
            .map(char::len_utf8)
            .sum();
        if token_len == 0 {
            let c = rest.chars().next().expect("rest is not empty");
            out.push(c);
            rest = &rest[c.len_utf8()..];
            continue;
        }
        let token = &rest[..token_len];
        // A key may follow `NAME=` or `Bearer` inside the same run of token characters.
        let start = SECRET_PREFIXES
            .iter()
            .filter_map(|(prefix, _)| token.find(prefix))
            .min();
        match start {
            Some(at) if looks_secret(&token[at..]) => {
                out.push_str(&token[..at]);
                out.push_str(REDACTED);
                count += 1;
            }
            _ => out.push_str(token),
        }
        rest = &rest[token_len..];
    }
    (out, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_allow_everything_and_scrub_secrets() {
        let g: Guardrails = serde_json::from_str("{}").unwrap();
        assert_eq!(g, Guardrails::default());
        assert!(g.allow_code_exec && g.redact_secrets);
        assert_eq!(g.provider_refusal(LlmProviderKind::ClaudeCode), None);
        assert_eq!(g.budget_refusal(i64::MAX, i64::MAX), None);
        assert!(serde_json::from_str::<Guardrails>(r#"{"typo": 1}"#).is_err());
    }

    #[test]
    fn budgets_and_providers_refuse_with_a_reason() {
        let g = Guardrails {
            monthly_token_budget: Some(1_000),
            member_monthly_token_budget: Some(100),
            allowed_providers: vec![LlmProviderKind::Anthropic],
            ..Guardrails::default()
        };
        assert_eq!(g.budget_refusal(999, 99), None);
        assert!(g.budget_refusal(1_000, 0).unwrap().contains("workspace"));
        assert!(
            g.budget_refusal(500, 100)
                .unwrap()
                .contains("your monthly budget")
        );
        assert_eq!(g.provider_refusal(LlmProviderKind::Anthropic), None);
        let refusal = g.provider_refusal(LlmProviderKind::ClaudeCode).unwrap();
        assert!(refusal.contains("anthropic") && refusal.contains("claude_code"));
    }

    #[test]
    fn scrubs_keys_tokens_and_private_keys() {
        let text = "Use ANTHROPIC_API_KEY=sk-ant-api03-AbCdEf0123456789xyz_-Q and \
            Authorization: Bearer ghp_0123456789abcdefghijABCDEFGHIJ0123 then\n\
            -----BEGIN RSA PRIVATE KEY-----\nMIIEow...\n-----END RSA PRIVATE KEY-----\ndone. \
            AWS AKIAIOSFODNN7EXAMPLE1 ok";
        let (clean, count) = redact_secrets(text);
        assert_eq!(count, 4, "{clean}");
        for leaked in ["sk-ant", "ghp_", "MIIEow", "AKIAIOSFODNN7"] {
            assert!(!clean.contains(leaked), "{leaked} in {clean}");
        }
        assert!(clean.contains("ANTHROPIC_API_KEY=[redacted secret]"));
        assert!(clean.ends_with("ok") && clean.contains("done."));
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        for text in [
            "The sk-learn library and task-list are fine.",
            "sk-short",
            "Risk-adjusted returns: AKIA is not a key here.",
            "ünïcode — text with dashes - and_underscores",
            "-----BEGIN CERTIFICATE-----\nabc\n-----END CERTIFICATE-----",
            "",
        ] {
            assert_eq!(redact_secrets(text), (text.to_owned(), 0), "{text}");
        }
    }
}
