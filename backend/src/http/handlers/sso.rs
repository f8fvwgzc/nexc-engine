//! Single sign-on with the OpenID Connect provider: the browser is sent to
//! the provider, comes back with a code, and leaves signed in exactly as a
//! password sign-in leaves it (an access token from `POST /auth/refresh`, a
//! refresh cookie). No token ever travels in an address. Also what the login
//! page offers, and the identities an account is linked to.
//!
//! The login and the callback are navigations, not API calls: they answer
//! with redirects, and a sign-in that does not happen sends the browser to
//! `/login?sso_error=<code>` while the reason goes to the server's log.

use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use serde::Deserialize;
use utoipa::IntoParams;

use super::auth::{cookie, session_config};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::account::AccountEventKind;
use crate::domain::sso::{
    LinkedIdentity, LoginProviders, Refusal, SignInMethods, SsoProvider, safe_return_to,
};
use crate::engine::account::note;
use crate::engine::sso::{self, Resolution};
use crate::http::extract::{Caller, ClientIp};
use crate::http::middleware::rate_limit::AuthRateLimit;
use crate::http::problem::Problem;
use crate::repo;
use crate::repo::sso::PendingLogin;
use crate::security::oidc::{OidcClient, OidcError};
use crate::security::random::{constant_time_eq, random_token, token_digest};
use crate::security::session::{self, Session};

/// The cookie that binds a sign-in to the browser that started it.
pub const BINDING_COOKIE: &str = "nexc_sso";
const BINDING_PATH: &str = "/api/v1/auth/oidc";
/// How long a sign-in may take at the provider, in seconds.
pub const LOGIN_TTL_SECS: i64 = 600;
/// Longest `state` and `code` looked at.
const STATE_MAX: usize = 128;
const CODE_MAX: usize = 4096;

/// Query of `GET /auth/oidc/login`.
#[derive(Debug, Default, Deserialize, IntoParams)]
pub struct OidcLoginQuery {
    /// Where to go once signed in: a path of this site (`/app/...`).
    /// Anything else, an address of another site included, is ignored for `/app`.
    pub return_to: Option<String>,
}

/// Query of `GET /auth/oidc/callback`, as the provider sends the browser back.
#[derive(Debug, Default, Deserialize, IntoParams)]
pub struct OidcCallbackQuery {
    /// The authorization code.
    pub code: Option<String>,
    /// The `state` of the authorization request; it works once, for ten minutes.
    pub state: Option<String>,
    /// Set by the provider when the sign-in did not happen there.
    pub error: Option<String>,
    /// The provider's explanation of `error`; logged, never shown.
    pub error_description: Option<String>,
    /// The issuer the answer comes from (RFC 9207); when given it must be the configured one.
    pub iss: Option<String>,
}

fn client(state: &AppState) -> Result<std::sync::Arc<OidcClient>, AppError> {
    state
        .oidc
        .clone()
        .ok_or(AppError::NotFound("single sign-on"))
}

/// The ways to sign in here, for the login page: a password or not,
/// registration or not, and the single sign-on provider.
#[utoipa::path(get, path = "/auth/providers", tag = "auth",
    responses((status = 200, body = LoginProviders)))]
pub async fn providers(State(state): State<AppState>) -> Json<LoginProviders> {
    let settings = &state.settings;
    let oidc = settings.oidc.as_ref();
    let password = oidc.is_none_or(|oidc| oidc.allow_local_login);
    Json(LoginProviders {
        password,
        signup: password && settings.allow_signup,
        sso: oidc
            .map(|oidc| SsoProvider {
                id: "oidc".into(),
                display_name: oidc.display_name.clone(),
                login_url: "/api/v1/auth/oidc/login".into(),
            })
            .into_iter()
            .collect(),
    })
}

/// How the caller signs in: whether the account has a password, and the
/// single sign-on identities linked to it.
#[utoipa::path(get, path = "/auth/identities", tag = "auth", security(("bearer" = [])),
    responses((status = 200, body = SignInMethods), (status = 401, body = Problem)))]
pub async fn identities(
    State(state): State<AppState>,
    auth: Caller,
) -> Result<Json<SignInMethods>, AppError> {
    let oidc = state.settings.oidc.as_ref();
    let identities = repo::sso::identities(&state.db, auth.id)
        .await?
        .into_iter()
        .map(|row| LinkedIdentity {
            provider: oidc
                .filter(|oidc| oidc.issuer == row.issuer)
                .map_or_else(|| row.issuer.clone(), |oidc| oidc.display_name.clone()),
            issuer: row.issuer,
            email: row.email,
            linked_at: row.created_at,
            last_login_at: row.last_login_at,
        })
        .collect();
    Ok(Json(SignInMethods {
        password: repo::sso::has_password(&state.db, auth.id).await?,
        identities,
    }))
}

/// The binding cookie: `SameSite=Lax`, because the provider sends the
/// browser back from another site.
fn binding_cookie(state: &AppState, value: &str, max_age: i64) -> HeaderValue {
    let secure = if state.settings.cookie_secure {
        "; Secure"
    } else {
        ""
    };
    let raw = format!(
        "{BINDING_COOKIE}={value}; HttpOnly; SameSite=Lax; Path={BINDING_PATH}; Max-Age={max_age}{secure}"
    );
    HeaderValue::from_str(&raw).expect("cookie is ASCII")
}

/// A `303 See Other` to `location`, setting `cookies`.
fn redirect(location: &str, cookies: impl IntoIterator<Item = HeaderValue>) -> Response {
    let mut response = StatusCode::SEE_OTHER.into_response();
    let headers = response.headers_mut();
    if let Ok(location) = HeaderValue::from_str(location) {
        headers.insert(header::LOCATION, location);
    }
    for cookie in cookies {
        headers.append(header::SET_COOKIE, cookie);
    }
    response
}

/// Back to the login page with the refusal's code; the binding is cleared.
fn refused(state: &AppState, refusal: Refusal) -> Response {
    redirect(&refusal.location(), [binding_cookie(state, "", 0)])
}

/// The value of cookie `name` in the request.
fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|pair| {
            let (key, value) = pair.trim().split_once('=')?;
            (key == name).then(|| value.to_owned())
        })
        .filter(|v| !v.is_empty() && v.len() <= 128)
}

/// Sends the browser to the provider to sign in.
#[utoipa::path(get, path = "/auth/oidc/login", tag = "auth", params(OidcLoginQuery),
    responses(
        (status = 303, description = "To the provider's authorization endpoint (authorization code with PKCE S256, a `state` and a `nonce` kept here for ten minutes), with the `nexc_sso` cookie binding the sign-in to this browser. When the provider cannot be reached: to `/login?sso_error=unavailable`"),
        (status = 404, description = "Single sign-on is not configured", body = Problem),
        (status = 429, description = "Rate limited", body = Problem),
    ))]
pub async fn oidc_login(
    _limit: AuthRateLimit,
    State(state): State<AppState>,
    query: Result<Query<OidcLoginQuery>, QueryRejection>,
) -> Result<Response, AppError> {
    let client = client(&state)?;
    let query = query.map(|Query(q)| q).unwrap_or_default();
    let discovery = match client.discovery().await {
        Ok(discovery) => discovery,
        Err(err) => {
            tracing::warn!(error = %err, "single sign-on cannot start");
            return Ok(refused(&state, Refusal::Unavailable));
        }
    };
    let (request_state, nonce, verifier, browser) = (
        random_token(),
        random_token(),
        random_token(),
        random_token(),
    );
    let url = match client.authorization_url(&discovery, &request_state, &nonce, &verifier) {
        Ok(url) => url,
        Err(err) => {
            tracing::warn!(error = %err, "single sign-on cannot start");
            return Ok(refused(&state, Refusal::Unavailable));
        }
    };
    let pending = PendingLogin {
        browser_hash: token_digest(&browser),
        nonce,
        code_verifier: verifier,
        return_to: safe_return_to(query.return_to.as_deref()),
        expires_at: Utc::now() + chrono::Duration::seconds(LOGIN_TTL_SECS),
    };
    repo::sso::start_login(&state.db, &token_digest(&request_state), &pending).await?;
    Ok(redirect(
        &url,
        [binding_cookie(&state, &browser, LOGIN_TTL_SECS)],
    ))
}

/// Where the provider sends the browser back: the code is exchanged, the ID
/// token verified, the account found, linked or created, and a session
/// started as `POST /auth/login` starts one. The browser then goes to the
/// path it asked for, carrying no token. No second factor is asked: the
/// provider is the one that authenticates.
#[utoipa::path(get, path = "/auth/oidc/callback", tag = "auth", params(OidcCallbackQuery),
    responses(
        (status = 303, description = "Signed in: to the `return_to` path given at the start, with the `nexc_refresh` cookie set (the console then calls `POST /auth/refresh`). Not signed in: to `/login?sso_error=` `unavailable` | `denied` | `expired` | `invalid` | `no_email` | `unverified_email` | `email_taken` | `suspended` | `failed`"),
        (status = 404, description = "Single sign-on is not configured", body = Problem),
        (status = 429, description = "Rate limited", body = Problem),
    ))]
pub async fn oidc_callback(
    _limit: AuthRateLimit,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    query: Result<Query<OidcCallbackQuery>, QueryRejection>,
) -> Result<Response, AppError> {
    let client = client(&state)?;
    let query = query.map(|Query(q)| q).unwrap_or_default();
    match complete(&state, &client, &headers, &query, ip).await {
        Ok((session, return_to)) => Ok(redirect(
            &return_to,
            [
                cookie(
                    &state,
                    &session.refresh_token,
                    state.settings.refresh_ttl.as_secs(),
                ),
                binding_cookie(&state, "", 0),
            ],
        )),
        Err(refusal) => Ok(refused(&state, refusal)),
    }
}

/// Logs why the provider's answer was not good enough.
fn provider_refusal(err: OidcError) -> Refusal {
    match err {
        OidcError::Unavailable(reason) => {
            tracing::warn!(%reason, "single sign-on: the provider is unavailable");
            Refusal::Unavailable
        }
        OidcError::Invalid(reason) => {
            tracing::warn!(%reason, "single sign-on refused");
            Refusal::Invalid
        }
    }
}

/// Logs a failure on this side.
fn failed(err: impl std::fmt::Display) -> Refusal {
    tracing::error!(error = %err, "single sign-on failed");
    Refusal::Failed
}

/// Everything between the provider's answer and a session.
async fn complete(
    state: &AppState,
    client: &OidcClient,
    headers: &HeaderMap,
    query: &OidcCallbackQuery,
    ip: std::net::IpAddr,
) -> Result<(Session, String), Refusal> {
    let settings = client.settings();
    let request_state = query
        .state
        .as_deref()
        .filter(|s| !s.is_empty() && s.len() <= STATE_MAX)
        .ok_or(Refusal::Invalid)?;
    // Taken before anything else is looked at: a state works once, whatever came with it.
    let pending = repo::sso::take_login(&state.db, &token_digest(request_state))
        .await
        .map_err(failed)?
        .ok_or(Refusal::Expired)?;
    if pending.expires_at <= Utc::now() {
        return Err(Refusal::Expired);
    }
    let browser = cookie_value(headers, BINDING_COOKIE).unwrap_or_default();
    if !constant_time_eq(
        token_digest(&browser).as_bytes(),
        pending.browser_hash.as_bytes(),
    ) {
        tracing::warn!(
            "single sign-on refused: the browser that came back is not the one that left"
        );
        return Err(Refusal::Invalid);
    }
    if query
        .iss
        .as_ref()
        .is_some_and(|iss| *iss != settings.issuer)
    {
        tracing::warn!("single sign-on refused: the answer names another issuer");
        return Err(Refusal::Invalid);
    }
    if let Some(error) = &query.error {
        let error: String = error.chars().take(100).collect();
        let description: String = query
            .error_description
            .as_deref()
            .unwrap_or_default()
            .chars()
            .take(300)
            .collect();
        tracing::info!(%error, %description, "single sign-on declined at the provider");
        return Err(Refusal::Denied);
    }
    let code = query
        .code
        .as_deref()
        .filter(|c| !c.is_empty() && c.len() <= CODE_MAX)
        .ok_or(Refusal::Invalid)?;
    let discovery = client.discovery().await.map_err(provider_refusal)?;
    let tokens = client
        .exchange(&discovery, code, &pending.code_verifier)
        .await
        .map_err(provider_refusal)?;
    let mut identity = client
        .verify_id_token(&discovery, &tokens.id_token, &pending.nonce)
        .await
        .map_err(provider_refusal)?;
    if !identity.profile.is_complete()
        && let Some(access_token) = &tokens.access_token
    {
        let more = client
            .userinfo(&discovery, access_token, &identity.subject)
            .await
            .map_err(provider_refusal)?;
        identity.profile.fill_from(more);
    }
    let user = match sso::sign_in(state, settings, &identity, ip)
        .await
        .map_err(failed)?
    {
        Resolution::Account(user) => user,
        Resolution::Refused(refusal) => {
            tracing::info!(code = refusal.code(), "single sign-on refused");
            return Err(refusal);
        }
    };
    let id = user.id;
    let session = session::start(&state.db, session_config(state), user)
        .await
        .map_err(failed)?;
    let kind = AccountEventKind::SsoSignedIn;
    note(state, id, kind, Some(ip), &settings.display_name).await;
    Ok((session, pending.return_to))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_binding_cookie() {
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            HeaderValue::from_static("a=1; nexc_sso=abc; nexc_sso_other=x"),
        );
        assert_eq!(cookie_value(&h, BINDING_COOKIE).as_deref(), Some("abc"));
        h.insert(header::COOKIE, HeaderValue::from_static("nexc_sso="));
        assert_eq!(cookie_value(&h, BINDING_COOKIE), None);
    }
}
