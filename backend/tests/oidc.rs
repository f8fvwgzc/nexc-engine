//! Single sign-on against an OpenID Connect provider in process
//! (`common::oidc`): the whole browser trip through the real router, from
//! `GET /auth/oidc/login` to the provider and back to the callback, then the
//! session it left behind.
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode};
use common::oidc::{
    Assertion, CLIENT_SECRET, Signing, Stub, begin, callback, local, session_of, trip,
};
use common::{PASSWORD, TestApp};
use nexc::domain::user::Role;
use serde_json::{Value, json};
use sqlx::PgPool;

/// The app with single sign-on pointed at `stub`, plus `extra` settings.
async fn app(pool: PgPool, stub: &Stub, extra: &[(&str, &str)]) -> TestApp {
    let settings = stub.settings();
    // Every trip comes from an address of its own (`common::oidc::visitor`).
    let mut overrides = vec![("NEXC_TRUST_PROXY", "true")];
    overrides.extend(settings.iter().map(|(k, v)| (*k, v.as_str())));
    overrides.extend_from_slice(extra);
    TestApp::new(pool, &overrides).await
}

async fn call(
    app: &TestApp,
    method: Method,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let r = app
        .request(method, &format!("/api/v1{path}"), Some(token), body)
        .await;
    (r.status, r.body)
}

/// Signs in through the provider and returns the access token and the user.
async fn signed_in(app: &TestApp, stub: &Stub, assertion: Assertion) -> (String, Value) {
    let t = trip(app, stub, assertion, None).await;
    assert_eq!(t.status, StatusCode::SEE_OTHER);
    assert_eq!(t.location, "/app", "signed in");
    let session = session_of(app, &t.refresh.expect("a refresh cookie")).await;
    assert_eq!(session.status, StatusCode::OK, "{}", session.body);
    (
        session.body["access_token"].as_str().unwrap().to_owned(),
        session.body["user"].clone(),
    )
}

async fn activity(app: &TestApp, token: &str) -> Vec<String> {
    let (_, events) = call(app, Method::GET, "/auth/activity", token, None).await;
    events
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap().to_owned())
        .collect()
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn single_sign_on_is_off_until_an_issuer_is_set(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    for path in [
        "/api/v1/auth/oidc/login",
        "/api/v1/auth/oidc/callback?code=x&state=y",
    ] {
        let r = app.request(Method::GET, path, None, None).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{path}: {}", r.body);
    }
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_login_page_learns_how_to_sign_in(pool: PgPool) {
    let stub = Stub::start().await;
    let providers = |app: &TestApp| {
        let app = app.router.clone();
        async move {
            let request = axum::http::Request::get("/api/v1/auth/providers")
                .body(axum::body::Body::empty())
                .unwrap();
            let response = tower::ServiceExt::oneshot(app, request).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert!(response.headers().contains_key("etag"), "a tagged read");
            let bytes = http_body_util::BodyExt::collect(response.into_body())
                .await
                .unwrap()
                .to_bytes();
            serde_json::from_slice::<Value>(&bytes).unwrap()
        }
    };
    let off = TestApp::new(pool.clone(), &[]).await;
    assert_eq!(
        providers(&off).await,
        json!({"password": true, "signup": true, "sso": []})
    );
    let acme =
        json!([{"id": "oidc", "display_name": "Acme SSO", "login_url": "/api/v1/auth/oidc/login"}]);
    let both = app(pool.clone(), &stub, &[("NEXC_ALLOW_SIGNUP", "false")]).await;
    assert_eq!(
        providers(&both).await,
        json!({"password": true, "signup": false, "sso": acme})
    );
    let only = app(
        pool.clone(),
        &stub,
        &[("NEXC_OIDC_ALLOW_LOCAL_LOGIN", "false")],
    )
    .await;
    assert_eq!(
        providers(&only).await,
        json!({"password": false, "signup": false, "sso": acme})
    );

    // The account page shows how the account signs in.
    let (token, _) = signed_in(&both, &stub, Assertion::person("sub-id", "id@example.com")).await;
    let (status, methods) = call(&both, Method::GET, "/auth/identities", &token, None).await;
    assert_eq!(status, StatusCode::OK, "{methods}");
    assert_eq!(
        methods["password"], false,
        "single sign-on made it: no password"
    );
    let identity = &methods["identities"][0];
    assert_eq!(identity["provider"], "Acme SSO");
    assert_eq!(identity["issuer"], stub.issuer.as_str());
    assert_eq!(identity["email"], "id@example.com");
    assert!(identity["linked_at"].is_string() && identity["last_login_at"].is_string());
    let (password_token, _) = off.register("pw@example.com").await;
    let (_, methods) = call(&off, Method::GET, "/auth/identities", &password_token, None).await;
    assert_eq!(methods, json!({"password": true, "identities": []}));
    // Without the provider configured, a linked identity still shows, by its issuer.
    let (_, methods) = call(&off, Method::GET, "/auth/identities", &token, None).await;
    assert_eq!(methods["identities"][0]["provider"], stub.issuer.as_str());
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_first_sign_in_creates_the_account_just_in_time(pool: PgPool) {
    let stub = Stub::start().await;
    let app = app(pool.clone(), &stub, &[]).await;

    // The login sends the browser to the provider, bound to it by a cookie, with PKCE.
    let (start, location, binding) = begin(&app, Some("/app/graphs")).await;
    assert_eq!(start.status, StatusCode::SEE_OTHER);
    assert!(
        location.starts_with(&format!("{}/authorize?", stub.issuer)),
        "{location}"
    );
    assert!(!binding.is_empty());
    let cookie = start.headers["set-cookie"].to_str().unwrap();
    assert!(
        cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax"),
        "{cookie}"
    );
    stub.expect(Assertion::person("sub-ada", "Ada@Example.com"));
    let back = callback(&app, &local(&stub.authorize(&location)), &binding).await;
    assert_eq!(back.status, StatusCode::SEE_OTHER, "{}", back.body);
    let to = back.headers["location"].to_str().unwrap();
    assert_eq!(
        to, "/app/graphs",
        "where the browser asked to go, carrying no token"
    );
    let cookies: Vec<&str> = back
        .headers
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap())
        .collect();
    assert!(
        cookies
            .iter()
            .any(|c| c.starts_with("nexc_refresh=") && c.contains("SameSite=Strict")),
        "the refresh cookie a password sign-in sets: {cookies:?}"
    );
    assert!(
        cookies.iter().any(|c| c.starts_with("nexc_sso=;")),
        "the binding is cleared: {cookies:?}"
    );
    let refresh = common::oidc::set_cookie(&back.headers, "nexc_refresh").unwrap();

    // The console then refreshes, as after a password sign-in.
    let session = session_of(&app, &refresh).await;
    assert_eq!(session.status, StatusCode::OK, "{}", session.body);
    let user = &session.body["user"];
    assert_eq!(user["email"], "ada@example.com");
    assert_eq!(user["name"], "Ada Lovelace");
    assert_eq!(user["role"], "user");
    let token = session.body["access_token"].as_str().unwrap();
    let (_, workspaces) = call(&app, Method::GET, "/workspaces", token, None).await;
    assert_eq!(
        workspaces.as_array().unwrap().len(),
        1,
        "a personal workspace"
    );
    let kinds = activity(&app, token).await;
    assert_eq!(kinds, ["sso_signed_in", "sso_provisioned"], "{kinds:?}");

    // There is no password to sign in with.
    let login = app
        .request(
            Method::POST,
            "/api/v1/auth/login",
            None,
            Some(json!({"email": "ada@example.com", "password": "!"})),
        )
        .await;
    assert_eq!(login.status, StatusCode::UNAUTHORIZED);

    // The next sign-in is the same account, whatever address the provider gives now.
    let (_, again) = signed_in(
        &app,
        &stub,
        Assertion::person("sub-ada", "ada.lovelace@example.com"),
    )
    .await;
    assert_eq!(again["id"], user["id"]);
    assert_eq!(
        again["email"], "ada@example.com",
        "the account's address stays"
    );
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 1);
    let shown = || async {
        sqlx::query_scalar::<_, String>("SELECT email FROM user_identities")
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    assert_eq!(shown().await, "ada.lovelace@example.com");
    // A sign-in whose provider gives no address keeps the one it gave before.
    let (_, again) = signed_in(
        &app,
        &stub,
        Assertion::person("sub-ada", "unused@example.com").without("email"),
    )
    .await;
    assert_eq!(again["id"], user["id"]);
    assert_eq!(shown().await, "ada.lovelace@example.com");
}

/// The bytes an authenticator app decodes a typed secret to (RFC 4648 base32).
fn base32_decode(text: &str) -> Vec<u8> {
    let (mut buffer, mut bits, mut out) = (0u32, 0u32, Vec::new());
    for c in text.bytes() {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'2'..=b'7' => c - b'2' + 26,
            _ => panic!("not base32: {text}"),
        };
        buffer = (buffer << 5) | u32::from(value);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    out
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_verified_address_links_the_existing_account(pool: PgPool) {
    let stub = Stub::start().await;
    let app = app(pool.clone(), &stub, &[]).await;
    let (password_token, _) = app.register("grace@example.com").await;
    let (_, me) = call(&app, Method::GET, "/auth/me", &password_token, None).await;

    // The account signs in with a second factor; single sign-on does not ask for it.
    let (_, setup) = call(&app, Method::POST, "/auth/2fa/setup", &password_token, None).await;
    let secret = base32_decode(setup["secret"].as_str().unwrap());
    let step = chrono::Utc::now().timestamp() / nexc::security::totp::PERIOD;
    let code = nexc::security::totp::code(&secret, step);
    let (status, _) = call(
        &app,
        Method::POST,
        "/auth/2fa/enable",
        &password_token,
        Some(json!({"code": code})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (token, user) = signed_in(
        &app,
        &stub,
        Assertion::person("sub-grace", "GRACE@example.com"),
    )
    .await;
    assert_eq!(user["id"], me["id"], "linked, not created");
    let kinds = activity(&app, &token).await;
    assert_eq!(&kinds[..2], ["sso_signed_in", "sso_linked"], "{kinds:?}");

    // Another identity of the provider with the same address is not linked as well.
    let other = trip(
        &app,
        &stub,
        Assertion::person("sub-impostor", "grace@example.com"),
        None,
    )
    .await;
    assert_eq!(other.location, "/login?sso_error=email_taken");
    assert!(other.refresh.is_none());
    assert_eq!(
        count(&pool, "SELECT count(*) FROM user_identities").await,
        1
    );
    // The password still works alongside.
    let login = app
        .request(
            Method::POST,
            "/api/v1/auth/login",
            None,
            Some(json!({"email": "grace@example.com", "password": PASSWORD, "code": nexc::security::totp::code(&secret, step + 1)})),
        )
        .await;
    assert_eq!(login.status, StatusCode::OK, "{}", login.body);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_unverified_address_is_never_linked(pool: PgPool) {
    let stub = Stub::start().await;
    let strict = app(pool.clone(), &stub, &[]).await;
    let (owner, _) = strict.register("hopper@example.com").await;
    let unverified = |sub: &str, email: &str| {
        Assertion::person(sub, email).claim("email_verified", json!(false))
    };

    let t = trip(
        &strict,
        &stub,
        unverified("sub-h", "hopper@example.com"),
        None,
    )
    .await;
    assert_eq!(t.location, "/login?sso_error=unverified_email");
    // Nor does an absent claim vouch for it.
    let t = trip(
        &strict,
        &stub,
        Assertion::person("sub-h", "hopper@example.com").without("email_verified"),
        None,
    )
    .await;
    assert_eq!(t.location, "/login?sso_error=unverified_email");
    // A new address the provider does not vouch for gets no account either.
    let t = trip(&strict, &stub, unverified("sub-n", "new@example.com"), None).await;
    assert_eq!(t.location, "/login?sso_error=unverified_email");
    assert_eq!(
        count(&pool, "SELECT count(*) FROM user_identities").await,
        0
    );

    // Allowed to create accounts for unverified addresses, it still links none, and accepts no
    // invitation sent to such an address.
    let lax = app(
        pool.clone(),
        &stub,
        &[("NEXC_OIDC_REQUIRE_VERIFIED_EMAIL", "false")],
    )
    .await;
    let t = trip(&lax, &stub, unverified("sub-h", "hopper@example.com"), None).await;
    assert_eq!(t.location, "/login?sso_error=unverified_email");
    let (_, workspaces) = call(&lax, Method::GET, "/workspaces", &owner, None).await;
    let wid = workspaces[0]["id"].as_str().unwrap().to_owned();
    let (status, _) = call(
        &lax,
        Method::POST,
        &format!("/workspaces/{wid}/members"),
        &owner,
        Some(json!({"email": "newbie@example.com", "role": "member"})),
    )
    .await;
    assert!(status.is_success());
    let t = trip(
        &lax,
        &stub,
        unverified("sub-newbie", "newbie@example.com"),
        None,
    )
    .await;
    let session = session_of(&lax, &t.refresh.expect("signed in")).await;
    let token = session.body["access_token"].as_str().unwrap();
    let (_, theirs) = call(&lax, Method::GET, "/workspaces", token, None).await;
    let theirs = theirs.as_array().unwrap();
    assert_eq!(theirs.len(), 1);
    assert_ne!(
        theirs[0]["id"], wid,
        "the invitation waits for a verified address"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn tokens_that_do_not_verify_are_refused(pool: PgPool) {
    let stub = Stub::start().await;
    let app = app(pool.clone(), &stub, &[]).await;
    let now = chrono::Utc::now().timestamp();
    let person = || Assertion::person("sub-mallory", "mallory@example.com");
    let cases = [
        (
            "a signature by a key that is not the one named",
            person().signed(Signing::Key {
                key: "k2".into(),
                kid: "k1".into(),
            }),
        ),
        (
            "another audience",
            person().override_claim("aud", json!("someone-else")),
        ),
        (
            "several audiences and no authorized party",
            person().override_claim("aud", json!(["nexc-test", "someone-else"])),
        ),
        (
            "issued to another authorized party",
            person().override_claim("azp", json!("someone-else")),
        ),
        ("expired", person().override_claim("exp", json!(now - 3600))),
        (
            "issued in the future",
            person().override_claim("iat", json!(now + 3600)),
        ),
        (
            "issued long ago",
            person().override_claim("iat", json!(now - 7200)),
        ),
        (
            "not valid yet",
            person().override_claim("nbf", json!(now + 3600)),
        ),
        (
            "another nonce",
            person().override_claim("nonce", json!("not-the-nonce")),
        ),
        ("no nonce", person().override_claim("nonce", Value::Null)),
        (
            "another issuer",
            person().override_claim("iss", json!("https://evil.example")),
        ),
        ("alg none", person().signed(Signing::None)),
        (
            "HS256 with the client secret",
            person().signed(Signing::Hs256(CLIENT_SECRET.into())),
        ),
        (
            "HS256 with anything",
            person().signed(Signing::Hs256("0123456789abcdef0123456789abcdef".into())),
        ),
    ];
    for (name, assertion) in cases {
        let t = trip(&app, &stub, assertion, None).await;
        assert_eq!(t.status, StatusCode::SEE_OTHER, "{name}");
        assert_eq!(t.location, "/login?sso_error=invalid", "{name}");
        assert!(t.refresh.is_none(), "{name}");
    }
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 0);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM user_identities").await,
        0
    );
    // The provider itself is fine: an honest token signs in.
    signed_in(&app, &stub, person()).await;
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_state_works_once_for_ten_minutes_in_its_own_browser(pool: PgPool) {
    let stub = Stub::start().await;
    let app = app(pool.clone(), &stub, &[]).await;
    let person = || Assertion::person("sub-eve", "eve@example.com");
    let first = trip(&app, &stub, person(), None).await;
    assert!(first.refresh.is_some());
    let exchanges = stub.exchanges();

    // The same answer again: the state was used.
    let replay = callback(&app, &first.callback, &first.binding).await;
    assert_eq!(
        replay.headers["location"].to_str().unwrap(),
        "/login?sso_error=expired"
    );
    assert!(common::oidc::set_cookie(&replay.headers, "nexc_refresh").is_none());
    assert_eq!(stub.exchanges(), exchanges, "the code is not even tried");

    // Started in one browser, finished in another: refused, and the state is spent.
    stub.expect(person());
    let (_, location, binding) = begin(&app, None).await;
    let back = local(&stub.authorize(&location));
    let elsewhere = callback(&app, &back, "another-browsers-cookie").await;
    assert_eq!(
        elsewhere.headers["location"].to_str().unwrap(),
        "/login?sso_error=invalid"
    );
    let late = callback(&app, &back, &binding).await;
    assert_eq!(
        late.headers["location"].to_str().unwrap(),
        "/login?sso_error=expired"
    );

    // Older than ten minutes.
    stub.expect(person());
    let (_, location, binding) = begin(&app, None).await;
    let back = local(&stub.authorize(&location));
    sqlx::query("UPDATE oidc_logins SET expires_at = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    let stale = callback(&app, &back, &binding).await;
    assert_eq!(
        stale.headers["location"].to_str().unwrap(),
        "/login?sso_error=expired"
    );

    // Made up, missing, or declined at the provider.
    for (path, code) in [
        ("/api/v1/auth/oidc/callback?code=c&state=made-up", "expired"),
        ("/api/v1/auth/oidc/callback?code=c", "invalid"),
        ("/api/v1/auth/oidc/callback", "invalid"),
    ] {
        let r = callback(&app, path, &binding).await;
        assert_eq!(
            r.headers["location"].to_str().unwrap(),
            format!("/login?sso_error={code}"),
            "{path}"
        );
    }
    let (_, location, binding) = begin(&app, None).await;
    let declined = callback(&app, &local(&stub.decline(&location)), &binding).await;
    assert_eq!(
        declined.headers["location"].to_str().unwrap(),
        "/login?sso_error=denied"
    );
    // An answer that says it comes from another issuer (RFC 9207).
    stub.expect(person());
    let (_, location, binding) = begin(&app, None).await;
    let mixed = format!(
        "{}&iss=https%3A%2F%2Fevil.example",
        local(&stub.authorize(&location))
    );
    let r = callback(&app, &mixed, &binding).await;
    assert_eq!(
        r.headers["location"].to_str().unwrap(),
        "/login?sso_error=invalid"
    );
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 1);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn rotated_keys_are_fetched_again_once(pool: PgPool) {
    let stub = Stub::start().await;
    let app = app(pool.clone(), &stub, &[]).await;
    let person = || Assertion::person("sub-kim", "kim@example.com");
    signed_in(&app, &stub, person()).await;
    assert_eq!(stub.jwks_fetches(), 1);
    signed_in(&app, &stub, person()).await;
    assert_eq!(stub.jwks_fetches(), 1, "the key set is kept");

    // The provider rotates to k2: the unknown key id fetches the set again, once.
    stub.publish(&["k2"]);
    let by_k2 = || {
        person().signed(Signing::Key {
            key: "k2".into(),
            kid: "k2".into(),
        })
    };
    signed_in(&app, &stub, by_k2()).await;
    assert_eq!(stub.jwks_fetches(), 2);
    signed_in(&app, &stub, by_k2()).await;
    assert_eq!(stub.jwks_fetches(), 2);

    // A key id nobody publishes does not make the set fetched again within the minute.
    let t = trip(
        &app,
        &stub,
        person().signed(Signing::Key {
            key: "k2".into(),
            kid: "k9".into(),
        }),
        None,
    )
    .await;
    assert_eq!(t.location, "/login?sso_error=invalid");
    // Nor does the retired key verify any more.
    let t = trip(&app, &stub, person(), None).await;
    assert_eq!(t.location, "/login?sso_error=invalid");
    assert_eq!(stub.jwks_fetches(), 2);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn es256_tokens_sign_in_too(pool: PgPool) {
    let stub = Stub::start().await;
    stub.publish(&["k1", "e1"]);
    let app = app(pool, &stub, &[]).await;
    let (_, user) = signed_in(
        &app,
        &stub,
        Assertion::person("sub-ec", "ec@example.com").signed(Signing::Key {
            key: "e1".into(),
            kid: "e1".into(),
        }),
    )
    .await;
    assert_eq!(user["email"], "ec@example.com");
    // An EC key named for an RS256 token does not verify it.
    let t = trip(
        &app,
        &stub,
        Assertion::person("sub-ec", "ec@example.com").signed(Signing::Key {
            key: "k1".into(),
            kid: "e1".into(),
        }),
        None,
    )
    .await;
    assert_eq!(t.location, "/login?sso_error=invalid");
}

async fn role_in(pool: &PgPool, wid: &str, user: &Value) -> Option<String> {
    sqlx::query_scalar(
        "SELECT role FROM workspace_members WHERE workspace_id = $1::uuid AND user_id = $2::uuid",
    )
    .bind(wid)
    .bind(user["id"].as_str().unwrap())
    .fetch_optional(pool)
    .await
    .unwrap()
}

async fn audit_of(pool: &PgPool, wid: &str) -> Vec<(String, String, String)> {
    sqlx::query_as(
        "SELECT action, actor_name, detail FROM audit_log WHERE workspace_id = $1::uuid
         ORDER BY created_at, id",
    )
    .bind(wid)
    .fetch_all(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn groups_give_roles_and_take_back_only_what_they_gave(pool: PgPool) {
    let stub = Stub::start().await;
    let plain = TestApp::new(pool.clone(), &[]).await;
    let (owner, _) = plain.register("owner@example.com").await;
    let (_, list) = call(&plain, Method::GET, "/workspaces", &owner, None).await;
    let wid = list[0]["id"].as_str().unwrap().to_owned();
    // `ghosts` names a workspace that does not exist (any more).
    let gone = uuid::Uuid::now_v7();
    let mapping = format!("eng={wid}:member,leads={wid}:admin,ghosts={gone}:owner");
    let app = app(
        pool.clone(),
        &stub,
        &[
            ("NEXC_OIDC_ADMIN_GROUPS", "nexc-admins"),
            ("NEXC_OIDC_WORKSPACE_ROLES", &mapping),
        ],
    )
    .await;

    // The admin group makes a platform administrator, from the first sign-in.
    let root = || Assertion::person("sub-root", "root@example.com");
    let (token, user) = signed_in(&app, &stub, root().groups(&["nexc-admins"])).await;
    assert_eq!(user["role"], "admin");
    let (status, _) = call(&app, Method::GET, "/admin/users", &token, None).await;
    assert_eq!(status, StatusCode::OK);
    // Without the group at the next sign-in, the role the group gave is gone, and the account
    // gets a workspace to work in.
    let (token, user) = signed_in(&app, &stub, root().groups(&["staff"])).await;
    assert_eq!(user["role"], "user");
    let (_, theirs) = call(&app, Method::GET, "/workspaces", &token, None).await;
    assert_eq!(theirs.as_array().unwrap().len(), 1);
    let kinds = activity(&app, &token).await;
    assert_eq!(
        kinds,
        [
            "sso_signed_in",
            "role_changed",
            "sso_signed_in",
            "sso_provisioned"
        ],
        "the first role was given at creation, the change is noted: {kinds:?}"
    );
    // Both are in the platform's activity log too, by single sign-on, as the console's are.
    let events: Vec<(Option<uuid::Uuid>, String, String, String, String)> = sqlx::query_as(
        "SELECT actor_id, actor_name, action, subject, detail FROM platform_events
         ORDER BY created_at, id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(events.len(), 2, "{events:?}");
    for (actor_id, actor, action, subject, _) in &events {
        assert_eq!(*actor_id, None);
        assert_eq!(actor, "single sign-on (Acme SSO)");
        assert_eq!(action, "role_changed");
        assert_eq!(subject, "Ada Lovelace <root@example.com>");
    }
    assert!(
        events[0].4.contains("→ admin") && events[0].4.contains("`nexc-admins`"),
        "{events:?}"
    );
    assert!(events[1].4.starts_with("admin → user"), "{events:?}");
    // A platform role given in the console is never taken back by a sign-in.
    sqlx::query("UPDATE users SET role = 'admin' WHERE email = 'root@example.com'")
        .execute(&pool)
        .await
        .unwrap();
    let (_, user) = signed_in(&app, &stub, root().groups(&[])).await;
    assert_eq!(user["role"], "admin");

    // Workspace roles: the highest of the person's groups.
    let dev = || Assertion::person("sub-dev", "dev@example.com");
    let (_, user) = signed_in(&app, &stub, dev().groups(&["eng"])).await;
    assert_eq!(role_in(&pool, &wid, &user).await.as_deref(), Some("member"));
    let (_, user) = signed_in(&app, &stub, dev().groups(&["eng", "leads", "ghosts"])).await;
    assert_eq!(role_in(&pool, &wid, &user).await.as_deref(), Some("admin"));
    let (_, user) = signed_in(&app, &stub, dev().groups(&["eng"])).await;
    assert_eq!(role_in(&pool, &wid, &user).await.as_deref(), Some("member"));
    // No group any more: the membership the mapping gave goes, and the account keeps a
    // workspace of its own.
    let (token, user) = signed_in(&app, &stub, dev().groups(&[])).await;
    assert_eq!(role_in(&pool, &wid, &user).await, None);
    let (_, theirs) = call(&app, Method::GET, "/workspaces", &token, None).await;
    assert_eq!(theirs.as_array().unwrap().len(), 1);
    let log = audit_of(&pool, &wid).await;
    let by_sso: Vec<&(String, String, String)> = log
        .iter()
        .filter(|(_, actor, _)| actor == "system")
        .collect();
    let actions: Vec<&str> = by_sso.iter().map(|(a, _, _)| a.as_str()).collect();
    assert_eq!(
        actions,
        [
            "member_added",
            "member_role_changed",
            "member_role_changed",
            "member_removed"
        ],
        "{log:?}"
    );
    assert!(by_sso[0].2.contains("group `eng`"), "{:?}", by_sso[0]);
    assert!(by_sso[3].2.contains("no longer applies"), "{:?}", by_sso[3]);

    // A role someone set by hand since is not the mapping's to take back.
    let (_, user) = signed_in(&app, &stub, dev().groups(&["eng"])).await;
    let uid = user["id"].as_str().unwrap();
    let (status, body) = call(
        &plain,
        Method::PATCH,
        &format!("/workspaces/{wid}/members/{uid}"),
        &owner,
        Some(json!({"role": "admin"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, user) = signed_in(&app, &stub, dev().groups(&[])).await;
    assert_eq!(role_in(&pool, &wid, &user).await.as_deref(), Some("admin"));

    // A member before the mapping is raised by it and goes back to what it was without it.
    plain.register("guest@example.com").await;
    let (status, _) = call(
        &plain,
        Method::POST,
        &format!("/workspaces/{wid}/members"),
        &owner,
        Some(json!({"email": "guest@example.com", "role": "guest"})),
    )
    .await;
    assert!(status.is_success());
    let guest = || Assertion::person("sub-guest", "guest@example.com");
    let (_, user) = signed_in(&app, &stub, guest().groups(&["leads"])).await;
    assert_eq!(role_in(&pool, &wid, &user).await.as_deref(), Some("admin"));
    let (_, user) = signed_in(&app, &stub, guest().groups(&[])).await;
    assert_eq!(role_in(&pool, &wid, &user).await.as_deref(), Some("guest"));

    // Groups the ID token does not carry are read from userinfo.
    let (_, user) = signed_in(
        &app,
        &stub,
        Assertion::person("sub-info", "info@example.com").userinfo("groups", json!(["eng"])),
    )
    .await;
    assert_eq!(role_in(&pool, &wid, &user).await.as_deref(), Some("member"));
}

/// A provider that leaves the groups claim out (an Entra ID overage, a mapper
/// not added to the token, a userinfo answer that is a signed JWT) says
/// nothing about groups: the roles they gave stay, and only a list that is
/// there, empty or not, gives and takes back.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_sign_in_without_the_groups_claim_changes_no_role(pool: PgPool) {
    let stub = Stub::start().await;
    let plain = TestApp::new(pool.clone(), &[]).await;
    let (owner, _) = plain.register("owner@example.com").await;
    let (_, list) = call(&plain, Method::GET, "/workspaces", &owner, None).await;
    let wid = list[0]["id"].as_str().unwrap().to_owned();
    let mapping = format!("eng={wid}:member");
    let app = app(
        pool.clone(),
        &stub,
        &[
            ("NEXC_OIDC_ADMIN_GROUPS", "nexc-admins"),
            ("NEXC_OIDC_WORKSPACE_ROLES", &mapping),
        ],
    )
    .await;

    let root = || Assertion::person("sub-root", "root@example.com");
    let (_, user) = signed_in(&app, &stub, root().groups(&["nexc-admins"])).await;
    assert_eq!(user["role"], "admin");
    let (_, user) = signed_in(&app, &stub, root()).await;
    assert_eq!(user["role"], "admin", "no claim takes nothing back");

    let dev = || Assertion::person("sub-dev", "dev@example.com");
    let (_, user) = signed_in(&app, &stub, dev().groups(&["eng"])).await;
    assert_eq!(role_in(&pool, &wid, &user).await.as_deref(), Some("member"));
    let logged = audit_of(&pool, &wid).await.len();
    let (token, user) = signed_in(&app, &stub, dev()).await;
    assert_eq!(role_in(&pool, &wid, &user).await.as_deref(), Some("member"));
    assert_eq!(audit_of(&pool, &wid).await.len(), logged, "nothing changed");
    let (_, theirs) = call(&app, Method::GET, "/workspaces", &token, None).await;
    assert_eq!(theirs.as_array().unwrap().len(), 1, "no personal workspace");

    // An empty list is a sign-in without the groups.
    let (_, user) = signed_in(&app, &stub, dev().groups(&[])).await;
    assert_eq!(role_in(&pool, &wid, &user).await, None);
    let (_, user) = signed_in(&app, &stub, root().groups(&[])).await;
    assert_eq!(user["role"], "user");

    // A first sign-in without the claim makes a plain account with a workspace of its own.
    let (token, user) =
        signed_in(&app, &stub, Assertion::person("sub-new", "new@example.com")).await;
    assert_eq!(user["role"], "user");
    let (_, theirs) = call(&app, Method::GET, "/workspaces", &token, None).await;
    assert_eq!(theirs.as_array().unwrap().len(), 1);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn password_sign_in_can_be_off_but_for_the_break_glass_administrator(pool: PgPool) {
    let stub = Stub::start().await;
    let plain = TestApp::new(pool.clone(), &[]).await;
    plain.register("worker@example.com").await;
    let hash = nexc::security::password::hash_password(PASSWORD).unwrap();
    nexc::http::handlers::auth::create_user(
        &plain.state,
        "root@example.com",
        "Root",
        Role::Admin,
        &hash,
    )
    .await
    .unwrap();
    let app = app(
        pool.clone(),
        &stub,
        &[
            ("NEXC_OIDC_ALLOW_LOCAL_LOGIN", "false"),
            ("NEXC_OIDC_BREAK_GLASS_EMAIL", "Root@Example.com"),
        ],
    )
    .await;
    let login = |email: &str, password: &str| {
        let body = json!({"email": email, "password": password});
        let app = &app;
        async move {
            app.request(Method::POST, "/api/v1/auth/login", None, Some(body))
                .await
        }
    };
    // `nexc doctor` and the start-up check say whether the break-glass address opens anything.
    let problem = |email: &'static str| {
        let pool = pool.clone();
        async move {
            nexc::engine::sso::break_glass_problem(&pool, email)
                .await
                .unwrap()
        }
    };
    assert_eq!(problem("root@example.com").await, None);
    assert!(
        problem("worker@example.com")
            .await
            .unwrap()
            .contains("not a platform administrator")
    );
    assert!(
        problem("rot@example.com")
            .await
            .unwrap()
            .contains("names no account")
    );

    let refused = login("worker@example.com", PASSWORD).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert!(
        refused.body["detail"]
            .as_str()
            .unwrap()
            .contains("Acme SSO"),
        "{}",
        refused.body
    );
    let register = app
        .request(
            Method::POST,
            "/api/v1/auth/register",
            None,
            Some(json!({"email": "new@example.com", "password": PASSWORD, "name": "New"})),
        )
        .await;
    assert_eq!(register.status, StatusCode::FORBIDDEN);
    assert_eq!(
        login("root@example.com", PASSWORD).await.status,
        StatusCode::OK
    );
    assert_eq!(
        login("root@example.com", "a wrong password!").await.status,
        StatusCode::UNAUTHORIZED
    );
    // Single sign-on still signs in.
    signed_in(
        &app,
        &stub,
        Assertion::person("sub-w", "worker@example.com"),
    )
    .await;

    // The break-glass address only opens a platform administrator's account.
    let misnamed = self::app(
        pool,
        &stub,
        &[
            ("NEXC_OIDC_ALLOW_LOCAL_LOGIN", "false"),
            ("NEXC_OIDC_BREAK_GLASS_EMAIL", "worker@example.com"),
        ],
    )
    .await;
    let r = misnamed
        .request(
            Method::POST,
            "/api/v1/auth/login",
            None,
            Some(json!({"email": "worker@example.com", "password": PASSWORD})),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_browser_only_ever_returns_to_this_site(pool: PgPool) {
    let stub = Stub::start().await;
    let app = app(pool, &stub, &[]).await;
    let person = || Assertion::person("sub-r", "r@example.com");
    for target in [
        "https://evil.example/app",
        "//evil.example",
        "/\\evil.example",
        "/%2F%2Fevil.example",
        "javascript:alert(1)",
        "/app\r\nSet-Cookie: x=1",
    ] {
        let t = trip(&app, &stub, person(), Some(target)).await;
        assert_eq!(t.location, "/app", "{target:?}");
    }
    let t = trip(&app, &stub, person(), Some("/app/runs?tab=all#latest")).await;
    assert_eq!(t.location, "/app/runs?tab=all#latest");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_suspended_account_is_refused(pool: PgPool) {
    let stub = Stub::start().await;
    let app = app(pool.clone(), &stub, &[]).await;
    let person = || Assertion::person("sub-s", "s@example.com");
    let (_, user) = signed_in(&app, &stub, person()).await;
    sqlx::query("UPDATE users SET suspended_at = now() WHERE id = $1::uuid")
        .bind(user["id"].as_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let last_sign_in = || async {
        sqlx::query_scalar::<_, Option<chrono::DateTime<chrono::Utc>>>(
            "SELECT last_login_at FROM user_identities",
        )
        .fetch_one(&pool)
        .await
        .unwrap()
    };
    let before = last_sign_in().await;
    let t = trip(&app, &stub, person(), None).await;
    assert_eq!(t.location, "/login?sso_error=suspended");
    assert!(t.refresh.is_none());
    assert_eq!(
        last_sign_in().await,
        before,
        "a refused sign-in is no sign-in"
    );
    let failed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM account_events WHERE user_id = $1::uuid AND kind = 'sign_in_failed'",
    )
    .bind(user["id"].as_str().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(failed, 1);
}
