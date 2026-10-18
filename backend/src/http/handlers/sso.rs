//! Single sign-on with the OpenID Connect provider: the browser is sent to
//! the provider, comes back with a code, and leaves signed in exactly as a
//! password sign-in leaves it (an access token from `POST /auth/refresh`, a
//! refresh cookie). No token ever travels in an address. Also what the login
//! page offers, and the identities an account is linked to.
//!
//! The login and the callback are navigations, not API calls: they answer
//! with redirects, and a sign-in that does not happen sends the browser to
//! `/login?sso_error=<code>` while the reason goes to the server's log. A
//! signed-in account links the provider's identity with `POST
//! /auth/oidc/link` (its password confirms it), which starts the same trip;
//! that one comes back to the account page.

use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::account::confirm_password;
use super::auth::{cookie, session_config};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::account::AccountEventKind;
use crate::domain::sso::{
    LINK_RETURN_TO, LinkStart, LinkedIdentity, LoginProviders, Refusal, SignInMethods, SsoProvider,
    safe_return_to,
};
use crate::domain::user;
use crate::domain::validation::{FieldErrors, Validate};
use crate::engine::account::note;
use crate::engine::sso::{self, Resolution};
use crate::http::extract::{Caller, ClientIp, ValidatedJson};
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
            provider: provider_name(&state, &row.issuer),
            current: oidc.is_some_and(|oidc| oidc.issuer == row.issuer),
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

/// The name an identity's provider goes by: the login page's, while it is
/// the provider configured, else its issuer.
pub(super) fn provider_name(state: &AppState, issuer: &str) -> String {
    state
        .settings
        .oidc
        .as_ref()
        .filter(|oidc| oidc.issuer == issuer)
        .map_or_else(|| issuer.to_owned(), |oidc| oidc.display_name.clone())
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

/// Back to the login page (the account page, when linking) with the
/// refusal's code; the binding is cleared.
fn refused(state: &AppState, refusal: Refusal, linking: bool) -> Response {
    redirect(&refusal.location(linking), [binding_cookie(state, "", 0)])
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

/// Keeps a new sign-in for ten minutes and returns the provider's
/// authorization URL with the cookie that binds the sign-in to this browser.
/// `Ok(Err(_))` when the provider cannot be reached.
async fn start(
    state: &AppState,
    client: &OidcClient,
    return_to: String,
    link_user_id: Option<Uuid>,
) -> Result<Result<(String, HeaderValue), OidcError>, AppError> {
    let discovery = match client.discovery().await {
        Ok(discovery) => discovery,
        Err(err) => return Ok(Err(err)),
    };
    let (request_state, nonce, verifier, browser) = (
        random_token(),
        random_token(),
        random_token(),
        random_token(),
    );
    let url = match client.authorization_url(&discovery, &request_state, &nonce, &verifier) {
        Ok(url) => url,
        Err(err) => return Ok(Err(err)),
    };
    let pending = PendingLogin {
        browser_hash: token_digest(&browser),
        nonce,
        code_verifier: verifier,
        return_to,
        expires_at: Utc::now() + chrono::Duration::seconds(LOGIN_TTL_SECS),
        link_user_id,
    };
    repo::sso::start_login(&state.db, &token_digest(&request_state), &pending).await?;
    Ok(Ok((url, binding_cookie(state, &browser, LOGIN_TTL_SECS))))
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
    let return_to = safe_return_to(query.return_to.as_deref());
    match start(&state, &client, return_to, None).await? {
        Ok((url, binding)) => Ok(redirect(&url, [binding])),
        Err(err) => {
            tracing::warn!(error = %err, "single sign-on cannot start");
            Ok(refused(&state, Refusal::Unavailable, false))
        }
    }
}

/// `POST /auth/oidc/link` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LinkIdentity {
    /// The account's password: a session left open is not enough to link an identity to it.
    pub password: String,
}

impl Validate for LinkIdentity {
    fn validate(&self, errors: &mut FieldErrors) {
        if self.password.is_empty() || self.password.chars().count() > user::PASSWORD_MAX {
            errors.add("password", "enter your password");
        }
    }
}

/// Links the provider's identity to the signed-in account: the answer names
/// the provider's authorization URL for the browser to go to (with the
/// `nexc_sso` cookie set, as `GET /auth/oidc/login` sets it). Whoever signs
/// in there is linked to this account, whatever address the provider gives,
/// and the browser comes back to the account page. This is how an account
/// that is never linked by its address (a platform administrator's, or one
/// with two-factor sign-in) gets single sign-on. The break-glass account is
/// never linked: it stays independent of the provider.
#[utoipa::path(post, path = "/auth/oidc/link", tag = "auth", security(("bearer" = [])), request_body = LinkIdentity,
    responses(
        (status = 200, description = "Where to send the browser; the `nexc_sso` cookie is set", body = LinkStart),
        (status = 401, body = Problem),
        (status = 404, description = "Single sign-on is not configured", body = Problem),
        (status = 409, description = "The account is linked to an identity of the provider already, or is the break-glass account", body = Problem),
        (status = 422, description = "Wrong password", body = Problem),
        (status = 429, description = "Rate limited", body = Problem),
        (status = 503, description = "The provider cannot be reached", body = Problem),
    ))]
pub async fn oidc_link(
    _limit: AuthRateLimit,
    State(state): State<AppState>,
    auth: Caller,
    ClientIp(ip): ClientIp,
    ValidatedJson(req): ValidatedJson<LinkIdentity>,
) -> Result<Response, AppError> {
    let client = client(&state)?;
    let settings = client.settings();
    confirm_password(&state, auth, ip, &req.password, "password").await?;
    let account = repo::users::find(&state.db, auth.id)
        .await?
        .ok_or(AppError::NotFound("user"))?;
    if settings.is_break_glass(&account.email) {
        return Err(AppError::Conflict(
            "the break-glass administrator's account stays independent of the identity provider"
                .into(),
        ));
    }
    if repo::sso::has_identity_at(&state.db, auth.id, &settings.issuer).await? {
        return Err(AppError::Conflict(format!(
            "this account is linked to a {} identity already",
            settings.display_name
        )));
    }
    match start(&state, &client, LINK_RETURN_TO.to_owned(), Some(auth.id)).await? {
        Ok((authorization_url, binding)) => Ok((
            [(header::SET_COOKIE, binding)],
            Json(LinkStart { authorization_url }),
        )
            .into_response()),
        Err(err) => {
            tracing::warn!(error = %err, "single sign-on cannot start");
            Err(AppError::Unavailable(format!(
                "{} cannot be reached; try again in a moment",
                settings.display_name
            )))
        }
    }
}

/// Where the provider sends the browser back: the code is exchanged, the ID
/// token verified, the account found, linked or created, and a session
/// started as `POST /auth/login` starts one. The browser then goes to the
/// path it asked for, carrying no token. No second factor is asked: the
/// provider is the one that authenticates.
#[utoipa::path(get, path = "/auth/oidc/callback", tag = "auth", params(OidcCallbackQuery),
    responses(
        (status = 303, description = "Signed in: to the `return_to` path given at the start (the account page after `POST /auth/oidc/link`), with the `nexc_refresh` cookie set (the console then calls `POST /auth/refresh`). Not signed in: to `/login?sso_error=` (`/app/settings/profile?sso_error=` when linking) `unavailable` | `denied` | `expired` | `invalid` | `no_email` | `unverified_email` | `email_taken` | `link_required` | `identity_taken` | `suspended` | `failed`"),
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
        Err(Refused { refusal, linking }) => Ok(refused(&state, refusal, linking)),
    }
}

/// A sign-in that did not happen, and whether it was started to link an
/// identity from the account page (which it then goes back to).
struct Refused {
    refusal: Refusal,
    linking: bool,
}

impl From<Refusal> for Refused {
    fn from(refusal: Refusal) -> Self {
        Refused {
            refusal,
            linking: false,
        }
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
) -> Result<(Session, String), Refused> {
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
    let linking = pending.link_user_id.is_some();
    finish(state, client, headers, query, pending, ip)
        .await
        .map_err(|refusal| Refused { refusal, linking })
}

/// The rest of [`complete`], once the sign-in the `state` names was taken.
async fn finish(
    state: &AppState,
    client: &OidcClient,
    headers: &HeaderMap,
    query: &OidcCallbackQuery,
    pending: PendingLogin,
    ip: std::net::IpAddr,
) -> Result<(Session, String), Refusal> {
    let settings = client.settings();
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
    if let Some(user_id) = pending.link_user_id
        && let Some(refusal) = sso::link_requested(state, settings, &identity, user_id, ip)
            .await
            .map_err(failed)?
    {
        tracing::warn!(code = refusal.code(), %user_id, "single sign-on: the identity was not linked");
        return Err(refusal);
    }
    let user = match sso::sign_in(state, settings, &identity, ip)
        .await
        .map_err(failed)?
    {
        Resolution::Account(user) => user,
        Resolution::Refused(refusal) => {
            // What decided it, without the address itself.
            tracing::warn!(
                code = refusal.code(),
                issuer = %settings.issuer,
                email_given = identity.profile.email.is_some(),
                email_verified = ?identity.profile.email_verified,
                "single sign-on refused"
            );
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
