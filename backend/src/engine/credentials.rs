//! Resolves which LLM provider, model and key a user's work runs with.
//!
//! A user's stored settings win over the server defaults; a user's stored
//! key wins over `ANTHROPIC_API_KEY`. Keys are decrypted only here.

use uuid::Uuid;

use crate::app::AppState;
use crate::config::Secret;
use crate::domain::AppError;
use crate::domain::settings::{KeySource, LlmProviderKind, LlmSettings, key_hint};
use crate::llm::LlmTarget;
use crate::repo;

/// The effective configuration for a user.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub target: LlmTarget,
    pub source: KeySource,
    pub key_hint: Option<String>,
}

impl Resolved {
    /// The user-facing view (never includes the key).
    pub fn settings(&self) -> LlmSettings {
        LlmSettings {
            provider: self.target.provider,
            model: self.target.model.clone(),
            base_url: self.target.base_url.clone(),
            has_api_key: self.target.api_key.is_some(),
            key_hint: self.key_hint.clone(),
            source: self.source,
        }
    }

    /// True when requests can be sent (a key exists if the provider needs one).
    pub fn is_usable(&self) -> bool {
        !self.target.provider.requires_api_key() || self.target.api_key.is_some()
    }

    /// True for the offline demo provider.
    pub fn is_demo(&self) -> bool {
        self.target.provider == LlmProviderKind::Demo
    }
}

/// Loads and decrypts the user's effective LLM configuration.
pub async fn resolve(state: &AppState, user_id: Uuid) -> Result<Resolved, AppError> {
    let s = &state.settings;
    let stored = repo::settings::find(&state.db, user_id).await?;
    let user_key = match stored.as_ref().and_then(|row| row.api_key_enc.as_ref()) {
        Some(sealed) => {
            let plain = state.secret_box.open(sealed, user_id.as_bytes())?;
            Some(Secret::new(
                String::from_utf8(plain).map_err(anyhow::Error::from)?,
            ))
        }
        None => None,
    };
    let (provider, model, base_url) = match &stored {
        Some(row) => (row.provider, row.model.clone(), row.base_url.clone()),
        None => (s.llm_provider, s.llm_model.clone(), s.llm_base_url.clone()),
    };
    let server_key = (provider == LlmProviderKind::Anthropic)
        .then(|| s.anthropic_api_key.clone())
        .flatten();
    let (api_key, source, hint) = match (user_key, server_key) {
        (Some(k), _) => {
            let hint = stored
                .as_ref()
                .and_then(|r| r.key_hint.clone())
                .or_else(|| Some(key_hint(k.expose())));
            (Some(k), KeySource::User, hint)
        }
        (None, Some(k)) => (Some(k), KeySource::Server, None),
        (None, None) => (None, KeySource::None, None),
    };
    Ok(Resolved {
        target: LlmTarget {
            provider,
            model,
            base_url,
            api_key,
        },
        source,
        key_hint: hint,
    })
}

/// Like [`resolve`] but fails fast with a 422 when no usable key exists.
pub async fn require(state: &AppState, user_id: Uuid) -> Result<Resolved, AppError> {
    let resolved = resolve(state, user_id).await?;
    if resolved.is_usable() {
        Ok(resolved)
    } else {
        Err(AppError::Unprocessable(
            "No LLM API key configured — add one in Settings, or switch the provider to \"demo\" \
             to try planning and runs offline without a key."
                .into(),
        ))
    }
}
