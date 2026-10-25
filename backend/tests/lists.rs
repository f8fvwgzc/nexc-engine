//! What the lists send and how much of it: issue lists without descriptions
//! but with the latest run's status, the unread count, a workspace's runs in
//! one request, a graph's latest runs, every file a page at a time, and the
//! sidebar's recent graphs.
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

async fn user(app: &TestApp, email: &str) -> (String, String) {
    let (token, _) = app.register(email).await;
    let me = app
        .request(Method::GET, "/api/v1/auth/me", Some(&token), None)
        .await;
    (token, me.body["id"].as_str().unwrap().to_owned())
}

/// A workspace with a team (its owner's) and a member who is not on the team.
struct World {
    wid: String,
    ws: String,
    team: String,
    owner: String,
    member: String,
    member_id: String,
    outsider: String,
}

async fn world(app: &TestApp) -> World {
    let (owner, _) = user(app, "owner@example.com").await;
    let (member, member_id) = user(app, "member@example.com").await;
    let (outsider, _) = user(app, "outsider@example.com").await;
    let (_, list) = call(app, Method::GET, "/workspaces", &owner, None).await;
    let wid = list[0]["id"].as_str().unwrap().to_owned();
    let ws = format!("/workspaces/{wid}");
    let invite = json!({"email": "member@example.com", "role": "member"});
    let (status, _) = call(
        app,
        Method::POST,
        &format!("{ws}/members"),
        &owner,
        Some(invite),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let body = json!({"name": "Engineering", "key": "ENG"});
    let (status, team) = call(
        app,
        Method::POST,
        &format!("{ws}/teams"),
        &owner,
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{team}");
    World {
        wid,
        ws,
        team: team["id"].as_str().unwrap().to_owned(),
        owner,
        member,
        member_id,
        outsider,
    }
}

async fn file_issue(app: &TestApp, w: &World, body: Value) -> Value {
    let path = format!("{}/teams/{}/issues", w.ws, w.team);
    let (status, issue) = call(app, Method::POST, &path, &w.owner, Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{issue}");
    issue
}

/// Gives the issue a graph of one step and runs it to the end; returns the graph id.
async fn plan_and_run(app: &TestApp, w: &World, iid: &str, runs: usize) -> String {
    let (status, linked) = call(
        app,
        Method::POST,
        &format!("/issues/{iid}/graph"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{linked}");
    let gid = linked["graph_id"].as_str().unwrap().to_owned();
    app.node(&w.owner, &gid, json!({"title": "Draft", "kind": "task"}))
        .await;
    for _ in 0..runs {
        let mut rx = app.events(&gid);
        let r = app
            .request(
                Method::POST,
                &format!("/api/v1/graphs/{gid}/runs"),
                Some(&w.owner),
                Some(json!({})),
            )
            .await;
        assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
        common::collect_until(&mut rx, "run.finished").await;
    }
    gid
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn issue_lists_leave_out_descriptions_and_carry_the_latest_run(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let parent = file_issue(
        &app,
        &w,
        json!({"title": "Ship it", "description": "# Long"}),
    )
    .await;
    let pid = parent["id"].as_str().unwrap().to_owned();
    assert_eq!(parent["run_status"], Value::Null, "no graph, no run");
    let (_, states) = call(
        &app,
        Method::GET,
        &format!("{}/teams/{}/states", w.ws, w.team),
        &w.owner,
        None,
    )
    .await;
    let done = states
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["category"] == "completed")
        .unwrap()["id"]
        .clone();
    for (title, state) in [("Part one", Some(done)), ("Part two", None)] {
        let mut body = json!({"title": title, "parent_id": pid});
        if let Some(state) = state {
            body["state_id"] = state;
        }
        file_issue(&app, &w, body).await;
    }
    plan_and_run(&app, &w, &pid, 1).await;

    let (status, list) = call(
        &app,
        Method::GET,
        &format!("{}/issues", w.ws),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 3);
    assert!(
        list.iter().all(|i| i.get("description").is_none()),
        "a list carries no descriptions"
    );
    let listed = list.iter().find(|i| i["id"] == pid.as_str()).unwrap();
    assert_eq!(listed["run_status"], "succeeded");
    assert_eq!(listed["sub_issues"], json!({"total": 2, "closed": 1}));
    assert!(
        list.iter()
            .filter(|i| i["id"] != pid.as_str())
            .all(|i| i["run_status"].is_null() && i["sub_issues"]["total"] == 0)
    );

    // The issue itself has its description, and the run's status as well.
    let (_, one) = call(&app, Method::GET, &format!("/issues/{pid}"), &w.owner, None).await;
    assert_eq!(one["description"], "# Long");
    assert_eq!(one["run_status"], "succeeded");

    // Sub-issues by their parent, as the issue page asks for them.
    let (_, subs) = call(
        &app,
        Method::GET,
        &format!("{}/issues?parent_id={pid}&limit=1", w.ws),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(subs.as_array().unwrap().len(), 1, "limit is honoured");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_workspace_lists_its_runs_in_one_request(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let issue = file_issue(&app, &w, json!({"title": "Run me"})).await;
    let gid = plan_and_run(&app, &w, issue["id"].as_str().unwrap(), 2).await;

    // A graph's runs, newest first, each with its node results; or only the latest.
    let (status, runs) = call(
        &app,
        Method::GET,
        &format!("/graphs/{gid}/runs"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{runs}");
    let runs = runs.as_array().unwrap().clone();
    assert_eq!(runs.len(), 2);
    assert!(runs[0]["created_at"].as_str() >= runs[1]["created_at"].as_str());
    assert!(
        runs.iter()
            .all(|r| r["node_runs"].as_array().unwrap().len() == 1)
    );
    let (_, latest) = call(
        &app,
        Method::GET,
        &format!("/graphs/{gid}/runs?limit=1"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(latest.as_array().unwrap().len(), 1);
    assert_eq!(latest[0]["id"], runs[0]["id"]);
    assert_eq!(latest[0]["node_runs"], runs[0]["node_runs"]);

    // The workspace's runs: the same rows with the graph's name, a page at a time.
    let (status, all) = call(&app, Method::GET, &format!("{}/runs", w.ws), &w.owner, None).await;
    assert_eq!(status, StatusCode::OK, "{all}");
    let all = all.as_array().unwrap();
    assert_eq!(
        all.iter().map(|r| r["id"].clone()).collect::<Vec<_>>(),
        runs.iter().map(|r| r["id"].clone()).collect::<Vec<_>>()
    );
    assert!(all.iter().all(|r| r["graph_name"] == "ENG-1 Run me"));
    assert_eq!(all[0]["node_runs"], runs[0]["node_runs"]);
    let first = runs[0]["id"].as_str().unwrap();
    let (_, next) = call(
        &app,
        Method::GET,
        &format!("{}/runs?limit=1&before={first}", w.ws),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(next.as_array().unwrap().len(), 1);
    assert_eq!(next[0]["id"], runs[1]["id"]);

    // The team's graph is the team's: a member off the team sees no runs, an outsider no workspace.
    let (status, theirs) = call(
        &app,
        Method::GET,
        &format!("{}/runs", w.ws),
        &w.member,
        None,
    )
    .await;
    assert_eq!((status, theirs), (StatusCode::OK, json!([])));
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("{}/runs", w.ws),
        &w.outsider,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_unread_count_follows_the_inbox(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let count = |token: String| {
        let (app, path) = (&app, format!("{}/inbox/unread-count", w.ws));
        async move { call(app, Method::GET, &path, &token, None).await }
    };
    assert_eq!(
        count(w.member.clone()).await,
        (StatusCode::OK, json!({"count": 0}))
    );
    for title in ["One", "Two"] {
        file_issue(
            &app,
            &w,
            json!({"title": title, "assignee_id": w.member_id}),
        )
        .await;
    }
    assert_eq!(count(w.member.clone()).await.1, json!({"count": 2}));
    let (_, inbox) = call(
        &app,
        Method::GET,
        &format!("{}/inbox", w.ws),
        &w.member,
        None,
    )
    .await;
    let first = inbox[0]["id"].clone();
    call(
        &app,
        Method::POST,
        &format!("{}/inbox/read", w.ws),
        &w.member,
        Some(json!({"ids": [first]})),
    )
    .await;
    assert_eq!(count(w.member.clone()).await.1, json!({"count": 1}));
    assert_eq!(count(w.owner.clone()).await.1, json!({"count": 0}));
    assert_eq!(count(w.outsider.clone()).await.0, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn every_file_comes_a_page_at_a_time(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let files = format!("/api/v1{}/artifact-files", w.ws);
    for name in ["a.md", "b.md", "c.md"] {
        let r = app
            .send_bytes(&format!("{files}?name={name}"), &w.owner, b"# x".to_vec())
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    }
    let names = |list: &Value| -> Vec<String> {
        list.as_array()
            .unwrap()
            .iter()
            .map(|f| f["name"].as_str().unwrap().to_owned())
            .collect()
    };
    let list = format!("{}/artifact-files", w.ws);
    let (_, page) = call(
        &app,
        Method::GET,
        &format!("{list}?limit=2"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(names(&page), ["c.md", "b.md"], "newest first");
    let last = &page[1];
    let cursor = format!(
        "{},{}",
        last["updated_at"].as_str().unwrap(),
        last["id"].as_str().unwrap()
    );
    let (_, rest) = call(
        &app,
        Method::GET,
        &format!("{list}?limit=2&before={}", cursor.replace('+', "%2B")),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(names(&rest), ["a.md"]);
    // A folder's files (here the top level's) all come at once, by name.
    let (_, top) = call(
        &app,
        Method::GET,
        &format!("{list}?root=true&limit=1"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(names(&top), ["a.md", "b.md", "c.md"]);
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("{list}?before=yesterday"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_sidebar_reads_only_its_recent_graphs(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    for name in ["First", "Second", "Third"] {
        let body = json!({"name": name, "goal": "g", "workspace_id": w.wid});
        let (status, _) = call(&app, Method::POST, "/graphs", &w.owner, Some(body)).await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (_, recent) = call(
        &app,
        Method::GET,
        &format!("/graphs?workspace_id={}&limit=2", w.wid),
        &w.owner,
        None,
    )
    .await;
    let names: Vec<&str> = recent
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Third", "Second"]);
    assert!(recent[0]["node_count"].is_number());
}
