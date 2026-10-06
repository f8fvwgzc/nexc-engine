//! LLM settings of users and of workspaces (API keys are stored AES-GCM encrypted).

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

/// Removes the user's own settings so that the workspace's (or the server's) apply.
pub async fn delete(db: impl PgExecutor<'_>, user_id: Uuid) -> Result<bool, sqlx::Error> {
    let done = sqlx::query("DELETE FROM llm_settings WHERE user_id = $1")
        .bind(user_id)
        .execute(db)
        .await?;
    Ok(done.rows_affected() == 1)
}

/// Loads the credential of a workspace, if it has one.
pub async fn find_for_workspace(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<Option<StoredLlmSettings>, sqlx::Error> {
    sqlx::query_as(
        "SELECT provider, model, base_url, api_key_enc, key_hint FROM workspace_llm_settings
         WHERE workspace_id = $1",
    )
    .bind(workspace_id)
    .fetch_optional(db)
    .await
}

/// Creates or updates the credential of a workspace.
pub async fn upsert_for_workspace(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    updated_by: Uuid,
    provider: LlmProviderKind,
    model: &str,
    base_url: Option<&str>,
    key: KeyUpdate,
) -> Result<(), sqlx::Error> {
    let (keep, enc, hint) = key.parts();
    sqlx::query(
        "INSERT INTO workspace_llm_settings
            (workspace_id, provider, model, base_url, api_key_enc, key_hint, updated_by)
         VALUES ($1, $2, $3, $4, $5, $6, $8)
         ON CONFLICT (workspace_id) DO UPDATE SET
            provider = EXCLUDED.provider,
            model = EXCLUDED.model,
            base_url = EXCLUDED.base_url,
            api_key_enc = CASE WHEN $7 THEN workspace_llm_settings.api_key_enc ELSE EXCLUDED.api_key_enc END,
            key_hint = CASE WHEN $7 THEN workspace_llm_settings.key_hint ELSE EXCLUDED.key_hint END,
            updated_by = EXCLUDED.updated_by,
            updated_at = now()",
    )
    .bind(workspace_id)
    .bind(provider.as_str())
    .bind(model)
    .bind(base_url)
    .bind(enc)
    .bind(hint)
    .bind(keep)
    .bind(updated_by)
    .execute(db)
    .await?;
    Ok(())
}

/// Removes the credential of a workspace. Returns false if it had none.
pub async fn delete_for_workspace(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query("DELETE FROM workspace_llm_settings WHERE workspace_id = $1")
        .bind(workspace_id)
        .execute(db)
        .await?;
    Ok(done.rows_affected() == 1)
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

impl KeyUpdate {
    /// `(keep the stored key, new ciphertext, new hint)` as the upserts bind them.
    pub(crate) fn parts(self) -> (bool, Option<Vec<u8>>, Option<String>) {
        match self {
            KeyUpdate::Keep => (true, None, None),
            KeyUpdate::Clear => (false, None, None),
            KeyUpdate::Set { ciphertext, hint } => (false, Some(ciphertext), Some(hint)),
        }
    }
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
    let (keep, enc, hint) = key.parts();
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
