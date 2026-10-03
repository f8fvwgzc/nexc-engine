//! Per-user LLM settings.

use std::net::IpAddr;

use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::settings::{LlmProviderKind, LlmSettings, key_hint};
use crate::domain::validation::{FieldErrors, Validate, check_text};
use crate::engine::credentials;
use crate::http::extract::{AuthUser, ValidatedJson};
use crate::http::problem::Problem;
use crate::repo;
use crate::repo::settings::KeyUpdate;

/// `PUT /settings/llm` body. `api_key`: omitted keeps the stored key,
/// `""` deletes it, any other value replaces it.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateLlmSettings {
    pub provider: LlmProviderKind,
    pub model: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
}

impl Validate for UpdateLlmSettings {
    fn validate(&self, errors: &mut FieldErrors) {
        check_text(errors, "model", &self.model, 100);
        if let Some(url) = self.base_url.as_deref().filter(|u| !u.is_empty())
            && !is_http_url(url)
        {
            errors.add(
                "base_url",
                "must be an http(s) URL of at most 300 characters",
            );
        }
        if let Some(key) = self.api_key.as_deref().filter(|k| !k.is_empty())
            && (key.len() < 8
                || key.len() > 512
                || key.chars().any(|c| c.is_whitespace() || c.is_control()))
        {
            errors.add(
                "api_key",
                "must be 8-512 printable characters without spaces",
            );
        }
    }
}

fn is_http_url(url: &str) -> bool {
    url.len() <= 300
        && reqwest::Url::parse(url)
            .is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
}

/// In production only public `https` endpoints may be configured, so the
/// server cannot be pointed at internal services (SSRF).
fn check_production_url(url: &str) -> Result<(), AppError> {
    let parsed =
        reqwest::Url::parse(url).map_err(|_| AppError::field("base_url", "invalid URL"))?;
    let host = parsed.host_str().unwrap_or_default();
    let private_ip = host
        .trim_matches(['[', ']'])
        .parse::<IpAddr>()
        .is_ok_and(|ip| match ip {
            IpAddr::V4(v4) => {
                v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_unspecified()
            }
            IpAddr::V6(v6) => {
                v6.is_loopback() || v6.is_unspecified() || (v6.segments()[0] & 0xfe00) == 0xfc00
            }
        });
    if parsed.scheme() != "https"
        || private_ip
        || host == "localhost"
        || host.ends_with(".internal")
    {
        return Err(AppError::field(
            "base_url",
            "must be a public https URL in production",
        ));
    }
    Ok(())
}

/// The effective LLM settings of the caller.
#[utoipa::path(get, path = "/settings/llm", tag = "settings", security(("bearer" = [])),
    responses((status = 200, body = LlmSettings), (status = 401, body = Problem)))]
pub async fn get_llm(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<LlmSettings>, AppError> {
    Ok(Json(
        credentials::resolve(&state, auth.id).await?.settings(),
    ))
}

/// Updates provider, model, base URL and (encrypted) API key.
#[utoipa::path(put, path = "/settings/llm", tag = "settings", security(("bearer" = [])),
    request_body = UpdateLlmSettings,
    responses((status = 200, body = LlmSettings), (status = 422, body = Problem)))]
pub async fn put_llm(
    State(state): State<AppState>,
    auth: AuthUser,
    ValidatedJson(req): ValidatedJson<UpdateLlmSettings>,
) -> Result<Json<LlmSettings>, AppError> {
    let base_url = req
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty());
    if let Some(url) = base_url
        && state.settings.is_production()
    {
        check_production_url(url)?;
    }
    let key = match req.api_key.as_deref() {
        None => KeyUpdate::Keep,
        Some("") => KeyUpdate::Clear,
        Some(key) => KeyUpdate::Set {
            ciphertext: state.secret_box.seal(key.as_bytes(), auth.id.as_bytes())?,
            hint: key_hint(key),
        },
    };
    repo::settings::upsert(
        &state.db,
        auth.id,
        req.provider,
        req.model.trim(),
        base_url,
        key,
    )
    .await?;
    Ok(Json(
        credentials::resolve(&state, auth.id).await?.settings(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_urls_must_be_public_https() {
        assert!(check_production_url("https://api.example.com/v1").is_ok());
        for bad in [
            "http://api.example.com",
            "https://localhost:11434",
            "https://127.0.0.1",
            "https://10.0.0.5",
            "https://[::1]:8080",
            "https://metadata.internal",
        ] {
            assert!(check_production_url(bad).is_err(), "{bad}");
        }
        assert!(is_http_url("http://localhost:11434/v1"));
        assert!(!is_http_url("file:///etc/passwd"));
    }
}
