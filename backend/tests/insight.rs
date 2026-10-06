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
    let issue = json!({"title": "Reset password", "due_date": today});
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
    // What surrounded the day reached the model too: a deadline, and the day's spending.
    let around = again["highlights"][1].as_str().unwrap();
    assert!(
        around.starts_with("Around: Due this day: ENG-2 Reset password ("),
        "{around}"
    );

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

/// The ties of a tie list as `(label, kind, count, first title)`.
fn ties(view: &Value) -> Vec<(String, String, i64, String)> {
    view["ties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                t["label"].as_str().unwrap().to_owned(),
                t["kind"].as_str().unwrap().to_owned(),
                t["count"].as_i64().unwrap(),
                t["items"][0]["title"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect()
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_thing_shows_what_it_is_tied_to(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let (member, _) = app.register("member@example.com").await;
    let me = |token: String| {
        let app = &app;
        async move {
            let (_, me) = call(app, Method::GET, "/auth/me", &token, None).await;
            me["id"].as_str().unwrap().to_owned()
        }
    };
    let (owner_id, member_id) = (me(owner.clone()).await, me(member.clone()).await);
    let (_, list) = call(&app, Method::GET, "/workspaces", &owner, None).await;
    let wid = list[0]["id"].as_str().unwrap().to_owned();
    let ws = format!("/workspaces/{wid}");
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
    let team_id = team["id"].as_str().unwrap().to_owned();
    let seat = json!({"role": "member"});
    let path = format!("{ws}/teams/{team_id}/members/{member_id}");
    let (status, _) = call(&app, Method::PUT, &path, &owner, Some(seat)).await;
    assert_eq!(status, StatusCode::OK);
    let project = json!({"name": "Launch"});
    let (_, project) = call(
        &app,
        Method::POST,
        &format!("{ws}/projects"),
        &owner,
        Some(project),
    )
    .await;
    let label = json!({"name": "Bug", "color": "#ef4444"});
    let (_, label) = call(
        &app,
        Method::POST,
        &format!("{ws}/labels"),
        &owner,
        Some(label),
    )
    .await;
    let issues = format!("{ws}/teams/{team_id}/issues");
    let parent = json!({"title": "Ship login", "assignee_id": member_id,
        "project_id": project["id"], "label_ids": [label["id"]]});
    let (status, parent) = call(&app, Method::POST, &issues, &owner, Some(parent)).await;
    assert_eq!(status, StatusCode::CREATED, "{parent}");
    let parent_id = parent["id"].as_str().unwrap().to_owned();
    let part = json!({"title": "Form", "parent_id": parent_id});
    call(&app, Method::POST, &issues, &member, Some(part)).await;
    let (status, planned) = call(
        &app,
        Method::POST,
        &format!("/issues/{parent_id}/graph"),
        &owner,
        None,
    )
    .await;
    assert!(status.is_success(), "{planned}");
    let map = format!("{ws}/map");

    // The things of a kind, by name, to pick one from.
    let (status, found) = call(&app, Method::GET, &format!("{map}/issue"), &owner, None).await;
    assert_eq!(status, StatusCode::OK, "{found}");
    let titles: Vec<&str> = found
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["ENG-1 Ship login", "ENG-2 Form"]);
    assert_eq!(found[0]["kind"], "issue");
    let (_, found) = call(
        &app,
        Method::GET,
        &format!("{map}/issue?q=form"),
        &owner,
        None,
    )
    .await;
    assert_eq!(found.as_array().unwrap().len(), 1);
    let (_, people) = call(
        &app,
        Method::GET,
        &format!("{map}/member?q=member@"),
        &owner,
        None,
    )
    .await;
    assert_eq!(people[0]["id"].as_str(), Some(member_id.as_str()));

    // An issue: its team, project, people, parts, graph and labels.
    let (status, view) = call(
        &app,
        Method::GET,
        &format!("{map}/issue/{parent_id}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["item"]["title"], "ENG-1 Ship login");
    let tie = |label: &str, kind: &str, count: i64, first: &str| {
        (label.to_owned(), kind.to_owned(), count, first.to_owned())
    };
    assert_eq!(
        ties(&view),
        [
            tie("belongs to", "team", 1, "Engineering"),
            tie("is part of", "project", 1, "Launch"),
            tie("is assigned to", "member", 1, "Test"),
            tie("was filed by", "member", 1, "Test"),
            tie("is split into", "issue", 1, "ENG-2 Form"),
            tie("is planned as", "graph", 1, "ENG-1 Ship login"),
            tie("is labelled", "label", 1, "Bug"),
        ],
        "ties to nothing (a parent, an agent, a cycle) are left out"
    );
    let assignee = &view["ties"][2]["items"][0];
    assert_eq!(assignee["id"].as_str(), Some(member_id.as_str()));

    // Followed on: the member it is assigned to, then the team.
    let (_, person) = call(
        &app,
        Method::GET,
        &format!("{map}/member/{member_id}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(person["item"]["subtitle"], "member@example.com");
    assert_eq!(
        ties(&person),
        [
            tie("is on", "team", 1, "Engineering"),
            tie("is assigned", "issue", 1, "ENG-1 Ship login"),
            tie("filed", "issue", 1, "ENG-2 Form"),
        ]
    );
    let (_, squad) = call(
        &app,
        Method::GET,
        &format!("{map}/team/{team_id}"),
        &owner,
        None,
    )
    .await;
    let reached = ties(&squad);
    assert_eq!(reached[0].0, "has");
    assert_eq!(
        reached[0].2, 2,
        "the owner who made it and the member: {squad}"
    );
    assert_eq!(reached[1], tie("tracks", "issue", 2, "ENG-1 Ship login"));
    assert_eq!(reached[2], tie("owns", "graph", 1, "ENG-1 Ship login"));
    // A graph's nodes and runs are counted, not listed.
    let graph_id = view["ties"][5]["items"][0]["id"].as_str().unwrap();
    let (_, graph) = call(
        &app,
        Method::GET,
        &format!("{map}/graph/{graph_id}"),
        &owner,
        None,
    )
    .await;
    assert!(
        ties(&graph).contains(&tie("plans", "issue", 1, "ENG-1 Ship login")),
        "{graph}"
    );

    // A document: who uploaded it, what it is about, and how many passages it has.
    let (document, topic) = (uuid::Uuid::now_v7(), uuid::Uuid::now_v7());
    let workspace: uuid::Uuid = wid.parse().unwrap();
    let uploader: uuid::Uuid = member_id.parse().unwrap();
    sqlx::query(
        "WITH d AS (
             INSERT INTO documents (id, workspace_id, name, size_bytes, sha256, status, uploaded_by)
             VALUES ($1, $2, 'customs.pdf', 10, 'abc', 'ready', $4)),
         k AS (
             INSERT INTO knowledge_topics (id, workspace_id, label, centroid, embedding_model)
             VALUES ($3, $2, 'customs · invoices', ''::bytea, 'test'))
         INSERT INTO document_chunks (id, document_id, workspace_id, ordinal, kind, content, topic_id)
         SELECT gen_random_uuid(), $1, $2, n, 'text', 'Goods under 150 EUR are exempt.', $3
         FROM generate_series(1, 2) n",
    )
    .bind(document)
    .bind(workspace)
    .bind(topic)
    .bind(uploader)
    .execute(&app.state.db)
    .await
    .unwrap();
    let (status, paper) = call(
        &app,
        Method::GET,
        &format!("{map}/document/{document}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{paper}");
    assert_eq!(
        ties(&paper),
        [
            tie("was uploaded by", "member", 1, "Test"),
            tie("is about", "topic", 1, "customs · invoices"),
            tie("has", "passage", 2, ""),
        ]
    );
    assert_eq!(paper["ties"][1]["items"][0]["subtitle"], "2 passages");
    let (_, person) = call(
        &app,
        Method::GET,
        &format!("{map}/member/{member_id}"),
        &owner,
        None,
    )
    .await;
    assert!(ties(&person).contains(&tie("uploaded", "document", 1, "customs.pdf")));

    // Admins and owners only; nothing of another workspace; only kinds there are.
    for path in [format!("{map}/issue"), format!("{map}/issue/{parent_id}")] {
        let (status, _) = call(&app, Method::GET, &path, &member, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
    }
    let (_, theirs) = call(&app, Method::GET, "/workspaces", &member, None).await;
    let other = theirs
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["role"] == "owner")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{other}/map/issue/{parent_id}"),
        &member,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "an issue of another workspace"
    );
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{other}/map/member/{owner_id}"),
        &member,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "someone who is not a member there"
    );
    let (status, _) = call(&app, Method::GET, &format!("{map}/galaxy"), &owner, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    for kind in [
        "member", "team", "project", "issue", "graph", "document", "agent",
    ] {
        let (status, body) = call(&app, Method::GET, &format!("{map}/{kind}"), &owner, None).await;
        assert_eq!(status, StatusCode::OK, "{kind}: {body}");
    }
    // Every tie of every kind runs: each thing of the workspace opens.
    for kind in [
        "member", "team", "project", "issue", "graph", "document", "agent",
    ] {
        let (_, things) = call(&app, Method::GET, &format!("{map}/{kind}"), &owner, None).await;
        let first = things[0]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("no {kind}"));
        let (status, body) = call(
            &app,
            Method::GET,
            &format!("{map}/{kind}/{first}"),
            &owner,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{kind}: {body}");
    }
}
