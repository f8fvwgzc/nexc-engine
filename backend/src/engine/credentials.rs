//! Resolves which LLM provider, model and key a user's work runs with.
//!
//! The account a user connected wins over the credential of the workspace
//! they are working in, which wins over the server defaults (and
//! `ANTHROPIC_API_KEY`). Keys are decrypted only here.

use uuid::Uuid;

use crate::app::AppState;
use crate::config::Secret;
use crate::domain::AppError;
use crate::domain::settings::{ConfigScope, KeySource, LlmProviderKind, LlmSettings, key_hint};
use crate::llm::LlmTarget;
use crate::repo;

/// The effective configuration for a user.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub target: LlmTarget,
    pub source: KeySource,
    /// Whose configuration this is.
    pub scope: ConfigScope,
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
            scope: self.scope,
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

/// Loads and decrypts the LLM configuration that applies to work `user_id`
/// does in `workspace_id`: the account the user connected themselves, else
/// the workspace's credential, else the server defaults. Usage is therefore
/// spent on whichever of them supplied the key.
pub async fn resolve(
    state: &AppState,
    user_id: Uuid,
    workspace_id: Option<Uuid>,
) -> Result<Resolved, AppError> {
    let s = &state.settings;
    let mut stored = repo::settings::find(&state.db, user_id)
        .await?
        .map(|row| (row, ConfigScope::User, user_id));
    if stored.is_none()
        && let Some(workspace_id) = workspace_id
    {
        stored = repo::settings::find_for_workspace(&state.db, workspace_id)
            .await?
            .map(|row| (row, ConfigScope::Workspace, workspace_id));
    }
    let scope = stored
        .as_ref()
        .map_or(ConfigScope::Server, |(_, scope, _)| *scope);
    // The key is sealed to the id of whoever owns it (user or workspace).
    let own_key = match stored.as_ref() {
        Some((row, _, sealed_to)) => match row.api_key_enc.as_ref() {
            Some(sealed) => {
                let plain = state.secret_box.open(sealed, sealed_to.as_bytes())?;
                Some(Secret::new(
                    String::from_utf8(plain).map_err(anyhow::Error::from)?,
                ))
            }
            None => None,
        },
        None => None,
    };
    let stored = stored.map(|(row, _, _)| row);
    let (provider, model, base_url) = match &stored {
        Some(row) => (row.provider, row.model.clone(), row.base_url.clone()),
        None => (s.llm_provider, s.llm_model.clone(), s.llm_base_url.clone()),
    };
    let server_key = (provider == LlmProviderKind::Anthropic)
        .then(|| s.anthropic_api_key.clone())
        .flatten();
    let (api_key, source, hint) = match (own_key, server_key) {
        (Some(k), _) => {
            let hint = stored
                .as_ref()
                .and_then(|r| r.key_hint.clone())
                .or_else(|| Some(key_hint(k.expose())));
            let source = match scope {
                ConfigScope::Workspace => KeySource::Workspace,
                _ => KeySource::User,
            };
            (Some(k), source, hint)
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
        scope,
        key_hint: hint,
    })
}

/// The credential a workspace holds, as its members see it; `None` when it has none.
pub async fn workspace_settings(
    state: &AppState,
    workspace_id: Uuid,
) -> Result<Option<LlmSettings>, AppError> {
    let Some(row) = repo::settings::find_for_workspace(&state.db, workspace_id).await? else {
        return Ok(None);
    };
    Ok(Some(LlmSettings {
        provider: row.provider,
        model: row.model,
        base_url: row.base_url,
        has_api_key: row.api_key_enc.is_some(),
        key_hint: row.key_hint,
        source: if row.api_key_enc.is_some() {
            KeySource::Workspace
        } else {
            KeySource::None
        },
        scope: ConfigScope::Workspace,
    }))
}

/// Like [`resolve`] but fails fast with a 422 when no usable key exists.
pub async fn require(
    state: &AppState,
    user_id: Uuid,
    workspace_id: Option<Uuid>,
) -> Result<Resolved, AppError> {
    let resolved = resolve(state, user_id, workspace_id).await?;
    if resolved.is_usable() {
        Ok(resolved)
    } else {
        Err(AppError::Unprocessable(
            "No LLM API key configured — add one in Settings, ask a workspace admin to set the \
             workspace credential, or switch the provider to \"demo\" to try planning and runs \
             offline without a key."
                .into(),
        ))
    }
}
