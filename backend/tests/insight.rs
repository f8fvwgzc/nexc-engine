//! A workspace's day told by its model: written on request, kept, marked
//! stale when more happens, and paid for like every other model call.
#![forbid(unsafe_code)]

mod common;

use std::sync::atomic::Ordering;

use axum::http::{Method, StatusCode};
use common::TestApp;
use serde_json::{Value, json};
use sqlx::PgPool;

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

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_day_is_summarised_on_request_and_kept(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let (member, _) = app.register("member@example.com").await;
    let (_, list) = call(&app, Method::GET, "/workspaces", &owner, None).await;
    let ws = format!("/workspaces/{}", list[0]["id"].as_str().unwrap());
    let invite = json!({"email": "member@example.com", "role": "member"});
    call(
        &app,
        Method::POST,
        &format!("{ws}/members"),
        &owner,
        Some(invite),
    )
    .await;
    let team = json!({"name": "Engineering", "key": "ENG"});
    let (_, team) = call(
        &app,
        Method::POST,
        &format!("{ws}/teams"),
        &owner,
        Some(team),
    )
    .await;
    let issues = format!("{ws}/teams/{}/issues", team["id"].as_str().unwrap());
    let issue = json!({"title": "Ship login"});
    call(&app, Method::POST, &issues, &member, Some(issue)).await;
    let summary = format!("{ws}/timeline/summary");
    let calls = || app.fake.calls.load(Ordering::SeqCst);

    // Like the timeline it is told from, a summary is for admins and owners.
    for method in [Method::GET, Method::POST] {
        let (status, _) = call(&app, method, &summary, &member, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    // Nothing is written before someone asks, and asking about it spends nothing.
    let (status, none) = call(&app, Method::GET, &summary, &owner, None).await;
    assert_eq!((status, none.is_null()), (StatusCode::OK, true), "{none}");
    assert_eq!(calls(), 0);

    // Written from the day's three entries: the member, the team and the issue.
    let (status, written) = call(&app, Method::POST, &summary, &owner, None).await;
    assert_eq!(status, StatusCode::OK, "{written}");
    let today = chrono::Utc::now().date_naive().to_string();
    assert_eq!(
        written["headline"].as_str(),
        Some(format!("3 things happened on {today} (UTC).").as_str())
    );
    assert_eq!(
        written["highlights"],
        json!(["By kind: issue_created 1, member_added 1, team_created 1"]),
        "the digest reached the model, and the empty point was dropped"
    );
    assert_eq!(written["attention"], json!([]));
    assert_eq!(
        (
            written["day"].as_str(),
            written["event_count"].as_i64(),
            written["stale"].as_bool(),
            written["created_by"].as_str(),
        ),
        (Some(today.as_str()), Some(3), Some(false), Some("Test"))
    );
    assert!(!written["model"].as_str().unwrap().is_empty());
    assert_eq!(calls(), 1);

    // Reading it again is free, and it is booked as its own kind of usage.
    let (_, kept) = call(&app, Method::GET, &summary, &owner, None).await;
    assert_eq!(kept, written);
    assert_eq!(calls(), 1);
    let (_, report) = call(&app, Method::GET, &format!("{ws}/usage"), &owner, None).await;
    let booked = report["by_purpose"]
        .as_array()
        .unwrap()
        .iter()
        .find(|slice| slice["key"] == "summary")
        .expect("the summary is in the usage report");
    assert_eq!(booked["calls"].as_i64(), Some(1));

    // More happens: the kept summary says so, and writing it again catches up.
    let issue = json!({"title": "Reset password"});
    call(&app, Method::POST, &issues, &member, Some(issue)).await;
    let (_, stale) = call(&app, Method::GET, &summary, &owner, None).await;
    assert_eq!(
        (stale["stale"].as_bool(), stale["event_count"].as_i64()),
        (Some(true), Some(3))
    );
    let (_, again) = call(&app, Method::POST, &summary, &owner, None).await;
    assert_eq!(
        (again["stale"].as_bool(), again["event_count"].as_i64()),
        (Some(false), Some(4))
    );
    assert_eq!(calls(), 2);

    // A day on which nothing happened is not sent to a model.
    let quiet = format!("{summary}?day=2001-01-01");
    let (status, _) = call(&app, Method::POST, &quiet, &owner, None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (_, none) = call(&app, Method::GET, &quiet, &owner, None).await;
    assert!(none.is_null());
    assert_eq!(calls(), 2);

    // The workspace's guardrails apply: with the month's budget used up, nothing is written.
    let rails = json!({"monthly_token_budget": 1});
    let (status, _) = call(
        &app,
        Method::PUT,
        &format!("{ws}/guardrails"),
        &owner,
        Some(rails),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, refused) = call(&app, Method::POST, &summary, &owner, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    assert_eq!(calls(), 2);
    let (_, still) = call(&app, Method::GET, &summary, &owner, None).await;
    assert_eq!(
        still["event_count"].as_i64(),
        Some(4),
        "the kept one stays readable"
    );
}
