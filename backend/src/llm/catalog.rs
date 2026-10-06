//! The models a provider currently offers, asked from the provider itself.
//!
//! Nothing here names a model: the list is whatever the configured endpoint
//! answers, so new releases appear without a code change.

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::LlmTarget;
use crate::domain::AppError;
use crate::domain::settings::LlmProviderKind;

const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// A model released within this many days counts as recent.
pub const RECENT_DAYS: i64 = 120;
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// One model a provider offers.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct ModelInfo {
    /// The identifier to configure.
    pub id: String,
    pub name: String,
    /// When the provider released it, if it says.
    #[schema(required = true)]
    pub released_at: Option<DateTime<Utc>>,
    /// Released within the last [`RECENT_DAYS`] days.
    pub recent: bool,
}

/// What `GET /settings/llm/models` returns.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ModelCatalog {
    pub provider: LlmProviderKind,
    /// Newest first; models without a release date come last, by id.
    pub models: Vec<ModelInfo>,
    /// Why the list is empty or partial, when the provider cannot be asked.
    #[schema(required = true)]
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Listing {
    #[serde(default)]
    data: Vec<Entry>,
}

/// A model as Anthropic (`display_name`, RFC 3339 `created_at`) or an
/// OpenAI-compatible server (`created` in Unix seconds) describes it.
#[derive(Debug, Deserialize)]
struct Entry {
    id: String,
    display_name: Option<String>,
    created_at: Option<DateTime<Utc>>,
    created: Option<i64>,
}

/// Turns a provider's `/models` answer into the catalog, newest first.
fn parse(body: &str, now: DateTime<Utc>) -> Result<Vec<ModelInfo>, serde_json::Error> {
    let listing: Listing = serde_json::from_str(body)?;
    let mut models: Vec<ModelInfo> = listing
        .data
        .into_iter()
        .filter(|e| !e.id.trim().is_empty())
        .map(|e| {
            // Local servers often report 0 for "unknown".
            let released_at = e.created_at.or_else(|| {
                e.created
                    .filter(|secs| *secs > 0)
                    .and_then(|secs| Utc.timestamp_opt(secs, 0).single())
            });
            ModelInfo {
                name: e.display_name.unwrap_or_else(|| e.id.clone()),
                recent: released_at.is_some_and(|at| (now - at).num_days() <= RECENT_DAYS),
                released_at,
                id: e.id,
            }
        })
        .collect();
    models.sort_by(|a, b| {
        b.released_at
            .cmp(&a.released_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(models)
}

/// Asks the provider behind `target` which models it offers.
pub async fn list(http: &reqwest::Client, target: &LlmTarget) -> Result<ModelCatalog, AppError> {
    let provider = target.provider;
    let empty = |note: &str| ModelCatalog {
        provider,
        models: Vec::new(),
        note: Some(note.to_owned()),
    };
    let request = match provider {
        LlmProviderKind::Demo => {
            return Ok(empty(
                "The demo provider is offline and has no models to choose from.",
            ));
        }
        LlmProviderKind::ClaudeCode => {
            return Ok(empty(
                "The Claude Code CLI does not list models; use an alias such as `sonnet`, `opus` \
                 or `haiku`, or a full model id your account has access to.",
            ));
        }
        LlmProviderKind::Anthropic => {
            let Some(key) = &target.api_key else {
                return Ok(empty(
                    "Add an API key to see the models your account can use.",
                ));
            };
            let base = target.base_url.as_deref().unwrap_or(ANTHROPIC_BASE_URL);
            http.get(format!(
                "{}/v1/models?limit=1000",
                base.trim_end_matches('/')
            ))
            .header("x-api-key", key.expose())
            .header("anthropic-version", ANTHROPIC_VERSION)
        }
        LlmProviderKind::OpenaiCompatible => {
            let base = target
                .base_url
                .as_deref()
                .unwrap_or(super::openai_compat::DEFAULT_BASE_URL);
            let request = http.get(format!("{}/models", base.trim_end_matches('/')));
            match &target.api_key {
                Some(key) => request.bearer_auth(key.expose()),
                None => request,
            }
        }
    };
    let unreachable = |detail: String| {
        AppError::Unprocessable(format!("could not list the provider's models: {detail}"))
    };
    let response = request
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|e| unreachable(e.without_url().to_string()))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| unreachable(e.without_url().to_string()))?;
    if !status.is_success() {
        return Err(unreachable(format!("the provider answered HTTP {status}")));
    }
    let models = parse(&body, Utc::now())
        .map_err(|_| unreachable("the provider's answer was not a model list".into()))?;
    Ok(ModelCatalog {
        provider,
        models,
        note: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_both_listing_dialects_newest_first() {
        let now = Utc.with_ymd_and_hms(2026, 10, 6, 0, 0, 0).unwrap();
        let anthropic = r#"{"data": [
            {"type": "model", "id": "old-model", "display_name": "Old", "created_at": "2025-02-19T00:00:00Z"},
            {"type": "model", "id": "new-model", "display_name": "New", "created_at": "2026-08-01T00:00:00Z"}
        ], "has_more": false}"#;
        let models = parse(anthropic, now).unwrap();
        assert_eq!(models[0].id, "new-model");
        assert_eq!(models[0].name, "New");
        assert!(models[0].recent && !models[1].recent);

        let openai = r#"{"object": "list", "data": [
            {"id": "local-b", "object": "model", "created": 0},
            {"id": "hosted", "object": "model", "created": 1790000000},
            {"id": "local-a", "object": "model"},
            {"id": " "}
        ]}"#;
        let models = parse(openai, now).unwrap();
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            ["hosted", "local-a", "local-b"],
            "undated models last, by id"
        );
        assert_eq!(models[1].released_at, None);
        assert_eq!(models[1].name, "local-a");
        assert!(parse("<html>", now).is_err());
        assert!(parse("{}", now).unwrap().is_empty());
    }
}
