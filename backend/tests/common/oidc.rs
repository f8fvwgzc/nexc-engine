//! An OpenID Connect provider in process, for the single sign-on tests: axum
//! serving discovery, a key set (two RSA keys and one P-256 key made at
//! start, of which the test decides which are published), a token endpoint
//! that checks the client, the redirect URI and the PKCE verifier, and
//! userinfo. The authorization step is the test's own: [`Stub::authorize`]
//! reads the authorization request the backend sent the browser to and
//! answers as the provider would, with what the test said to assert next.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use aws_lc_rs::encoding::AsDer;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use jsonwebtoken::jwk::Jwk;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::{Resp, TestApp};

/// The client the backend is registered as. The secret holds characters that
/// HTTP Basic needs form-encoded.
pub const CLIENT_ID: &str = "nexc-test";
pub const CLIENT_SECRET: &str = "s3cret:with+chars&more";
/// The redirect URI the backend derives from the test settings' public URL.
pub const REDIRECT_URI: &str = "http://localhost:8080/api/v1/auth/oidc/callback";

/// How the next ID token is signed.
#[derive(Debug, Clone)]
pub enum Signing {
    /// With the key of this id, naming `kid` in the header.
    Key { key: String, kid: String },
    /// `alg: none`, no signature.
    None,
    /// HS256 with this secret (the client secret, in the classic attack).
    Hs256(String),
}

/// What the provider asserts at the next code exchange.
#[derive(Debug, Clone)]
pub struct Assertion {
    pub sub: String,
    /// Claims of the ID token on top of iss, sub, aud, exp, iat and nonce.
    pub claims: Map<String, Value>,
    /// Claims only the userinfo endpoint answers.
    pub userinfo: Map<String, Value>,
    /// Replaces claims of the ID token after everything else (aud, exp, nonce ...).
    pub overrides: Map<String, Value>,
    pub signing: Signing,
}

impl Assertion {
    /// A person with a verified address and a name, signed with `k1`.
    pub fn person(sub: &str, email: &str) -> Self {
        let claims = json!({"email": email, "email_verified": true, "name": "Ada Lovelace"});
        Assertion {
            sub: sub.to_owned(),
            claims: claims.as_object().unwrap().clone(),
            userinfo: Map::new(),
            overrides: Map::new(),
            signing: Signing::Key {
                key: "k1".into(),
                kid: "k1".into(),
            },
        }
    }

    pub fn claim(mut self, name: &str, value: Value) -> Self {
        self.claims.insert(name.to_owned(), value);
        self
    }

    pub fn without(mut self, name: &str) -> Self {
        self.claims.remove(name);
        self
    }

    pub fn groups(self, groups: &[&str]) -> Self {
        self.claim("groups", json!(groups))
    }

    pub fn userinfo(mut self, name: &str, value: Value) -> Self {
        self.userinfo.insert(name.to_owned(), value);
        self
    }

    pub fn override_claim(mut self, name: &str, value: Value) -> Self {
        self.overrides.insert(name.to_owned(), value);
        self
    }

    pub fn signed(mut self, signing: Signing) -> Self {
        self.signing = signing;
        self
    }
}

/// One key the provider holds.
struct Key {
    alg: Algorithm,
    encoding: EncodingKey,
    jwk: Value,
}

/// A code the provider handed out and has not taken back yet.
struct Grant {
    challenge: String,
    nonce: String,
    redirect_uri: String,
    assertion: Assertion,
}

struct Inner {
    issuer: String,
    keys: HashMap<String, Key>,
    published: Vec<String>,
    jwks_fetches: usize,
    next: Option<Assertion>,
    codes: HashMap<String, Grant>,
    access_tokens: HashMap<String, Value>,
    /// Number of codes exchanged successfully.
    exchanges: usize,
}

/// The running provider.
#[derive(Clone)]
pub struct Stub {
    pub issuer: String,
    inner: Arc<Mutex<Inner>>,
}

/// PKCS#1 `RSAPrivateKey` out of a PKCS#8 `PrivateKeyInfo` (what `jsonwebtoken`
/// reads from DER): SEQUENCE { version, algorithm, OCTET STRING key }.
fn pkcs1_of(pkcs8: &[u8]) -> Vec<u8> {
    fn tlv(input: &[u8], tag: u8) -> (&[u8], &[u8]) {
        assert_eq!(input[0], tag, "DER tag");
        let (len, head) = match input[1] {
            n if n < 0x80 => (usize::from(n), 2),
            0x81 => (usize::from(input[2]), 3),
            0x82 => (usize::from(u16::from_be_bytes([input[2], input[3]])), 4),
            other => panic!("DER length form {other:#x}"),
        };
        (&input[head..head + len], &input[head + len..])
    }
    let (body, _) = tlv(pkcs8, 0x30);
    let (_, rest) = tlv(body, 0x02);
    let (_, rest) = tlv(rest, 0x30);
    let (key, _) = tlv(rest, 0x04);
    key.to_vec()
}

fn rsa_key(kid: &str) -> Key {
    let pair = aws_lc_rs::rsa::KeyPair::generate(aws_lc_rs::rsa::KeySize::Rsa2048).unwrap();
    let pkcs8: aws_lc_rs::encoding::Pkcs8V1Der = pair.as_der().unwrap();
    let encoding = EncodingKey::from_rsa_der(&pkcs1_of(pkcs8.as_ref()));
    key(kid, Algorithm::RS256, encoding)
}

fn ec_key(kid: &str) -> Key {
    let pair = aws_lc_rs::signature::EcdsaKeyPair::generate(
        &aws_lc_rs::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
    )
    .unwrap();
    let encoding = EncodingKey::from_ec_der(pair.to_pkcs8v1().unwrap().as_ref());
    key(kid, Algorithm::ES256, encoding)
}

fn key(kid: &str, alg: Algorithm, encoding: EncodingKey) -> Key {
    let mut jwk = serde_json::to_value(Jwk::from_encoding_key(&encoding, alg).unwrap()).unwrap();
    jwk["kid"] = json!(kid);
    jwk["use"] = json!("sig");
    Key { alg, encoding, jwk }
}

/// Reads `application/x-www-form-urlencoded` (or a query) into pairs.
fn pairs(raw: &str) -> HashMap<String, String> {
    reqwest::Url::parse(&format!("http://x.invalid/?{raw}"))
        .unwrap()
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

async fn discovery(State(stub): State<Stub>) -> Json<Value> {
    let issuer = &stub.issuer;
    Json(json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/authorize"),
        "token_endpoint": format!("{issuer}/token"),
        "userinfo_endpoint": format!("{issuer}/userinfo"),
        "jwks_uri": format!("{issuer}/jwks"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256", "ES256"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["client_secret_basic"],
    }))
}

async fn jwks(State(stub): State<Stub>) -> Json<Value> {
    let mut inner = stub.inner.lock().unwrap();
    inner.jwks_fetches += 1;
    let keys: Vec<Value> = inner
        .published
        .iter()
        .map(|kid| inner.keys[kid].jwk.clone())
        .collect();
    Json(json!({ "keys": keys }))
}

fn refuse(error: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response()
}

async fn token(
    State(stub): State<Stub>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    // The client authenticates with HTTP Basic, both halves form-encoded (RFC 6749 §2.3.1).
    let basic = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .and_then(|v| STANDARD.decode(v).ok())
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|v| {
            let (id, secret) = v.split_once(':')?;
            let decoded = pairs(&format!("id={id}&secret={secret}"));
            Some((decoded["id"].clone(), decoded["secret"].clone()))
        });
    if basic != Some((CLIENT_ID.to_owned(), CLIENT_SECRET.to_owned())) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "invalid_client"})),
        )
            .into_response();
    }
    if form.get("grant_type").map(String::as_str) != Some("authorization_code") {
        return refuse("unsupported_grant_type");
    }
    let mut inner = stub.inner.lock().unwrap();
    let Some(grant) = form.get("code").and_then(|c| inner.codes.remove(c)) else {
        return refuse("invalid_grant");
    };
    if form.get("redirect_uri") != Some(&grant.redirect_uri) {
        return refuse("invalid_grant");
    }
    let verifier = form.get("code_verifier").cloned().unwrap_or_default();
    if URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) != grant.challenge {
        return refuse("invalid_grant");
    }
    let now = chrono::Utc::now().timestamp();
    let assertion = grant.assertion;
    let mut claims = json!({
        "iss": inner.issuer.clone(), "sub": assertion.sub.clone(), "aud": CLIENT_ID,
        "exp": now + 300, "iat": now, "nonce": grant.nonce,
    });
    for (name, value) in assertion.claims.iter().chain(&assertion.overrides) {
        claims[name] = value.clone();
    }
    let id_token = match &assertion.signing {
        Signing::Key { key, kid } => {
            let key = &inner.keys[key];
            let mut header = Header::new(key.alg);
            header.kid = Some(kid.clone());
            jsonwebtoken::encode(&header, &claims, &key.encoding).unwrap()
        }
        Signing::Hs256(secret) => {
            let header = Header::new(Algorithm::HS256);
            jsonwebtoken::encode(
                &header,
                &claims,
                &EncodingKey::from_secret(secret.as_bytes()),
            )
            .unwrap()
        }
        Signing::None => format!(
            "{}.{}.",
            URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        ),
    };
    let access_token = format!("at-{}", uuid::Uuid::now_v7());
    let mut userinfo = json!({"sub": assertion.sub});
    for (name, value) in &assertion.userinfo {
        userinfo[name] = value.clone();
    }
    inner.access_tokens.insert(access_token.clone(), userinfo);
    inner.exchanges += 1;
    Json(json!({
        "access_token": access_token, "token_type": "Bearer",
        "expires_in": 300, "id_token": id_token,
    }))
    .into_response()
}

async fn userinfo(State(stub): State<Stub>, headers: HeaderMap) -> Response {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    match stub.inner.lock().unwrap().access_tokens.get(token) {
        Some(claims) => Json(claims.clone()).into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

impl Stub {
    /// Starts the provider on a port of its own; `k1` is published.
    pub async fn start() -> Stub {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        let keys = HashMap::from([
            ("k1".to_owned(), rsa_key("k1")),
            ("k2".to_owned(), rsa_key("k2")),
            ("e1".to_owned(), ec_key("e1")),
        ]);
        let stub = Stub {
            issuer: issuer.clone(),
            inner: Arc::new(Mutex::new(Inner {
                issuer,
                keys,
                published: vec!["k1".into()],
                jwks_fetches: 0,
                next: None,
                codes: HashMap::new(),
                access_tokens: HashMap::new(),
                exchanges: 0,
            })),
        };
        let app = Router::new()
            .route("/.well-known/openid-configuration", get(discovery))
            .route("/jwks", get(jwks))
            .route("/token", post(token))
            .route("/userinfo", get(userinfo))
            .with_state(stub.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        stub
    }

    /// The `NEXC_OIDC_*` settings that point the backend at this provider.
    pub fn settings(&self) -> Vec<(&'static str, String)> {
        vec![
            ("NEXC_OIDC_ISSUER", self.issuer.clone()),
            ("NEXC_OIDC_CLIENT_ID", CLIENT_ID.to_owned()),
            ("NEXC_OIDC_CLIENT_SECRET", CLIENT_SECRET.to_owned()),
            ("NEXC_OIDC_DISPLAY_NAME", "Acme SSO".to_owned()),
        ]
    }

    /// What the next authorization asserts.
    pub fn expect(&self, assertion: Assertion) {
        self.inner.lock().unwrap().next = Some(assertion);
    }

    /// Publishes exactly these keys.
    pub fn publish(&self, kids: &[&str]) {
        self.inner.lock().unwrap().published = kids.iter().map(|k| (*k).to_owned()).collect();
    }

    /// How often the key set was fetched.
    pub fn jwks_fetches(&self) -> usize {
        self.inner.lock().unwrap().jwks_fetches
    }

    /// How many codes were exchanged.
    pub fn exchanges(&self) -> usize {
        self.inner.lock().unwrap().exchanges
    }

    /// The provider's side of the authorization request the browser was sent
    /// to: checks it and answers with a code for the assertion set by
    /// [`Stub::expect`]. Returns where the provider sends the browser back.
    pub fn authorize(&self, location: &str) -> String {
        let url = reqwest::Url::parse(location).expect("an absolute authorization URL");
        assert_eq!(
            format!("{}{}", url.origin().ascii_serialization(), url.path()),
            format!("{}/authorize", self.issuer),
            "the browser was sent to the provider"
        );
        let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["client_id"], CLIENT_ID);
        assert_eq!(q["redirect_uri"], REDIRECT_URI);
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["code_challenge"].len(), 43, "an S256 challenge");
        assert!(
            q["scope"].split(' ').any(|s| s == "openid"),
            "{}",
            q["scope"]
        );
        assert!(q["state"].len() >= 32 && q["nonce"].len() >= 32);
        let mut inner = self.inner.lock().unwrap();
        let assertion = inner.next.take().expect("the test said what to assert");
        let code = format!("code-{}", uuid::Uuid::now_v7());
        inner.codes.insert(
            code.clone(),
            Grant {
                challenge: q["code_challenge"].clone(),
                nonce: q["nonce"].clone(),
                redirect_uri: q["redirect_uri"].clone(),
                assertion,
            },
        );
        format!("{}?code={code}&state={}", q["redirect_uri"], q["state"])
    }

    /// The provider's answer when the person declines.
    pub fn decline(&self, location: &str) -> String {
        let url = reqwest::Url::parse(location).unwrap();
        let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
        format!(
            "{}?error=access_denied&error_description=The+user+said+no&state={}",
            q["redirect_uri"], q["state"]
        )
    }
}

/// What one browser trip through single sign-on came to.
pub struct Trip {
    /// The callback's answer.
    pub status: StatusCode,
    /// Where the callback sent the browser.
    pub location: String,
    /// The refresh cookie, when signed in.
    pub refresh: Option<String>,
    /// The callback URL (path and query) the provider sent the browser to.
    pub callback: String,
    /// The binding cookie the login set.
    pub binding: String,
}

fn set_cookies(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok().map(str::to_owned))
        .collect()
}

/// The value of the cookie `name` among the `Set-Cookie` headers.
pub fn set_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    set_cookies(headers).iter().find_map(|c| {
        let value = c.strip_prefix(&format!("{name}="))?.split(';').next()?;
        (!value.is_empty()).then(|| value.to_owned())
    })
}

/// An address of its own for each request, so that a test with
/// `NEXC_TRUST_PROXY=true` is not stopped at the `/auth/*` rate limit.
pub fn visitor() -> String {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("10.{}.{}.{}", (n >> 16) & 255, (n >> 8) & 255, n & 255)
}

/// Starts a sign-in: `GET /auth/oidc/login`. Returns the answer, the
/// authorization URL and the binding cookie.
pub async fn begin(app: &TestApp, return_to: Option<&str>) -> (Resp, String, String) {
    let path = match return_to {
        Some(to) => format!("/api/v1/auth/oidc/login?return_to={}", pairs_encode(to)),
        None => "/api/v1/auth/oidc/login".to_owned(),
    };
    let from = visitor();
    let r = app
        .request_with(
            Method::GET,
            &path,
            None,
            None,
            &[("x-forwarded-for", &from)],
        )
        .await;
    let location = r
        .headers
        .get(header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let binding = set_cookie(&r.headers, "nexc_sso").unwrap_or_default();
    (r, location, binding)
}

fn pairs_encode(value: &str) -> String {
    let mut url = reqwest::Url::parse("http://x.invalid/").unwrap();
    url.query_pairs_mut().append_pair("v", value);
    url.query().unwrap()[2..].to_owned()
}

/// The path and query of an absolute URL on the backend.
pub fn local(url: &str) -> String {
    let url = reqwest::Url::parse(url).unwrap();
    match url.query() {
        Some(q) => format!("{}?{q}", url.path()),
        None => url.path().to_owned(),
    }
}

/// Sends the browser back to the callback with the binding cookie.
pub async fn callback(app: &TestApp, callback: &str, binding: &str) -> Resp {
    let cookie = format!("nexc_sso={binding}");
    let from = visitor();
    let headers = [("cookie", cookie.as_str()), ("x-forwarded-for", &from)];
    app.request_with(Method::GET, callback, None, None, &headers)
        .await
}

/// A whole sign-in through the provider, asserting `assertion`.
pub async fn trip(
    app: &TestApp,
    stub: &Stub,
    assertion: Assertion,
    return_to: Option<&str>,
) -> Trip {
    stub.expect(assertion);
    let (start, location, binding) = begin(app, return_to).await;
    assert_eq!(start.status, StatusCode::SEE_OTHER, "{}", start.body);
    let callback_url = local(&stub.authorize(&location));
    let r = callback(app, &callback_url, &binding).await;
    Trip {
        status: r.status,
        location: r
            .headers
            .get(header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned(),
        refresh: set_cookie(&r.headers, "nexc_refresh"),
        callback: callback_url,
        binding,
    }
}

/// The session a refresh cookie stands for: `POST /auth/refresh`.
pub async fn session_of(app: &TestApp, refresh: &str) -> Resp {
    let cookie = format!("nexc_refresh={refresh}");
    let from = visitor();
    app.request_with(
        Method::POST,
        "/api/v1/auth/refresh",
        None,
        None,
        &[
            ("cookie", &cookie),
            ("x-requested-with", "nexc"),
            ("x-forwarded-for", &from),
        ],
    )
    .await
}
