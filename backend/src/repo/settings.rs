//! Per-user LLM settings (the API key is stored AES-GCM encrypted).

use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::settings::LlmProviderKind;

/// The stored row.
#[derive(Debug)]
pub struct StoredLlmSettings {
    pub provider: LlmProviderKind,
    pub model: String,
    pub base_url: Option<String>,
    pub api_key_enc: Option<Vec<u8>>,
    pub key_hint: Option<String>,
}

impl FromRow<'_, PgRow> for StoredLlmSettings {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(StoredLlmSettings {
            provider: enum_col(row, "provider")?,
            model: row.try_get("model")?,
            base_url: row.try_get("base_url")?,
            api_key_enc: row.try_get("api_key_enc")?,
            key_hint: row.try_get("key_hint")?,
        })
    }
}

/// Loads the user's settings, if any.
pub async fn find(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
) -> Result<Option<StoredLlmSettings>, sqlx::Error> {
    sqlx::query_as("SELECT provider, model, base_url, api_key_enc, key_hint FROM llm_settings WHERE user_id = $1")
        .bind(user_id)
        .fetch_optional(db)
        .await
}

/// How the stored key should change on update.
#[derive(Debug)]
pub enum KeyUpdate {
    /// Leave the stored key untouched.
    Keep,
    /// Delete the stored key.
    Clear,
    /// Replace with this ciphertext and hint.
    Set { ciphertext: Vec<u8>, hint: String },
}

/// Creates or updates the user's settings.
pub async fn upsert(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    provider: LlmProviderKind,
    model: &str,
    base_url: Option<&str>,
    key: KeyUpdate,
) -> Result<StoredLlmSettings, sqlx::Error> {
    let (keep, enc, hint) = match key {
        KeyUpdate::Keep => (true, None, None),
        KeyUpdate::Clear => (false, None, None),
        KeyUpdate::Set { ciphertext, hint } => (false, Some(ciphertext), Some(hint)),
    };
    sqlx::query_as(
        "INSERT INTO llm_settings (user_id, provider, model, base_url, api_key_enc, key_hint)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (user_id) DO UPDATE SET
            provider = EXCLUDED.provider,
            model = EXCLUDED.model,
            base_url = EXCLUDED.base_url,
            api_key_enc = CASE WHEN $7 THEN llm_settings.api_key_enc ELSE EXCLUDED.api_key_enc END,
            key_hint = CASE WHEN $7 THEN llm_settings.key_hint ELSE EXCLUDED.key_hint END,
            updated_at = now()
         RETURNING provider, model, base_url, api_key_enc, key_hint",
    )
    .bind(user_id)
    .bind(provider.as_str())
    .bind(model)
    .bind(base_url)
    .bind(enc)
    .bind(hint)
    .bind(keep)
    .fetch_one(db)
    .await
}
