//! Transferring a workspace to another PostgreSQL database.
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode};
use common::TestApp;
use nexc::engine::transfer;
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

/// An empty database next to the test's own, and its URL.
async fn empty_database(pool: &PgPool) -> (String, String) {
    let name = format!("nexc_transfer_{}", uuid::Uuid::now_v7().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
        .execute(pool)
        .await
        .unwrap();
    let base = std::env::var("DATABASE_URL").unwrap();
    let (server, _) = base.rsplit_once('/').unwrap();
    (format!("{server}/{name}"), name)
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_workspace_moves_to_its_owners_database(pool: PgPool) {
    let app = TestApp::new(pool.clone(), &[]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let (member, _) = app.register("member@example.com").await;
    let (_, list) = call(&app, Method::GET, "/workspaces", &owner, None).await;
    let wid = list[0]["id"].as_str().unwrap().to_owned();
    let workspace: uuid::Uuid = wid.parse().unwrap();
    let ws = format!("/workspaces/{wid}");
    let invite = json!({"email": "member@example.com", "role": "admin"});
    call(
        &app,
        Method::POST,
        &format!("{ws}/members"),
        &owner,
        Some(invite),
    )
    .await;
    let (_, team) = call(
        &app,
        Method::POST,
        &format!("{ws}/teams"),
        &owner,
        Some(json!({"name": "Engineering", "key": "ENG"})),
    )
    .await;
    let (_, label) = call(
        &app,
        Method::POST,
        &format!("{ws}/labels"),
        &owner,
        Some(json!({"name": "Bug", "color": "#ef4444"})),
    )
    .await;
    let issues = format!("{ws}/teams/{}/issues", team["id"].as_str().unwrap());
    let (_, parent) = call(
        &app,
        Method::POST,
        &issues,
        &member,
        Some(json!({"title": "Ship login", "label_ids": [label["id"]]})),
    )
    .await;
    let part = json!({"title": "Form", "parent_id": parent["id"]});
    call(&app, Method::POST, &issues, &member, Some(part)).await;
    let comments = format!("/issues/{}/comments", parent["id"].as_str().unwrap());
    call(
        &app,
        Method::POST,
        &comments,
        &owner,
        Some(json!({"body": "Looks good"})),
    )
    .await;
    let graph = app
        .request(
            Method::POST,
            "/api/v1/graphs",
            Some(&owner),
            Some(json!({"name": "G"})),
        )
        .await;
    assert_eq!(graph.status, StatusCode::CREATED);

    // Only the owner starts or reads transfers; the target must be PostgreSQL.
    let transfers = format!("{ws}/transfers");
    let body = json!({"url": "postgres://u:p@127.0.0.1:9/none"});
    let (status, _) = call(&app, Method::POST, &transfers, &member, Some(body)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "an admin is not the owner");
    let (status, _) = call(
        &app,
        Method::POST,
        &transfers,
        &owner,
        Some(json!({"url": "mysql://x/y"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // A day summary is part of the workspace too: it travels with its points.
    let summary = format!("{ws}/timeline/summary");
    let (status, written) = call(&app, Method::POST, &summary, &owner, None).await;
    assert_eq!(status, StatusCode::OK, "{written}");

    let (url, name) = empty_database(&pool).await;
    let report = transfer::copy_workspace(&pool, &url, workspace)
        .await
        .unwrap();
    let copied = |table: &str| {
        let r = report.iter().find(|r| r.table == table).unwrap();
        (r.read, r.written)
    };
    assert_eq!(copied("workspaces"), (1, 1));
    assert_eq!(copied("users"), (2, 2));
    assert_eq!(copied("workspace_members"), (2, 2));
    assert_eq!(copied("issues"), (2, 2));
    assert_eq!(copied("issue_labels"), (1, 1));
    assert_eq!(copied("issue_events"), (1, 1));
    assert_eq!(copied("graphs"), (1, 1));
    assert_eq!(copied("day_summaries"), (1, 1));
    assert!(copied("issue_states").0 >= 6);
    assert!(report.iter().all(|r| r.read == r.written), "{report:?}");

    // On the target: the data with its ties, and no credentials.
    let target = nexc::repo::connect(&url, 2).await.unwrap();
    let (parents, sub_issues): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE parent_id IS NULL), count(*) FILTER (WHERE parent_id IS NOT NULL)
         FROM issues WHERE workspace_id = $1",
    )
    .bind(workspace)
    .fetch_one(&target)
    .await
    .unwrap();
    assert_eq!((parents, sub_issues), (1, 1));
    let (headline, highlights): (String, Vec<String>) =
        sqlx::query_as("SELECT headline, highlights FROM day_summaries WHERE workspace_id = $1")
            .bind(workspace)
            .fetch_one(&target)
            .await
            .unwrap();
    assert_eq!(Some(headline.as_str()), written["headline"].as_str());
    assert_eq!(json!(highlights), written["highlights"]);
    let hashes: Vec<String> = sqlx::query_scalar("SELECT password_hash FROM users")
        .fetch_all(&target)
        .await
        .unwrap();
    assert_eq!(
        hashes,
        ["!", "!"],
        "nobody's password hash leaves this server"
    );
    let other: i64 = sqlx::query_scalar("SELECT count(*) FROM workspaces WHERE id <> $1")
        .bind(workspace)
        .fetch_one(&target)
        .await
        .unwrap();
    assert_eq!(other, 0, "the member's own workspace stays behind");
    target.close().await;

    // A second transfer to the same place is refused; this server still has everything.
    let again = transfer::copy_workspace(&pool, &url, workspace).await;
    assert!(again.unwrap_err().to_string().contains("already holds"));
    let (_, still) = call(&app, Method::GET, &format!("{ws}/issues"), &owner, None).await;
    assert_eq!(still.as_array().unwrap().len(), 2);

    // Through the API the outcome is recorded with where the data went, never the password.
    let (url2, name2) = empty_database(&pool).await;
    let (status, started) = call(
        &app,
        Method::POST,
        &transfers,
        &owner,
        Some(json!({"url": url2})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    assert!(!started["target"].as_str().unwrap().contains('@'));
    let mut done = Value::Null;
    for _ in 0..100 {
        let (_, list) = call(&app, Method::GET, &transfers, &owner, None).await;
        if list[0]["status"] != "running" {
            done = list[0].clone();
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(done["status"], "done", "{done}");
    assert!(done["report"].as_array().unwrap().len() > 20);

    for database in [name, name2] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE {database} WITH (FORCE)"
        )))
        .execute(&pool)
        .await
        .unwrap();
    }
}

/// Tables that hang off a workspace and are deliberately not copied.
const NOT_COPIED: &[(&str, &str)] = &[
    (
        "realtime_tickets",
        "single-use tickets that live for seconds",
    ),
    (
        "workspace_llm_settings",
        "the workspace's AI credential, sealed with this server's key; it is entered again",
    ),
    (
        "workspace_transfers",
        "the record of the transfers themselves, which stays where they were made",
    ),
];

/// A transfer is how a workspace takes all of its data with it. Whatever
/// refers to a workspace, directly or through other tables, is copied, or
/// is named above with the reason why not: a table added later cannot be
/// left behind without anyone deciding so.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn everything_that_belongs_to_a_workspace_is_copied_or_exempt(pool: PgPool) {
    let belonging: Vec<String> = sqlx::query_scalar(
        "WITH RECURSIVE reach(tbl) AS (
             SELECT 'workspaces'::regclass
             UNION
             SELECT c.conrelid::regclass FROM pg_constraint c JOIN reach r ON c.confrelid = r.tbl
             WHERE c.contype = 'f' AND c.conrelid <> c.confrelid)
         SELECT tbl::text FROM reach ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(belonging.len() > 25, "the schema was read: {belonging:?}");
    let copied: Vec<&str> = transfer::tables().collect();
    let left_behind: Vec<&String> = belonging
        .iter()
        .filter(|t| !copied.contains(&t.as_str()))
        .filter(|t| !NOT_COPIED.iter().any(|(name, _)| name == t))
        .collect();
    assert!(
        left_behind.is_empty(),
        "not copied by a transfer and not exempt: {left_behind:?}"
    );
    for (name, reason) in NOT_COPIED {
        assert!(
            belonging.iter().any(|t| t == name) && !copied.contains(name),
            "{name} is listed as not copied ({reason}) but that no longer holds"
        );
    }
    for name in &copied {
        assert!(
            *name == "users" || belonging.iter().any(|t| t == name),
            "{name} is copied but does not belong to a workspace"
        );
    }
}
