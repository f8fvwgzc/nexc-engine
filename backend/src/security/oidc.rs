//! The OpenID Connect relying party, authorization code flow with PKCE
//! (S256): the provider's discovery document and signing keys, kept and
//! refreshed; the authorization request; the code exchange; the ID token's
//! verification; and the userinfo answer.
//!
//! An ID token is accepted only when all of this holds:
//! * it is signed with RS256 or ES256 (never `none`, never an HMAC, whatever
//!   the header says) by a key of the provider's key set: a key id the set
//!   does not hold makes it fetched again once (the provider rotated), at
//!   most once a minute, and a key set is kept for an hour (a refresh that
//!   fails keeps the old set and is tried again after a minute);
//! * `iss` is the configured issuer, `aud` names the client and, when it
//!   names others too, `azp` is the client;
//! * `exp` has not passed and `iat` is not in the future, nor older than ten
//!   minutes, give or take a minute of clock skew;
//! * `nonce` is the one this server sent with the request.

use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use jsonwebtoken::errors::ErrorKind;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use super::random::constant_time_eq;
use crate::config::oidc::{GROUP_MAX, OidcSettings};
use crate::domain::user::{EMAIL_MAX, NAME_MAX};

/// How long a discovery document is used before it is fetched again.
const DISCOVERY_TTL: Duration = Duration::from_secs(3600);
/// How long a key set is used before it is fetched again.
const KEYS_TTL: Duration = Duration::from_secs(3600);
/// The least time between two fetches of the key set that a key id it did
/// not hold caused: tokens naming made-up keys do not make this side hammer
/// the provider.
const KEYS_REFETCH_GAP: Duration = Duration::from_secs(60);
/// After a failed refresh, how long the kept document or key set is used
/// before the provider is asked again: while it is slow or down, sign-ins
/// do not each wait for its timeout in turn.
const RETRY_GAP: Duration = Duration::from_secs(60);
/// After a failed first fetch (nothing kept), how long the failure is the
/// answer before the provider is asked again.
const FAILURE_GAP: Duration = Duration::from_secs(5);
/// The largest discovery document, key set or userinfo answer read.
const DOCUMENT_MAX_BYTES: usize = 256 * 1024;
/// The largest token endpoint answer read.
const TOKEN_ANSWER_MAX_BYTES: usize = 64 * 1024;
/// The longest ID token looked at.
const ID_TOKEN_MAX_BYTES: usize = 16 * 1024;
/// Clock skew allowed on `exp`, `nbf` and `iat`, in seconds.
pub const LEEWAY_SECS: i64 = 60;
/// How old an ID token may be when it arrives, in seconds.
pub const ID_TOKEN_MAX_AGE_SECS: i64 = 600;
/// Most keys of a key set looked at, most groups of a person kept.
const MAX_KEYS: usize = 100;
const MAX_GROUPS: usize = 1000;
/// Longest subject accepted.
const SUBJECT_MAX: usize = 255;

/// Why talking to the provider did not work.
#[derive(Debug, Clone, thiserror::Error)]
pub enum OidcError {
    /// The provider could not be reached, or answered something unusable.
    #[error("the identity provider is unavailable: {0}")]
    Unavailable(String),
    /// What the provider (or the browser) presented does not verify.
    #[error("{0}")]
    Invalid(String),
}

fn invalid(message: impl Into<String>) -> OidcError {
    OidcError::Invalid(message.into())
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

    /// Whether the client authenticates at the token endpoint with HTTP Basic
    /// (`client_secret_basic`, the default), else in the form
    /// (`client_secret_post`).
    fn basic_auth(&self) -> bool {
        self.token_endpoint_auth_methods_supported
            .as_ref()
            .is_none_or(|methods| {
                methods.iter().any(|m| m == "client_secret_basic")
                    || !methods.iter().any(|m| m == "client_secret_post")
            })
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

/// One key of the provider's key set this side may verify with.
#[derive(Clone)]
struct SigningKey {
    kid: Option<String>,
    alg: Algorithm,
    key: DecodingKey,
}

/// One entry of a JWK set (RFC 7517), as far as it is read.
#[derive(Deserialize)]
struct Jwk {
    kty: String,
    #[serde(default)]
    kid: Option<String>,
    #[serde(default, rename = "use")]
    usage: Option<String>,
    #[serde(default)]
    alg: Option<String>,
    #[serde(default)]
    n: Option<String>,
    #[serde(default)]
    e: Option<String>,
    #[serde(default)]
    crv: Option<String>,
    #[serde(default)]
    x: Option<String>,
    #[serde(default)]
    y: Option<String>,
}

/// The RS256 and ES256 (P-256) signing keys of a key set. Everything else is
/// left out: encryption keys, other algorithms, and above all symmetric
/// (`oct`) keys, so that no HMAC key ever comes from a key set.
fn signing_keys(set: &Value) -> Vec<SigningKey> {
    let entries = set["keys"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    entries
        .iter()
        .take(MAX_KEYS)
        .filter_map(|entry| serde_json::from_value::<Jwk>(entry.clone()).ok())
        .filter(|jwk| jwk.usage.as_deref().is_none_or(|u| u == "sig"))
        .filter_map(|jwk| {
            let (alg, key) = match (jwk.kty.as_str(), jwk.alg.as_deref()) {
                ("RSA", None | Some("RS256")) => (
                    Algorithm::RS256,
                    DecodingKey::from_rsa_components(jwk.n.as_deref()?, jwk.e.as_deref()?).ok()?,
                ),
                ("EC", None | Some("ES256")) if jwk.crv.as_deref() == Some("P-256") => (
                    Algorithm::ES256,
                    DecodingKey::from_ec_components(jwk.x.as_deref()?, jwk.y.as_deref()?).ok()?,
                ),
                _ => return None,
            };
            Some(SigningKey {
                kid: jwk.kid,
                alg,
                key,
            })
        })
        .collect()
}

/// How many keys of a key set a sign-in would verify with (`nexc doctor`).
pub fn usable_signing_keys(set: &Value) -> usize {
    signing_keys(set).len()
}

/// A request that got no answer, with every cause under it: reqwest's own
/// words ("error sending request") say nothing of why, and repeat the URL.
fn unreachable(url: &str, err: reqwest::Error) -> OidcError {
    let mut why: Vec<String> = Vec::new();
    let mut cause = std::error::Error::source(&err);
    while let Some(err) = cause {
        let text = err.to_string();
        if why.last() != Some(&text) {
            why.push(text);
        }
        cause = err.source();
    }
    let what = if err.is_timeout() {
        "no answer in time".to_owned()
    } else {
        err.without_url().to_string()
    };
    why.insert(0, what);
    OidcError::Unavailable(format!("{url}: {}", why.join(": ")))
}

/// The provider's keys as last fetched.
#[derive(Default)]
struct KeyCache {
    keys: Vec<SigningKey>,
    fetched_at: Option<Instant>,
    /// When a key id the set did not hold last made it fetched again.
    refetched_at: Option<Instant>,
    /// No fetch for being old before then: the last one failed.
    retry_at: Option<Instant>,
}

impl KeyCache {
    /// Whether the set is due to be fetched: never fetched or older than
    /// [`KEYS_TTL`], and no fetch failed lately.
    fn due(&self, now: Instant) -> bool {
        let old = self
            .fetched_at
            .is_none_or(|at| now.saturating_duration_since(at) >= KEYS_TTL);
        old && self.retry_at.is_none_or(|at| now >= at)
    }

    /// Notes a failed fetch: the next waits [`RETRY_GAP`] while keys are
    /// kept, [`FAILURE_GAP`] while there are none.
    fn failed(&mut self, now: Instant) {
        let gap = if self.keys.is_empty() {
            FAILURE_GAP
        } else {
            RETRY_GAP
        };
        self.retry_at = Some(now + gap);
    }

    fn matching(&self, alg: Algorithm, kid: Option<&str>) -> Vec<DecodingKey> {
        self.keys
            .iter()
            .filter(|k| k.alg == alg && kid.is_none_or(|kid| k.kid.as_deref() == Some(kid)))
            .map(|k| k.key.clone())
            .collect()
    }
}

/// What the provider says about the person.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub name: Option<String>,
    pub groups: Option<Vec<String>>,
}

impl Profile {
    /// Reads the profile claims; `groups_claim` may be a dotted path
    /// (`realm_access.roles`).
    pub fn from_claims(claims: &Map<String, Value>, groups_claim: &str) -> Profile {
        let text = |name: &str| {
            claims
                .get(name)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        };
        let email = text("email")
            .filter(|e| e.len() <= EMAIL_MAX)
            .map(str::to_owned);
        let email_verified = match claims.get("email_verified") {
            Some(Value::Bool(b)) => Some(*b),
            Some(Value::String(s)) if s.eq_ignore_ascii_case("true") => Some(true),
            Some(Value::String(s)) if s.eq_ignore_ascii_case("false") => Some(false),
            _ => None,
        };
        let name = text("name")
            .map(str::to_owned)
            .or_else(|| {
                let full = [text("given_name"), text("family_name")]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" ");
                (!full.is_empty()).then_some(full)
            })
            .or_else(|| text("preferred_username").map(str::to_owned))
            .map(|n| {
                n.chars()
                    .filter(|c| !c.is_control())
                    .take(NAME_MAX)
                    .collect()
            });
        let groups_value = claims.get(groups_claim).or_else(|| {
            let mut parts = groups_claim.split('.');
            let first = claims.get(parts.next()?)?;
            parts.try_fold(first, |value, part| value.get(part))
        });
        let groups = match groups_value {
            Some(Value::Array(items)) => Some(
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|g| !g.is_empty() && g.chars().count() <= GROUP_MAX)
                    .take(MAX_GROUPS)
                    .map(str::to_owned)
                    .collect(),
            ),
            Some(Value::String(group)) if group.chars().count() <= GROUP_MAX => {
                Some(vec![group.clone()])
            }
            _ => None,
        };
        Profile {
            email,
            email_verified,
            name,
            groups,
        }
    }

    /// Whether every claim the sign-in reads is here.
    pub fn is_complete(&self) -> bool {
        self.email.is_some()
            && self.email_verified.is_some()
            && self.name.is_some()
            && self.groups.is_some()
    }

    /// Fills what is missing from `other` (the userinfo answer). The address
    /// and whether it is verified are one statement: without an address both
    /// come from `other`; with one, `other` may only say whether that same
    /// address is verified.
    pub fn fill_from(&mut self, other: Profile) {
        match &self.email {
            None => {
                self.email = other.email;
                self.email_verified = other.email_verified;
            }
            Some(mine) => {
                let same = other
                    .email
                    .as_deref()
                    .is_some_and(|theirs| theirs.eq_ignore_ascii_case(mine));
                if self.email_verified.is_none() && same {
                    self.email_verified = other.email_verified;
                }
            }
        }
        self.name = self.name.take().or(other.name);
        self.groups = self.groups.take().or(other.groups);
    }
}

/// A verified ID token: who the person is at the provider.
#[derive(Debug, Clone)]
pub struct Identity {
    pub subject: String,
    pub profile: Profile,
}

/// What the token endpoint answered.
#[derive(Debug)]
pub struct Tokens {
    pub id_token: String,
    pub access_token: Option<String>,
}

/// The PKCE challenge of a verifier (RFC 7636, S256).
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// `application/x-www-form-urlencoded` of `pairs`.
fn form(pairs: &[(&str, &str)]) -> String {
    let mut url = reqwest::Url::parse("http://form.invalid/").expect("a constant URL");
    url.query_pairs_mut().extend_pairs(pairs);
    url.query().unwrap_or_default().to_owned()
}

/// A number claim as whole seconds.
fn seconds(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        _ => None,
    }
}

/// The checks `jsonwebtoken` does not make: `azp`, `iat`, the nonce and the
/// subject. `now` is in seconds.
fn check_claims(
    claims: &Map<String, Value>,
    client_id: &str,
    nonce: &str,
    now: i64,
) -> Result<String, OidcError> {
    let subject = claims
        .get("sub")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= SUBJECT_MAX)
        .ok_or_else(|| invalid("the ID token names no usable subject"))?;
    let azp = claims.get("azp").and_then(Value::as_str);
    let several = claims
        .get("aud")
        .and_then(Value::as_array)
        .is_some_and(|aud| aud.len() > 1);
    match azp {
        Some(azp) if azp != client_id => {
            return Err(invalid("the ID token was issued to another client (azp)"));
        }
        None if several => {
            return Err(invalid(
                "the ID token names several audiences and no authorized party (azp)",
            ));
        }
        _ => {}
    }
    let iat = seconds(claims.get("iat")).ok_or_else(|| invalid("the ID token has no iat"))?;
    if iat > now + LEEWAY_SECS {
        return Err(invalid("the ID token was issued in the future"));
    }
    if iat < now - ID_TOKEN_MAX_AGE_SECS - LEEWAY_SECS {
        return Err(invalid("the ID token was issued too long ago"));
    }
    let sent = claims
        .get("nonce")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !constant_time_eq(sent.as_bytes(), nonce.as_bytes()) {
        return Err(invalid("the ID token's nonce is not the one sent"));
    }
    Ok(subject.to_owned())
}

/// Says in words why `jsonwebtoken` refused a token.
fn describe(err: Option<jsonwebtoken::errors::Error>) -> OidcError {
    let Some(err) = err else {
        return invalid("the ID token was signed with a key the provider does not publish");
    };
    invalid(match err.kind() {
        ErrorKind::InvalidSignature => "the ID token's signature does not verify".to_owned(),
        ErrorKind::ExpiredSignature => "the ID token has expired".to_owned(),
        ErrorKind::ImmatureSignature => "the ID token is not valid yet (nbf)".to_owned(),
        ErrorKind::InvalidIssuer => "the ID token was issued by another issuer".to_owned(),
        ErrorKind::InvalidAudience => "the ID token was issued for another client".to_owned(),
        ErrorKind::MissingRequiredClaim(claim) => format!("the ID token has no {claim}"),
        ErrorKind::InvalidAlgorithm => "the ID token's algorithm is not accepted".to_owned(),
        other => format!("the ID token does not verify ({other:?})"),
    })
}

/// The discovery document as last fetched, and when to ask for it again.
#[derive(Default)]
struct DiscoveryCache {
    doc: Option<Discovery>,
    /// No fetch before then: the kept document is fresh, or a fetch failed lately.
    next_fetch: Option<Instant>,
    /// Why the last fetch failed, while no document is kept.
    failure: Option<OidcError>,
}

impl DiscoveryCache {
    /// The answer without asking the provider, while it is not time to ask.
    fn kept(&self, now: Instant) -> Option<Result<Discovery, OidcError>> {
        if self.next_fetch.is_none_or(|at| now >= at) {
            return None;
        }
        match (&self.doc, &self.failure) {
            (Some(doc), _) => Some(Ok(doc.clone())),
            (None, Some(err)) => Some(Err(err.clone())),
            (None, None) => None,
        }
    }

    /// Keeps what a fetch came to and answers with it: a failure answers
    /// with the kept document when there is one, asked for again after
    /// [`RETRY_GAP`], else is the answer for [`FAILURE_GAP`].
    fn record(
        &mut self,
        fetched: Result<Discovery, OidcError>,
        now: Instant,
    ) -> Result<Discovery, OidcError> {
        match fetched {
            Ok(doc) => {
                self.doc = Some(doc.clone());
                self.failure = None;
                self.next_fetch = Some(now + DISCOVERY_TTL);
                Ok(doc)
            }
            Err(err) => match &self.doc {
                Some(doc) => {
                    tracing::warn!(error = %err, "OIDC discovery failed; the previous document is used");
                    self.next_fetch = Some(now + RETRY_GAP);
                    Ok(doc.clone())
                }
                None => {
                    self.failure = Some(err.clone());
                    self.next_fetch = Some(now + FAILURE_GAP);
                    Err(err)
                }
            },
        }
    }
}

/// The relying party of one configured provider, shared by every request.
pub struct OidcClient {
    settings: OidcSettings,
    production: bool,
    http: reqwest::Client,
    discovery: Mutex<DiscoveryCache>,
    keys: Mutex<KeyCache>,
}

impl std::fmt::Debug for OidcClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OidcClient")
            .field("issuer", &self.settings.issuer)
            .finish_non_exhaustive()
    }
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
            discovery: Mutex::new(DiscoveryCache::default()),
            keys: Mutex::new(KeyCache::default()),
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
    /// an hour old, else fetched. A failed fetch keeps using the old one for
    /// another minute before asking again; with none kept, the failure is
    /// the answer for five seconds.
    pub async fn discovery(&self) -> Result<Discovery, OidcError> {
        let mut cache = self.discovery.lock().await;
        if let Some(answer) = cache.kept(Instant::now()) {
            return answer;
        }
        let fetched = self.fetch_discovery().await;
        cache.record(fetched, Instant::now())
    }

    async fn fetch_discovery(&self) -> Result<Discovery, OidcError> {
        let value = self.get_json(&self.discovery_url()).await?;
        let doc: Discovery = serde_json::from_value(value).map_err(|err| {
            OidcError::Unavailable(format!("the discovery document is incomplete: {err}"))
        })?;
        doc.check(&self.settings, self.production)?;
        Ok(doc)
    }

    /// The address the browser is sent to: the provider's authorization
    /// endpoint with this request's `state`, `nonce` and PKCE challenge.
    pub fn authorization_url(
        &self,
        discovery: &Discovery,
        state: &str,
        nonce: &str,
        verifier: &str,
    ) -> Result<String, OidcError> {
        let mut url = reqwest::Url::parse(&discovery.authorization_endpoint)
            .map_err(|err| OidcError::Unavailable(format!("authorization endpoint: {err}")))?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.settings.client_id)
            .append_pair("redirect_uri", &self.settings.redirect_url)
            .append_pair("scope", &self.settings.scope())
            .append_pair("state", state)
            .append_pair("nonce", nonce)
            .append_pair("code_challenge", &pkce_challenge(verifier))
            .append_pair("code_challenge_method", "S256");
        Ok(url.into())
    }

    /// Exchanges an authorization code (with its PKCE verifier) at the token
    /// endpoint. A refusal by the provider is [`OidcError::Invalid`].
    pub async fn exchange(
        &self,
        discovery: &Discovery,
        code: &str,
        verifier: &str,
    ) -> Result<Tokens, OidcError> {
        let settings = &self.settings;
        let mut pairs = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", settings.redirect_url.as_str()),
            ("code_verifier", verifier),
        ];
        let mut request = self
            .http
            .post(&discovery.token_endpoint)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .header(reqwest::header::ACCEPT, "application/json");
        if discovery.basic_auth() {
            // RFC 6749 §2.3.1: both are form-encoded before they are joined.
            let id = form(&[("", &settings.client_id)]);
            let secret = form(&[("", settings.client_secret.expose())]);
            let credentials = format!("{}:{}", &id[1..], &secret[1..]);
            request = request.header(
                reqwest::header::AUTHORIZATION,
                format!("Basic {}", STANDARD.encode(credentials)),
            );
        } else {
            pairs.push(("client_id", settings.client_id.as_str()));
            pairs.push(("client_secret", settings.client_secret.expose().as_str()));
        }
        let url = &discovery.token_endpoint;
        let response = request
            .body(form(&pairs))
            .send()
            .await
            .map_err(|err| unreachable(url, err))?;
        let status = response.status();
        let body = read_capped(response, TOKEN_ANSWER_MAX_BYTES)
            .await
            .map_err(|err| OidcError::Unavailable(format!("{url}: {err}")))?;
        let answer: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        if status.is_client_error() {
            let error = answer["error"].as_str().unwrap_or("no reason given");
            return Err(invalid(format!(
                "the provider refused the code ({status}: {error})"
            )));
        }
        if !status.is_success() {
            return Err(OidcError::Unavailable(format!("{url} answered {status}")));
        }
        if let Some(kind) = answer["token_type"].as_str()
            && !kind.eq_ignore_ascii_case("bearer")
        {
            return Err(invalid(format!(
                "the provider answered a `{kind}` token, not a bearer token"
            )));
        }
        let id_token = answer["id_token"]
            .as_str()
            .ok_or_else(|| invalid("the provider answered without an ID token"))?
            .to_owned();
        Ok(Tokens {
            id_token,
            access_token: answer["access_token"].as_str().map(str::to_owned),
        })
    }

    /// Verifies an ID token (see the module documentation) and reads who it
    /// is about.
    pub async fn verify_id_token(
        &self,
        discovery: &Discovery,
        token: &str,
        nonce: &str,
    ) -> Result<Identity, OidcError> {
        if token.len() > ID_TOKEN_MAX_BYTES {
            return Err(invalid("the ID token is too long"));
        }
        // `none` is no algorithm `jsonwebtoken` knows: such a header does not even parse.
        let header = jsonwebtoken::decode_header(token)
            .map_err(|_| invalid("the ID token is malformed or its algorithm is not accepted"))?;
        let alg = match header.alg {
            Algorithm::RS256 | Algorithm::ES256 => header.alg,
            other => {
                return Err(invalid(format!(
                    "the ID token is signed with {other:?}; only RS256 and ES256 are accepted"
                )));
            }
        };
        let keys = self
            .keys_for(&discovery.jwks_uri, alg, header.kid.as_deref())
            .await?;
        let mut validation = Validation::new(alg);
        validation.set_issuer(&[&self.settings.issuer]);
        validation.set_audience(&[&self.settings.client_id]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.leeway = LEEWAY_SECS.unsigned_abs();
        validation.validate_nbf = true;
        let mut last = None;
        for key in &keys {
            match jsonwebtoken::decode::<Map<String, Value>>(token, key, &validation) {
                Ok(data) => {
                    let now = chrono::Utc::now().timestamp();
                    let subject = check_claims(&data.claims, &self.settings.client_id, nonce, now)?;
                    return Ok(Identity {
                        subject,
                        profile: Profile::from_claims(&data.claims, &self.settings.groups_claim),
                    });
                }
                Err(err) => last = Some(err),
            }
        }
        Err(describe(last))
    }

    /// The keys an ID token signed with `alg` (and naming `kid`) may verify
    /// with. A key set older than an hour is fetched again; a key id it does
    /// not hold makes it fetched again at once, unless that happened less
    /// than a minute ago.
    async fn keys_for(
        &self,
        jwks_uri: &str,
        alg: Algorithm,
        kid: Option<&str>,
    ) -> Result<Vec<DecodingKey>, OidcError> {
        let mut cache = self.keys.lock().await;
        if cache.due(Instant::now()) {
            match self.fetch_keys(jwks_uri).await {
                Ok(keys) => {
                    cache.keys = keys;
                    cache.fetched_at = Some(Instant::now());
                    cache.retry_at = None;
                }
                Err(err) => {
                    let had_keys = !cache.keys.is_empty();
                    cache.failed(Instant::now());
                    if !had_keys {
                        return Err(err);
                    }
                    tracing::warn!(error = %err, "OIDC key set fetch failed; the previous keys are used");
                }
            }
        } else if cache.keys.is_empty() {
            return Err(OidcError::Unavailable(
                "the provider's key set could not be fetched; it is asked for again in a moment"
                    .into(),
            ));
        }
        let found = cache.matching(alg, kid);
        if !found.is_empty() {
            return Ok(found);
        }
        if cache
            .refetched_at
            .is_some_and(|at| at.elapsed() < KEYS_REFETCH_GAP)
        {
            return Err(invalid(
                "the ID token was signed with a key the provider does not publish",
            ));
        }
        // The provider may have rotated its keys since they were fetched.
        cache.refetched_at = Some(Instant::now());
        cache.keys = self.fetch_keys(jwks_uri).await?;
        cache.fetched_at = Some(Instant::now());
        let found = cache.matching(alg, kid);
        if found.is_empty() {
            return Err(invalid(
                "the ID token was signed with a key the provider does not publish",
            ));
        }
        Ok(found)
    }

    async fn fetch_keys(&self, jwks_uri: &str) -> Result<Vec<SigningKey>, OidcError> {
        let set = self.get_json(jwks_uri).await?;
        let keys = signing_keys(&set);
        tracing::debug!(keys = keys.len(), "OIDC key set fetched");
        Ok(keys)
    }

    /// What the userinfo endpoint says about the person the access token is
    /// for; nothing when the provider has no such endpoint or answers a
    /// signed JWT (only JSON is read). An answer about another subject is
    /// refused.
    pub async fn userinfo(
        &self,
        discovery: &Discovery,
        access_token: &str,
        subject: &str,
    ) -> Result<Profile, OidcError> {
        let Some(url) = &discovery.userinfo_endpoint else {
            return Ok(Profile::default());
        };
        let response = self
            .http
            .get(url)
            .bearer_auth(access_token)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|err| unreachable(url, err))?;
        let status = response.status();
        if !status.is_success() {
            return Err(OidcError::Unavailable(format!("{url} answered {status}")));
        }
        let signed = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.starts_with("application/jwt"));
        if signed {
            tracing::warn!("the OIDC userinfo endpoint answers signed JWTs, which are not read");
            return Ok(Profile::default());
        }
        let body = read_capped(response, DOCUMENT_MAX_BYTES)
            .await
            .map_err(|err| OidcError::Unavailable(format!("{url}: {err}")))?;
        let claims: Map<String, Value> = serde_json::from_slice(&body)
            .map_err(|err| OidcError::Unavailable(format!("{url} did not answer JSON: {err}")))?;
        if claims.get("sub").and_then(Value::as_str) != Some(subject) {
            return Err(invalid("the userinfo answer is about someone else"));
        }
        Ok(Profile::from_claims(&claims, &self.settings.groups_claim))
    }

    /// GETs a JSON document of at most [`DOCUMENT_MAX_BYTES`].
    pub(crate) async fn get_json(&self, url: &str) -> Result<Value, OidcError> {
        let response = self
            .http
            .get(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|err| unreachable(url, err))?;
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
    use serde_json::json;

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

    #[test]
    fn a_failed_refresh_is_not_tried_again_by_every_sign_in() {
        let s = settings();
        let t0 = Instant::now();
        let down = || Err(OidcError::Unavailable("connection refused".into()));
        let mut cache = DiscoveryCache::default();
        assert!(cache.kept(t0).is_none(), "nothing kept: fetch");
        // The provider is down at the first fetch: the failure is the answer for a moment.
        assert!(cache.record(down(), t0).is_err());
        assert!(matches!(
            cache.kept(t0 + Duration::from_secs(1)),
            Some(Err(_))
        ));
        assert!(cache.kept(t0 + FAILURE_GAP).is_none());
        // Fetched, the document is kept for an hour.
        assert!(cache.record(Ok(doc(&s.issuer)), t0).is_ok());
        assert!(matches!(
            cache.kept(t0 + Duration::from_secs(1800)),
            Some(Ok(_))
        ));
        let t1 = t0 + DISCOVERY_TTL;
        assert!(cache.kept(t1).is_none(), "an hour old: fetch again");
        // A failed refresh answers with the kept document, and nobody asks again for a minute.
        assert_eq!(cache.record(down(), t1).unwrap().issuer, s.issuer);
        assert!(matches!(
            cache.kept(t1 + Duration::from_secs(30)),
            Some(Ok(_))
        ));
        assert!(cache.kept(t1 + RETRY_GAP).is_none());

        let mut keys = KeyCache::default();
        assert!(keys.due(t0));
        keys.failed(t0);
        assert!(!keys.due(t0 + Duration::from_secs(1)));
        assert!(keys.due(t0 + FAILURE_GAP));
        keys.keys = signing_keys(&json!({"keys": [{
            "kty": "RSA", "alg": "RS256", "n": URL_SAFE_NO_PAD.encode([0xc3u8; 256]), "e": "AQAB",
        }]}));
        keys.fetched_at = Some(t0);
        keys.retry_at = None;
        assert!(!keys.due(t0 + Duration::from_secs(1800)));
        assert!(keys.due(t1));
        keys.failed(t1);
        assert!(
            !keys.due(t1 + Duration::from_secs(30)),
            "kept keys wait a minute"
        );
        assert!(keys.due(t1 + RETRY_GAP));
    }

    #[tokio::test]
    async fn an_unreachable_provider_is_named_once_with_the_reason() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/.well-known/openid-configuration",
            listener.local_addr().unwrap()
        );
        drop(listener);
        let client = OidcClient::new(settings(), false).unwrap();
        let err = client.get_json(&url).await.unwrap_err().to_string();
        assert_eq!(err.matches(url.as_str()).count(), 1, "{err}");
        assert!(err.to_lowercase().contains("refused"), "the reason: {err}");
    }

    #[test]
    fn client_authentication_follows_the_provider() {
        let s = settings();
        let mut d = doc(&s.issuer);
        assert!(d.basic_auth(), "basic is the default");
        d.token_endpoint_auth_methods_supported = Some(vec!["client_secret_post".into()]);
        assert!(!d.basic_auth());
        d.token_endpoint_auth_methods_supported = Some(vec![
            "client_secret_post".into(),
            "client_secret_basic".into(),
        ]);
        assert!(d.basic_auth());
        assert_eq!(form(&[("a", "x y&z"), ("b", "é")]), "a=x+y%26z&b=%C3%A9");
    }

    #[test]
    fn the_pkce_challenge_is_s256() {
        // BASE64URL(SHA256(verifier)) without padding (RFC 7636 §4.2), as Python's hashlib and
        // base64 compute it for the same verifier.
        let challenge = pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gXFkfjWkM");
        assert_eq!(challenge, "DpKa7g9nkZCHB2fplXxAMzx3zOjC-ah0H2CVK08OGnM");
        let fresh = pkce_challenge(&crate::security::random::random_token());
        assert_eq!(fresh.len(), 43);
        assert!(
            fresh
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );
    }

    #[test]
    fn only_signing_keys_of_the_two_algorithms_are_kept() {
        let n = URL_SAFE_NO_PAD.encode([0xc3u8; 256]);
        let set = json!({"keys": [
            {"kty": "RSA", "kid": "r1", "use": "sig", "alg": "RS256", "n": n, "e": "AQAB"},
            {"kty": "RSA", "kid": "enc", "use": "enc", "n": n, "e": "AQAB"},
            {"kty": "RSA", "kid": "ps", "alg": "PS256", "n": n, "e": "AQAB"},
            {"kty": "oct", "kid": "hmac", "k": "c2VjcmV0"},
            {"kty": "EC", "kid": "p384", "crv": "P-384", "x": "AA", "y": "AA"},
            {"kty": "RSA", "kid": "broken"},
            "not a key",
        ]});
        let keys = signing_keys(&set);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].kid.as_deref(), Some("r1"));
        assert_eq!(keys[0].alg, Algorithm::RS256);
        assert!(signing_keys(&json!({"keys": "nope"})).is_empty());
    }

    fn claims(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn azp_iat_and_the_nonce_are_checked() {
        let now = 1_800_000_000;
        let good = json!({"sub": "u1", "aud": "nexc", "iat": now, "nonce": "n0nce"});
        assert_eq!(
            check_claims(&claims(good.clone()), "nexc", "n0nce", now).unwrap(),
            "u1"
        );
        let mut several = good.clone();
        several["aud"] = json!(["nexc", "other"]);
        assert!(check_claims(&claims(several.clone()), "nexc", "n0nce", now).is_err());
        several["azp"] = json!("nexc");
        assert!(check_claims(&claims(several.clone()), "nexc", "n0nce", now).is_ok());
        several["azp"] = json!("other");
        assert!(check_claims(&claims(several), "nexc", "n0nce", now).is_err());
        for (field, value) in [
            ("iat", json!(now + LEEWAY_SECS + 5)),
            ("iat", json!(now - ID_TOKEN_MAX_AGE_SECS - LEEWAY_SECS - 5)),
            ("iat", json!("yesterday")),
            ("nonce", json!("other")),
            ("nonce", json!(null)),
            ("sub", json!("")),
            ("sub", json!("x".repeat(300))),
        ] {
            let mut bad = good.clone();
            bad[field] = value.clone();
            assert!(
                check_claims(&claims(bad), "nexc", "n0nce", now).is_err(),
                "{field} = {value}"
            );
        }
        let mut skewed = good;
        skewed["iat"] = json!(now + 30);
        assert!(
            check_claims(&claims(skewed), "nexc", "n0nce", now).is_ok(),
            "within the leeway"
        );
    }

    #[test]
    fn the_profile_is_read_leniently() {
        let p = Profile::from_claims(
            &claims(json!({
                "email": " Ada@Example.com ", "email_verified": "true",
                "given_name": "Ada", "family_name": "Lovelace",
                "realm_access": {"roles": ["eng", 7, "", "ops"]},
            })),
            "realm_access.roles",
        );
        assert_eq!(p.email.as_deref(), Some("Ada@Example.com"));
        assert_eq!(p.email_verified, Some(true));
        assert_eq!(p.name.as_deref(), Some("Ada Lovelace"));
        assert_eq!(p.groups, Some(vec!["eng".to_owned(), "ops".to_owned()]));
        assert!(p.is_complete());
        let mut sparse = Profile::from_claims(
            &claims(json!({"preferred_username": "ada", "groups": "eng"})),
            "groups",
        );
        assert_eq!(sparse.name.as_deref(), Some("ada"));
        assert_eq!(sparse.groups, Some(vec!["eng".to_owned()]));
        assert!(!sparse.is_complete());
        sparse.fill_from(Profile {
            email: Some("ada@example.com".into()),
            email_verified: Some(false),
            name: Some("Other".into()),
            groups: Some(Vec::new()),
        });
        assert_eq!(
            sparse.name.as_deref(),
            Some("ada"),
            "what the ID token said wins"
        );
        assert_eq!(sparse.email_verified, Some(false));
        assert_eq!(sparse.groups, Some(vec!["eng".to_owned()]));

        // Whether an address is verified is never taken from an answer about another address.
        let unvouched = || Profile {
            email: Some("ada@example.com".into()),
            ..Profile::default()
        };
        let mut p = unvouched();
        p.fill_from(Profile {
            email: Some("someone@else.example".into()),
            email_verified: Some(true),
            ..Profile::default()
        });
        assert_eq!(p.email.as_deref(), Some("ada@example.com"));
        assert_eq!(p.email_verified, None);
        let mut p = unvouched();
        p.fill_from(Profile {
            email_verified: Some(true),
            ..Profile::default()
        });
        assert_eq!(p.email_verified, None, "an answer naming no address");
        let mut p = unvouched();
        p.fill_from(Profile {
            email: Some("Ada@Example.com".into()),
            email_verified: Some(true),
            ..Profile::default()
        });
        assert_eq!(p.email_verified, Some(true), "the same address");
        // With no address of its own, both come from the answer, together.
        let mut p = Profile {
            email_verified: Some(true),
            ..Profile::default()
        };
        p.fill_from(Profile {
            email: Some("ada@example.com".into()),
            ..Profile::default()
        });
        assert_eq!(
            (p.email.as_deref(), p.email_verified),
            (Some("ada@example.com"), None)
        );
    }
}
