//! The OpenID Connect relying party: the provider's discovery document,
//! fetched from `{issuer}/.well-known/openid-configuration` and kept for an
//! hour.

use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::config::oidc::OidcSettings;

/// How long a discovery document is used before it is fetched again.
const DISCOVERY_TTL: Duration = Duration::from_secs(3600);
/// The largest discovery document or key set read.
const DOCUMENT_MAX_BYTES: usize = 256 * 1024;

/// Why talking to the provider did not work.
#[derive(Debug, thiserror::Error)]
pub enum OidcError {
    /// The provider could not be reached, or answered something unusable.
    #[error("the identity provider is unavailable: {0}")]
    Unavailable(String),
    /// What the provider (or the browser) presented does not verify.
    #[error("{0}")]
    Invalid(String),
}

/// The parts of the provider's discovery document the sign-in uses.
#[derive(Debug, Clone, Deserialize)]
pub struct Discovery {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub userinfo_endpoint: Option<String>,
    pub jwks_uri: String,
    #[serde(default)]
    pub token_endpoint_auth_methods_supported: Option<Vec<String>>,
    #[serde(default)]
    pub code_challenge_methods_supported: Option<Vec<String>>,
    #[serde(default)]
    pub id_token_signing_alg_values_supported: Option<Vec<String>>,
}

impl Discovery {
    /// Whether the provider says it takes S256 PKCE challenges. Providers that
    /// say nothing are tried anyway (many support it without saying).
    pub fn advertises_s256(&self) -> Option<bool> {
        self.code_challenge_methods_supported
            .as_ref()
            .map(|methods| methods.iter().any(|m| m == "S256"))
    }

    /// Checks the document against the configured issuer: the same issuer,
    /// usable endpoints, and a signing algorithm this side accepts.
    fn check(&self, settings: &OidcSettings, production: bool) -> Result<(), OidcError> {
        if self.issuer != settings.issuer {
            return Err(OidcError::Unavailable(format!(
                "the discovery document names the issuer `{}`, not `{}`",
                self.issuer, settings.issuer
            )));
        }
        let endpoints = [
            ("authorization_endpoint", Some(&self.authorization_endpoint)),
            ("token_endpoint", Some(&self.token_endpoint)),
            ("jwks_uri", Some(&self.jwks_uri)),
            ("userinfo_endpoint", self.userinfo_endpoint.as_ref()),
        ];
        for (name, url) in endpoints {
            let Some(url) = url else { continue };
            let usable = reqwest::Url::parse(url)
                .is_ok_and(|u| u.scheme() == "https" || (!production && u.scheme() == "http"));
            if !usable {
                return Err(OidcError::Unavailable(format!(
                    "the discovery document's {name} `{url}` is not an {} URL",
                    if production { "https" } else { "http(s)" }
                )));
            }
        }
        if let Some(algs) = &self.id_token_signing_alg_values_supported
            && !algs.iter().any(|a| a == "RS256" || a == "ES256")
        {
            return Err(OidcError::Unavailable(format!(
                "the provider signs ID tokens with {}; RS256 or ES256 is required",
                algs.join(", ")
            )));
        }
        Ok(())
    }
}

/// The relying party of one configured provider, shared by every request.
#[derive(Debug)]
pub struct OidcClient {
    settings: OidcSettings,
    production: bool,
    http: reqwest::Client,
    discovery: Mutex<Option<(Discovery, Instant)>>,
}

impl OidcClient {
    /// A client for `settings`; nothing is fetched until it is needed.
    pub fn new(settings: OidcSettings, production: bool) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            // A provider's endpoints answer where they are; a redirect is not followed.
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("nexc-engine/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(OidcClient {
            settings,
            production,
            http,
            discovery: Mutex::new(None),
        })
    }

    /// The configuration this client signs in with.
    pub fn settings(&self) -> &OidcSettings {
        &self.settings
    }

    /// Where the discovery document is.
    pub fn discovery_url(&self) -> String {
        format!(
            "{}/.well-known/openid-configuration",
            self.settings.issuer.trim_end_matches('/')
        )
    }

    /// The provider's discovery document: the kept one while it is less than
    /// an hour old, else fetched. A failed fetch keeps using the old one.
    pub async fn discovery(&self) -> Result<Discovery, OidcError> {
        let mut kept = self.discovery.lock().await;
        if let Some((doc, at)) = kept.as_ref()
            && at.elapsed() < DISCOVERY_TTL
        {
            return Ok(doc.clone());
        }
        match self.fetch_discovery().await {
            Ok(doc) => {
                *kept = Some((doc.clone(), Instant::now()));
                Ok(doc)
            }
            Err(err) => match kept.as_ref() {
                Some((doc, _)) => {
                    tracing::warn!(error = %err, "OIDC discovery failed; the previous document is used");
                    Ok(doc.clone())
                }
                None => Err(err),
            },
        }
    }

    async fn fetch_discovery(&self) -> Result<Discovery, OidcError> {
        let value = self.get_json(&self.discovery_url()).await?;
        let doc: Discovery = serde_json::from_value(value).map_err(|err| {
            OidcError::Unavailable(format!("the discovery document is incomplete: {err}"))
        })?;
        doc.check(&self.settings, self.production)?;
        Ok(doc)
    }

    /// GETs a JSON document of at most [`DOCUMENT_MAX_BYTES`].
    pub(crate) async fn get_json(&self, url: &str) -> Result<Value, OidcError> {
        let response = self
            .http
            .get(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|err| OidcError::Unavailable(format!("{url}: {err}")))?;
        let status = response.status();
        if !status.is_success() {
            return Err(OidcError::Unavailable(format!("{url} answered {status}")));
        }
        let bytes = read_capped(response, DOCUMENT_MAX_BYTES)
            .await
            .map_err(|err| OidcError::Unavailable(format!("{url}: {err}")))?;
        serde_json::from_slice(&bytes)
            .map_err(|err| OidcError::Unavailable(format!("{url} did not answer JSON: {err}")))
    }
}

/// Reads a response body, refusing one larger than `max` bytes.
pub(crate) async fn read_capped(
    mut response: reqwest::Response,
    max: usize,
) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|len| len > max as u64)
    {
        return Err(format!("the answer is larger than {max} bytes"));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|err| err.to_string())? {
        if body.len() + chunk.len() > max {
            return Err(format!("the answer is larger than {max} bytes"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Secret;

    fn settings() -> OidcSettings {
        OidcSettings {
            issuer: "https://idp.example.com/realms/acme".into(),
            client_id: "nexc".into(),
            client_secret: Secret::new("secret".into()),
            redirect_url: "https://nexc.example.com/api/v1/auth/oidc/callback".into(),
            scopes: vec!["openid".into()],
            display_name: "SSO".into(),
            groups_claim: "groups".into(),
            admin_groups: Vec::new(),
            workspace_roles: Vec::new(),
            allow_local_login: true,
            break_glass_email: None,
            require_verified_email: true,
        }
    }

    fn doc(issuer: &str) -> Discovery {
        Discovery {
            issuer: issuer.into(),
            authorization_endpoint: format!("{issuer}/auth"),
            token_endpoint: format!("{issuer}/token"),
            userinfo_endpoint: None,
            jwks_uri: format!("{issuer}/certs"),
            token_endpoint_auth_methods_supported: None,
            code_challenge_methods_supported: Some(vec!["plain".into(), "S256".into()]),
            id_token_signing_alg_values_supported: Some(vec!["RS256".into()]),
        }
    }

    #[test]
    fn a_discovery_document_must_be_the_issuers_own() {
        let s = settings();
        assert!(doc(&s.issuer).check(&s, true).is_ok());
        assert_eq!(doc(&s.issuer).advertises_s256(), Some(true));
        // Another issuer, even one only a slash apart, is refused (OIDC Discovery §4.3).
        assert!(
            doc("https://idp.example.com/realms/acme/")
                .check(&s, true)
                .is_err()
        );
        assert!(doc("https://evil.example.com").check(&s, true).is_err());
        let mut plain = doc(&s.issuer);
        plain.token_endpoint = "http://idp.example.com/token".into();
        assert!(plain.check(&s, true).is_err(), "plain http in production");
        assert!(plain.check(&s, false).is_ok(), "plain http in development");
        let mut hmac = doc(&s.issuer);
        hmac.id_token_signing_alg_values_supported = Some(vec!["HS256".into(), "none".into()]);
        assert!(
            hmac.check(&s, true).is_err(),
            "only RS256 and ES256 are accepted"
        );
    }
}
