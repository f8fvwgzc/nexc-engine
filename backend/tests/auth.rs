//! Authentication flow: register, login, refresh rotation, reuse detection,
//! logout, rate limiting and lockout.
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode, header};
use common::{PASSWORD, TestApp, refresh_cookie};
use serde_json::json;
use sqlx::PgPool;

const CSRF: (&str, &str) = ("x-requested-with", "nexc");

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn register_login_refresh_logout(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let r = app
        .request(
            Method::POST,
            "/api/v1/auth/register",
            None,
            Some(json!({"email": "Ada@Example.com", "password": PASSWORD, "name": "Ada"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert_eq!(r.body["user"]["email"], "ada@example.com");
    assert_eq!(r.body["user"]["role"], "user");
    assert_eq!(r.body["expires_in"], 900);
    let set_cookie = r.headers[header::SET_COOKIE].to_str().unwrap();
    assert!(
        set_cookie.contains("HttpOnly")
            && set_cookie.contains("SameSite=Strict")
            && set_cookie.contains("Path=/api/v1/auth")
    );
    let token = r.body["access_token"].as_str().unwrap().to_owned();

    let me = app
        .request(Method::GET, "/api/v1/auth/me", Some(&token), None)
        .await;
    assert_eq!(
        (me.status, me.body["name"].as_str()),
        (StatusCode::OK, Some("Ada"))
    );

    let dup = app
        .request(
            Method::POST,
            "/api/v1/auth/register",
            None,
            Some(json!({"email": "ada@example.com", "password": PASSWORD, "name": "A"})),
        )
        .await;
    assert_eq!(dup.status, StatusCode::CONFLICT);

    let bad = app
        .request(
            Method::POST,
            "/api/v1/auth/login",
            None,
            Some(json!({"email": "ada@example.com", "password": "wrong password!"})),
        )
        .await;
    assert_eq!(bad.status, StatusCode::UNAUTHORIZED);
    let unknown = app
        .request(
            Method::POST,
            "/api/v1/auth/login",
            None,
            Some(json!({"email": "nobody@example.com", "password": PASSWORD})),
        )
        .await;
    assert_eq!(unknown.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        bad.body["detail"], unknown.body["detail"],
        "failures are indistinguishable"
    );

    let login = app
        .request(
            Method::POST,
            "/api/v1/auth/login",
            None,
            Some(json!({"email": "ada@example.com", "password": PASSWORD})),
        )
        .await;
    assert_eq!(login.status, StatusCode::OK);
    let first = refresh_cookie(&login.headers).unwrap();
    let cookie = |v: &str| format!("nexc_refresh={v}");

    let no_csrf = app
        .request_with(
            Method::POST,
            "/api/v1/auth/refresh",
            None,
            None,
            &[("cookie", &cookie(&first))],
        )
        .await;
    assert_eq!(no_csrf.status, StatusCode::FORBIDDEN);

    let rotated = app
        .request_with(
            Method::POST,
            "/api/v1/auth/refresh",
            None,
            None,
            &[("cookie", &cookie(&first)), CSRF],
        )
        .await;
    assert_eq!(rotated.status, StatusCode::OK);
    let second = refresh_cookie(&rotated.headers).unwrap();
    assert_ne!(first, second);

    // Reusing the consumed token revokes the whole family, including `second`.
    let reuse = app
        .request_with(
            Method::POST,
            "/api/v1/auth/refresh",
            None,
            None,
            &[("cookie", &cookie(&first)), CSRF],
        )
        .await;
    assert_eq!(reuse.status, StatusCode::UNAUTHORIZED);
    let after = app
        .request_with(
            Method::POST,
            "/api/v1/auth/refresh",
            None,
            None,
            &[("cookie", &cookie(&second)), CSRF],
        )
        .await;
    assert_eq!(after.status, StatusCode::UNAUTHORIZED);

    let again = app
        .request(
            Method::POST,
            "/api/v1/auth/login",
            None,
            Some(json!({"email": "ada@example.com", "password": PASSWORD})),
        )
        .await;
    let third = refresh_cookie(&again.headers).unwrap();
    let out = app
        .request_with(
            Method::POST,
            "/api/v1/auth/logout",
            None,
            None,
            &[("cookie", &cookie(&third)), CSRF],
        )
        .await;
    assert_eq!(out.status, StatusCode::NO_CONTENT);
    assert!(
        out.headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("Max-Age=0")
    );
    let dead = app
        .request_with(
            Method::POST,
            "/api/v1/auth/refresh",
            None,
            None,
            &[("cookie", &cookie(&third)), CSRF],
        )
        .await;
    assert_eq!(dead.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn validation_errors_are_problem_json(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let r = app
        .request(
            Method::POST,
            "/api/v1/auth/register",
            None,
            Some(json!({"email": "nope", "password": "short", "name": ""})),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r.headers[header::CONTENT_TYPE], "application/problem+json");
    assert_eq!(r.body["type"], "about:blank");
    assert_eq!(r.body["status"], 422);
    assert_eq!(r.body["title"], "Validation failed");
    for field in ["email", "password", "name"] {
        assert!(
            r.body["errors"][field].is_array(),
            "missing error for {field}: {}",
            r.body
        );
    }
    let unknown = app
        .request(
            Method::POST,
            "/api/v1/auth/login",
            None,
            Some(json!({"email": "a@b.co", "password": "x", "admin": true})),
        )
        .await;
    assert_eq!(
        unknown.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "unknown fields are rejected"
    );
    let unauth = app.request(Method::GET, "/api/v1/graphs", None, None).await;
    assert_eq!(unauth.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        unauth.headers[header::CONTENT_TYPE],
        "application/problem+json"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn failed_logins_are_rate_limited_then_locked(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    app.register("eve@example.com").await;
    let wrong = json!({"email": "eve@example.com", "password": "definitely wrong"});
    for _ in 0..5 {
        let r = app
            .request(
                Method::POST,
                "/api/v1/auth/login",
                None,
                Some(wrong.clone()),
            )
            .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    }
    let limited = app
        .request(Method::POST, "/api/v1/auth/login", None, Some(wrong))
        .await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(limited.headers.contains_key(header::RETRY_AFTER));

    let locked: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT locked_until FROM users WHERE email = 'eve@example.com'")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert!(
        locked.is_some(),
        "five consecutive failures lock the account"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn signups_can_be_disabled(pool: PgPool) {
    let app = TestApp::new(pool, &[("NEXC_ALLOW_SIGNUP", "false")]).await;
    let r = app
        .request(
            Method::POST,
            "/api/v1/auth/register",
            None,
            Some(json!({"email": "a@example.com", "password": PASSWORD, "name": "A"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
}
