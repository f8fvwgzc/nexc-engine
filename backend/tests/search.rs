//! Searching a workspace from the command palette: every kind of thing,
//! and only what the caller may see.
#![forbid(unsafe_code)]

mod common;

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

/// The hits of a search as `(kind, title)`.
async fn find(app: &TestApp, ws: &str, token: &str, q: &str) -> Vec<(String, String)> {
    let (status, hits) = call(app, Method::GET, &format!("{ws}/search?q={q}"), token, None).await;
    assert_eq!(status, StatusCode::OK, "{hits}");
    hits.as_array()
        .unwrap()
        .iter()
        .map(|h| {
            (
                h["kind"].as_str().unwrap().to_owned(),
                h["title"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn hit(kind: &str, title: &str) -> (String, String) {
    (kind.to_owned(), title.to_owned())
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_search_finds_every_kind_and_only_what_the_caller_may_see(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let (member, _) = app.register("member@example.com").await;
    let (guest, _) = app.register("guest@example.com").await;
    let (outsider, _) = app.register("outsider@example.com").await;
    let (_, list) = call(&app, Method::GET, "/workspaces", &owner, None).await;
    let wid = list[0]["id"].as_str().unwrap().to_owned();
    let ws = format!("/workspaces/{wid}");
    for (email, role) in [
        ("member@example.com", "member"),
        ("guest@example.com", "guest"),
    ] {
        let invite = json!({"email": email, "role": role});
        let (status, _) = call(
            &app,
            Method::POST,
            &format!("{ws}/members"),
            &owner,
            Some(invite),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let teams = format!("{ws}/teams");
    let open = json!({"name": "Engineering", "key": "ENG"});
    let (_, open) = call(&app, Method::POST, &teams, &owner, Some(open)).await;
    let closed = json!({"name": "Leadership", "key": "LEAD", "private": true});
    let (_, closed) = call(&app, Method::POST, &teams, &owner, Some(closed)).await;
    for (team, title) in [(&open, "Rocket engine test"), (&closed, "Rocket budget")] {
        let path = format!("{ws}/teams/{}/issues", team["id"].as_str().unwrap());
        let (status, _) = call(
            &app,
            Method::POST,
            &path,
            &owner,
            Some(json!({"title": title})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let project = json!({"name": "Rocket launch"});
    call(
        &app,
        Method::POST,
        &format!("{ws}/projects"),
        &owner,
        Some(project),
    )
    .await;
    let graph = json!({"name": "Rocket plan", "goal": "Reach orbit", "workspace_id": wid});
    let (status, _) = call(&app, Method::POST, "/graphs", &owner, Some(graph)).await;
    assert_eq!(status, StatusCode::CREATED);
    let workspace: uuid::Uuid = wid.parse().unwrap();
    sqlx::query(
        "INSERT INTO documents (id, workspace_id, name, size_bytes, sha256, status)
         VALUES ($1, $2, 'rocket-manual.pdf', 10, 'abc', 'ready')",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(workspace)
    .execute(&app.state.db)
    .await
    .unwrap();

    // An owner finds all of it, kind by kind.
    assert_eq!(
        find(&app, &ws, &owner, "rocket").await,
        [
            hit("issue", "LEAD-1 Rocket budget"),
            hit("issue", "ENG-1 Rocket engine test"),
            hit("project", "Rocket launch"),
            hit("graph", "Rocket plan"),
            hit("document", "rocket-manual.pdf"),
        ]
    );
    // A member finds what they may open: not the private team's issue.
    assert_eq!(
        find(&app, &ws, &member, "ROCKET").await,
        [
            hit("issue", "ENG-1 Rocket engine test"),
            hit("project", "Rocket launch"),
            hit("graph", "Rocket plan"),
            hit("document", "rocket-manual.pdf"),
        ]
    );
    // A guest is on no team: no issues, no workspace graphs, no documents, no roster.
    let seen = find(&app, &ws, &guest, "rocket").await;
    assert!(
        seen.iter().all(|(kind, _)| kind == "project"),
        "a guest sees at most the project's name: {seen:?}"
    );
    assert_eq!(find(&app, &ws, &guest, "example.com").await, []);

    // By identifier, by team key, by e-mail.
    assert_eq!(
        find(&app, &ws, &member, "eng-1").await,
        [hit("issue", "ENG-1 Rocket engine test")]
    );
    assert_eq!(
        find(&app, &ws, &owner, "lead").await,
        [
            hit("issue", "LEAD-1 Rocket budget"),
            hit("team", "Leadership")
        ],
        "an owner sees the private team"
    );
    assert_eq!(find(&app, &ws, &member, "leader").await, []);
    assert_eq!(
        find(&app, &ws, &member, "owner@").await,
        [hit("member", "Test")]
    );

    // Too little to search for; a cap per kind; and nothing for people outside.
    assert_eq!(find(&app, &ws, &owner, "r").await, []);
    let capped = find(&app, &ws, &owner, "rocket&limit=1").await;
    assert_eq!(capped.iter().filter(|(kind, _)| kind == "issue").count(), 1);
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("{ws}/search?q=rocket"),
        &outsider,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&app, Method::GET, &format!("{ws}/search"), &owner, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "q is required");
}
