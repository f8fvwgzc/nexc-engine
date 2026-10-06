//! Issues, workflow states and projects.
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode};
use common::TestApp;
use serde_json::{Value, json};
use sqlx::PgPool;

async fn user(app: &TestApp, email: &str) -> (String, String) {
    let (token, _) = app.register(email).await;
    let me = app
        .request(Method::GET, "/api/v1/auth/me", Some(&token), None)
        .await;
    (token, me.body["id"].as_str().unwrap().to_owned())
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

/// A workspace with a public and a private team, a member and a guest.
struct World {
    ws: String,
    eng: String,
    secret: String,
    owner: String,
    member: String,
    member_id: String,
    guest: String,
    guest_id: String,
    outsider: String,
}

async fn world(app: &TestApp) -> World {
    let (owner, _) = user(app, "owner@example.com").await;
    let (member, member_id) = user(app, "member@example.com").await;
    let (guest, guest_id) = user(app, "guest@example.com").await;
    let (outsider, _) = user(app, "outsider@example.com").await;
    let (_, list) = call(app, Method::GET, "/workspaces", &owner, None).await;
    let wid = list[0]["id"].as_str().unwrap().to_owned();
    let ws = format!("/workspaces/{wid}");
    for (email, role) in [
        ("member@example.com", "member"),
        ("guest@example.com", "guest"),
    ] {
        let body = json!({"email": email, "role": role});
        call(
            app,
            Method::POST,
            &format!("{ws}/members"),
            &owner,
            Some(body),
        )
        .await;
    }
    let team = |body: Value| {
        let (ws, owner) = (ws.clone(), owner.clone());
        async move {
            let (status, team) = call(
                app,
                Method::POST,
                &format!("{ws}/teams"),
                &owner,
                Some(body),
            )
            .await;
            assert_eq!(status, StatusCode::CREATED, "{team}");
            team["id"].as_str().unwrap().to_owned()
        }
    };
    let eng = team(json!({"name": "Engineering", "key": "ENG"})).await;
    let secret = team(json!({"name": "Leadership", "key": "LEAD", "private": true})).await;
    World {
        ws,
        eng,
        secret,
        owner,
        member,
        member_id,
        guest,
        guest_id,
        outsider,
    }
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn issues_are_numbered_per_team_and_follow_team_visibility(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let eng_issues = format!("{}/teams/{}/issues", w.ws, w.eng);
    let secret_issues = format!("{}/teams/{}/issues", w.ws, w.secret);

    // A member files with a public team they have not joined; numbers count up per team.
    let (status, first) = call(
        &app,
        Method::POST,
        &eng_issues,
        &w.member,
        Some(json!({"title": " Fix login "})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    assert_eq!(
        (first["identifier"].as_str(), first["title"].as_str()),
        (Some("ENG-1"), Some("Fix login"))
    );
    assert_eq!(
        (
            first["state"]["name"].as_str(),
            first["state"]["category"].as_str()
        ),
        (Some("Todo"), Some("unstarted"))
    );
    assert_eq!(
        (
            first["priority"].as_i64(),
            &first["assignee"],
            &first["completed_at"]
        ),
        (Some(0), &Value::Null, &Value::Null)
    );
    let (_, second) = call(
        &app,
        Method::POST,
        &eng_issues,
        &w.owner,
        Some(json!({"title": "Ship billing", "priority": 1, "assignee_id": w.member_id})),
    )
    .await;
    assert_eq!(second["identifier"], "ENG-2");
    assert_eq!(second["assignee"]["name"], "Test");
    let (_, hidden) = call(
        &app,
        Method::POST,
        &secret_issues,
        &w.owner,
        Some(json!({"title": "Reorg"})),
    )
    .await;
    assert_eq!(
        hidden["identifier"], "LEAD-1",
        "each team has its own sequence"
    );

    // Who may file: not guests outside their teams, not members with a private team.
    assert_eq!(
        call(
            &app,
            Method::POST,
            &eng_issues,
            &w.guest,
            Some(json!({"title": "x"}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            Method::POST,
            &secret_issues,
            &w.member,
            Some(json!({"title": "x"}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    for bad in [
        json!({"title": ""}),
        json!({"title": "x", "priority": 9}),
        json!({"title": "x", "assignee_id": w.guest_id, "state_id": w.eng}),
    ] {
        assert_eq!(
            call(&app, Method::POST, &eng_issues, &w.owner, Some(bad))
                .await
                .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let (_, outsider_id) = user(&app, "nobody@example.com").await;
    assert_eq!(
        call(
            &app,
            Method::POST,
            &eng_issues,
            &w.owner,
            Some(json!({"title": "x", "assignee_id": outsider_id}))
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY,
        "assignees are workspace members"
    );

    // Lists show what the caller's teams allow, and filter.
    let ids = |token: String, query: &'static str| {
        let (app, ws) = (&app, w.ws.clone());
        async move {
            let (status, list) = call(
                app,
                Method::GET,
                &format!("{ws}/issues{query}"),
                &token,
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{list}");
            let mut ids: Vec<String> = list
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i["identifier"].as_str().unwrap().to_owned())
                .collect();
            ids.sort();
            ids
        }
    };
    assert_eq!(ids(w.owner.clone(), "").await, ["ENG-1", "ENG-2", "LEAD-1"]);
    assert_eq!(
        ids(w.member.clone(), "").await,
        ["ENG-1", "ENG-2"],
        "the private team stays hidden"
    );
    assert!(ids(w.guest.clone(), "").await.is_empty());
    assert_eq!(
        call(
            &app,
            Method::GET,
            &format!("{}/issues", w.ws),
            &w.outsider,
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(ids(w.owner.clone(), "?q=billing").await, ["ENG-2"]);
    assert_eq!(
        ids(w.owner.clone(), "?q=lead-").await,
        ["LEAD-1"],
        "identifiers are searchable"
    );
    let hidden_path = format!("/issues/{}", hidden["id"].as_str().unwrap());
    assert_eq!(
        call(&app, Method::GET, &hidden_path, &w.member, None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );

    // A guest added to a team works on its issues like anyone else.
    let add = format!("{}/teams/{}/members/{}", w.ws, w.eng, w.guest_id);
    call(&app, Method::PUT, &add, &w.owner, Some(json!({}))).await;
    assert_eq!(ids(w.guest.clone(), "").await, ["ENG-1", "ENG-2"]);
    let first_path = format!("/issues/{}", first["id"].as_str().unwrap());
    let (status, edited) = call(
        &app,
        Method::PATCH,
        &first_path,
        &w.guest,
        Some(json!({"priority": 2, "assignee_id": w.guest_id})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(
        (
            edited["priority"].as_i64(),
            edited["assignee"]["user_id"].as_str()
        ),
        (Some(2), Some(w.guest_id.as_str()))
    );

    // Closing stamps completed_at, reopening clears it, null clears the assignee.
    let (_, states) = call(
        &app,
        Method::GET,
        &format!("{}/teams/{}/states", w.ws, w.eng),
        &w.member,
        None,
    )
    .await;
    let state = |name: &str| {
        states
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == name)
            .unwrap()["id"]
            .clone()
    };
    let (_, done) = call(
        &app,
        Method::PATCH,
        &first_path,
        &w.member,
        Some(json!({"state_id": state("Done"), "assignee_id": null})),
    )
    .await;
    assert!(done["completed_at"].is_string());
    assert_eq!(
        (done["state"]["category"].as_str(), &done["assignee"]),
        (Some("completed"), &Value::Null)
    );
    assert_eq!(
        ids(w.owner.clone(), "?open=true").await,
        ["ENG-2", "LEAD-1"]
    );
    let (_, reopened) = call(
        &app,
        Method::PATCH,
        &first_path,
        &w.member,
        Some(json!({"state_id": state("In Progress")})),
    )
    .await;
    assert_eq!(reopened["completed_at"], Value::Null);
    // A state of another team is not a valid target.
    let (_, lead_states) = call(
        &app,
        Method::GET,
        &format!("{}/teams/{}/states", w.ws, w.secret),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(
        call(
            &app,
            Method::PATCH,
            &first_path,
            &w.owner,
            Some(json!({"state_id": lead_states[0]["id"]}))
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    // Deleting an issue does not give its number back.
    assert_eq!(
        call(&app, Method::DELETE, &first_path, &w.member, None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let (_, third) = call(
        &app,
        Method::POST,
        &eng_issues,
        &w.member,
        Some(json!({"title": "Next"})),
    )
    .await;
    assert_eq!(third["identifier"], "ENG-3");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn workflows_and_projects_are_data(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let states_path = format!("{}/teams/{}/states", w.ws, w.eng);
    let (_, states) = call(&app, Method::GET, &states_path, &w.member, None).await;
    let names: Vec<&str> = states
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "Backlog",
            "Todo",
            "In Progress",
            "In Review",
            "Done",
            "Canceled"
        ],
        "the starter workflow"
    );

    // Only team owners and workspace admins reshape the workflow.
    let qa = json!({"name": "QA", "category": "started", "color": "#a855f7"});
    assert_eq!(
        call(
            &app,
            Method::POST,
            &states_path,
            &w.member,
            Some(qa.clone())
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, created) =
        call(&app, Method::POST, &states_path, &w.owner, Some(qa.clone())).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["position"], 6, "new states go last");
    assert_eq!(
        call(&app, Method::POST, &states_path, &w.owner, Some(qa))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            Method::POST,
            &states_path,
            &w.owner,
            Some(json!({"name": "X", "category": "started", "color": "purple"}))
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let qa_path = format!("{states_path}/{}", created["id"].as_str().unwrap());
    let (_, renamed) = call(
        &app,
        Method::PATCH,
        &qa_path,
        &w.owner,
        Some(json!({"name": "Verification", "position": 3})),
    )
    .await;
    assert_eq!(
        (renamed["name"].as_str(), renamed["position"].as_i64()),
        (Some("Verification"), Some(3))
    );

    // A state with issues cannot go; an empty one can.
    let issues = format!("{}/teams/{}/issues", w.ws, w.eng);
    let (_, issue) = call(
        &app,
        Method::POST,
        &issues,
        &w.member,
        Some(json!({"title": "Check", "state_id": created["id"]})),
    )
    .await;
    assert_eq!(issue["state"]["name"], "Verification");
    assert_eq!(
        call(&app, Method::DELETE, &qa_path, &w.owner, None).await.0,
        StatusCode::CONFLICT
    );
    let todo = states
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "Todo")
        .unwrap()["id"]
        .clone();
    let issue_path = format!("/issues/{}", issue["id"].as_str().unwrap());
    call(
        &app,
        Method::PATCH,
        &issue_path,
        &w.member,
        Some(json!({"state_id": todo})),
    )
    .await;
    assert_eq!(
        call(&app, Method::DELETE, &qa_path, &w.owner, None).await.0,
        StatusCode::NO_CONTENT
    );

    // Projects span teams; counts cover only the issues the caller can see.
    let projects = format!("{}/projects", w.ws);
    assert_eq!(
        call(
            &app,
            Method::POST,
            &projects,
            &w.guest,
            Some(json!({"name": "Q4"}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, project) = call(
        &app,
        Method::POST,
        &projects,
        &w.member,
        Some(json!({"name": "Q4 launch", "target_date": "2026-12-15"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{project}");
    assert_eq!(
        (
            project["status"].as_str(),
            project["lead_id"].as_str(),
            project["target_date"].as_str()
        ),
        (
            Some("planned"),
            Some(w.member_id.as_str()),
            Some("2026-12-15")
        )
    );
    let pid = project["id"].as_str().unwrap().to_owned();
    call(
        &app,
        Method::PATCH,
        &issue_path,
        &w.member,
        Some(json!({"project_id": pid})),
    )
    .await;
    let secret_issues = format!("{}/teams/{}/issues", w.ws, w.secret);
    let done = {
        let (_, lead_states) = call(
            &app,
            Method::GET,
            &format!("{}/teams/{}/states", w.ws, w.secret),
            &w.owner,
            None,
        )
        .await;
        lead_states
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "Done")
            .unwrap()["id"]
            .clone()
    };
    call(
        &app,
        Method::POST,
        &secret_issues,
        &w.owner,
        Some(json!({"title": "Hidden", "project_id": pid, "state_id": done})),
    )
    .await;
    let counts = |token: String| {
        let (app, projects) = (&app, projects.clone());
        async move {
            let (_, list) = call(app, Method::GET, &projects, &token, None).await;
            (
                list[0]["issue_count"].as_i64().unwrap(),
                list[0]["closed_count"].as_i64().unwrap(),
            )
        }
    };
    assert_eq!(counts(w.owner.clone()).await, (2, 1));
    assert_eq!(
        counts(w.member.clone()).await,
        (1, 0),
        "the private team's issue is not counted"
    );
    let (_, list) = call(
        &app,
        Method::GET,
        &format!("{}/issues?project_id={pid}", w.ws),
        &w.member,
        None,
    )
    .await;
    assert_eq!(list.as_array().unwrap().len(), 1);

    let project_path = format!("{projects}/{pid}");
    let (_, started) = call(
        &app,
        Method::PATCH,
        &project_path,
        &w.member,
        Some(json!({"status": "started", "target_date": null})),
    )
    .await;
    assert_eq!(
        (started["status"].as_str(), &started["target_date"]),
        (Some("started"), &Value::Null)
    );
    assert_eq!(
        call(&app, Method::DELETE, &project_path, &w.member, None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, Method::DELETE, &project_path, &w.owner, None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let (_, kept) = call(&app, Method::GET, &issue_path, &w.member, None).await;
    assert_eq!(
        kept["project_id"],
        Value::Null,
        "issues outlive their project"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_issue_can_be_planned_and_run_as_a_graph(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let issues = format!("{}/teams/{}/issues", w.ws, w.eng);
    let body = json!({"title": "Research gold drivers", "description": "Cover real yields and the dollar."});
    let (_, issue) = call(&app, Method::POST, &issues, &w.member, Some(body)).await;
    let graph_path = format!("/issues/{}/graph", issue["id"].as_str().unwrap());

    // Graphs of a team belong to its members, so the member joins first.
    assert_eq!(
        call(&app, Method::POST, &graph_path, &w.member, None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let join = format!("{}/teams/{}/members/{}", w.ws, w.eng, w.member_id);
    assert_eq!(
        call(&app, Method::PUT, &join, &w.member, Some(json!({})))
            .await
            .0,
        StatusCode::OK
    );
    let (status, linked) = call(&app, Method::POST, &graph_path, &w.member, None).await;
    assert_eq!(status, StatusCode::OK, "{linked}");
    let gid = linked["graph_id"]
        .as_str()
        .expect("the issue now has a graph")
        .to_owned();

    let (_, graph) = call(&app, Method::GET, &format!("/graphs/{gid}"), &w.owner, None).await;
    assert_eq!(graph["name"], "ENG-1 Research gold drivers");
    assert_eq!(graph["team_id"], w.eng.as_str());
    assert!(graph["goal"].as_str().unwrap().contains("real yields"));
    // Asking again does not create a second graph.
    let (_, again) = call(&app, Method::POST, &graph_path, &w.owner, None).await;
    assert_eq!(again["graph_id"], gid.as_str());
    // Deleting the graph leaves the issue, unlinked.
    assert_eq!(
        call(
            &app,
            Method::DELETE,
            &format!("/graphs/{gid}"),
            &w.member,
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (_, issue) = call(
        &app,
        Method::GET,
        &format!("/issues/{}", issue["id"].as_str().unwrap()),
        &w.member,
        None,
    )
    .await;
    assert_eq!(issue["graph_id"], Value::Null);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_assistant_files_issues_with_the_callers_rights(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let ask = format!("{}/assistant", w.ws);

    // A plain question files nothing.
    let (status, answer) = call(
        &app,
        Method::POST,
        &ask,
        &w.member,
        Some(json!({"message": "What is open?"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(
        (
            answer["reply"].as_str(),
            answer["created"].as_array().map(Vec::len)
        ),
        (Some("Here you go."), Some(0))
    );

    // Asked to, it files with a team the member can file with and reports what it could not place.
    let body = json!({"message": "Please file an issue for the login bug",
        "history": [{"role": "user", "content": "hi"}, {"role": "assistant", "content": "hello"}]});
    let (status, answer) = call(&app, Method::POST, &ask, &w.member, Some(body.clone())).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let created = answer["created"].as_array().unwrap();
    assert_eq!(created.len(), 1);
    assert_eq!(
        (
            created[0]["identifier"].as_str(),
            created[0]["priority"].as_i64()
        ),
        (Some("ENG-1"), Some(2))
    );
    assert_eq!(
        created[0]["creator_id"],
        w.member_id.as_str(),
        "filed as the member"
    );
    assert_eq!(answer["skipped"], json!(["NOPE: Nowhere"]));

    // A guest in no team has nowhere to file: the same request creates nothing.
    let (_, answer) = call(&app, Method::POST, &ask, &w.guest, Some(body)).await;
    assert_eq!(answer["created"], json!([]));
    assert_eq!(answer["skipped"].as_array().unwrap().len(), 2);

    // Its calls are on the ledger, it obeys guardrails, and strangers cannot reach it.
    let (_, report) = call(
        &app,
        Method::GET,
        &format!("{}/usage", w.ws),
        &w.owner,
        None,
    )
    .await;
    let assistant = report["by_purpose"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == "assistant")
        .unwrap();
    assert_eq!(assistant["calls"], 3);
    call(
        &app,
        Method::PUT,
        &format!("{}/guardrails", w.ws),
        &w.owner,
        Some(json!({"monthly_token_budget": 1})),
    )
    .await;
    assert_eq!(
        call(
            &app,
            Method::POST,
            &ask,
            &w.member,
            Some(json!({"message": "hi"}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            Method::POST,
            &ask,
            &w.outsider,
            Some(json!({"message": "hi"}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            Method::POST,
            &ask,
            &w.owner,
            Some(json!({"message": " "}))
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_issue_keeps_a_timeline_of_comments_and_changes(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let (_, issue) = call(
        &app,
        Method::POST,
        &format!("{}/teams/{}/issues", w.ws, w.eng),
        &w.member,
        Some(json!({"title": "Fix login"})),
    )
    .await;
    let path = format!("/issues/{}", issue["id"].as_str().unwrap());
    let events = format!("{path}/events");
    let comments = format!("{path}/comments");

    // A new issue has an empty timeline; an edit that changes nothing adds nothing.
    let (status, list) = call(&app, Method::GET, &events, &w.member, None).await;
    assert_eq!(
        (status, list.as_array().unwrap().len()),
        (StatusCode::OK, 0)
    );
    let same = json!({"title": "Fix login", "priority": 0, "description": "more detail"});
    call(&app, Method::PATCH, &path, &w.member, Some(same)).await;
    let (_, list) = call(&app, Method::GET, &events, &w.member, None).await;
    assert_eq!(list.as_array().unwrap().len(), 0, "{list}");

    // One edit of state, priority and assignee is three entries, by the editor.
    let (_, states) = call(
        &app,
        Method::GET,
        &format!("{}/teams/{}/states", w.ws, w.eng),
        &w.owner,
        None,
    )
    .await;
    let done = states
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "Done")
        .unwrap()["id"]
        .clone();
    let edit = json!({"state_id": done, "priority": 1, "assignee_id": w.member_id});
    let (status, body) = call(&app, Method::PATCH, &path, &w.owner, Some(edit)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, list) = call(&app, Method::GET, &events, &w.member, None).await;
    let seen: Vec<(&str, Option<&str>, Option<&str>)> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["kind"].as_str().unwrap(),
                e["from"].as_str(),
                e["to"].as_str(),
            )
        })
        .collect();
    let assignee = body["assignee"]["name"].as_str();
    assert_eq!(
        seen,
        vec![
            ("state", Some("Todo"), Some("Done")),
            ("priority", Some("No priority"), Some("Urgent")),
            ("assignee", None, assignee),
        ]
    );
    assert!(assignee.is_some_and(|name| !name.is_empty()));
    assert!(list[0]["actor"]["name"].is_string() && list[0]["body"] == "");

    // Comments: trimmed, not empty, shown in order after the changes.
    let (status, problem) = call(
        &app,
        Method::POST,
        &comments,
        &w.member,
        Some(json!({"body": "  "})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    let (status, comment) = call(
        &app,
        Method::POST,
        &comments,
        &w.member,
        Some(json!({"body": " Reproduced on staging. "})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{comment}");
    assert_eq!(
        (
            comment["kind"].as_str(),
            comment["body"].as_str(),
            comment["actor"]["user_id"].as_str(),
            &comment["edited_at"]
        ),
        (
            Some("comment"),
            Some("Reproduced on staging."),
            Some(w.member_id.as_str()),
            &Value::Null
        )
    );
    let one = format!("{comments}/{}", comment["id"].as_str().unwrap());

    // Only the author edits; the edit is stamped.
    let edit = json!({"body": "Reproduced on staging and locally."});
    let (status, _) = call(&app, Method::PATCH, &one, &w.owner, Some(edit.clone())).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, edited) = call(&app, Method::PATCH, &one, &w.member, Some(edit)).await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert!(edited["edited_at"].is_string());
    assert_eq!(edited["body"], "Reproduced on staging and locally.");

    // A change entry is not a comment, so it cannot be edited or deleted.
    let change = format!("{comments}/{}", list[0]["id"].as_str().unwrap());
    let (status, _) = call(&app, Method::DELETE, &change, &w.owner, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Outsiders and guests outside the team see no issue, so no timeline.
    for token in [&w.outsider, &w.guest] {
        let (status, _) = call(&app, Method::GET, &events, token, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let body = json!({"body": "hello"});
        let (status, _) = call(&app, Method::POST, &comments, token, Some(body)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    // A second comment by the owner: the member cannot delete it, the owner
    // (who manages the team) can delete anyone's.
    let (_, second) = call(
        &app,
        Method::POST,
        &comments,
        &w.owner,
        Some(json!({"body": "Thanks"})),
    )
    .await;
    let two = format!("{comments}/{}", second["id"].as_str().unwrap());
    let (status, _) = call(&app, Method::DELETE, &two, &w.member, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&app, Method::DELETE, &one, &w.owner, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, list) = call(&app, Method::GET, &events, &w.member, None).await;
    let kinds: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["state", "priority", "assignee", "comment"]);
    assert_eq!(list[3]["body"], "Thanks");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn labels_belong_to_the_workspace_and_filter_issues(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let labels = format!("{}/labels", w.ws);

    // Members add labels; guests and outsiders do not; names are unique ignoring case.
    let bug = json!({"name": " Bug ", "color": "#ef4444"});
    let (status, _) = call(&app, Method::POST, &labels, &w.guest, Some(bug.clone())).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&app, Method::POST, &labels, &w.outsider, Some(bug.clone())).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, bug) = call(&app, Method::POST, &labels, &w.member, Some(bug)).await;
    assert_eq!(status, StatusCode::CREATED, "{bug}");
    assert_eq!(bug["name"], "Bug");
    let again = json!({"name": "bug", "color": "#000000"});
    let (status, _) = call(&app, Method::POST, &labels, &w.member, Some(again)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let bad = json!({"name": "UI", "color": "red"});
    let (status, _) = call(&app, Method::POST, &labels, &w.member, Some(bad)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let ui = json!({"name": "UI", "color": "#0ea5e9"});
    let (_, ui) = call(&app, Method::POST, &labels, &w.member, Some(ui)).await;
    let (bug_id, ui_id) = (bug["id"].as_str().unwrap(), ui["id"].as_str().unwrap());

    // An issue is filed with labels and lists them by name.
    let issues = format!("{}/teams/{}/issues", w.ws, w.eng);
    let body = json!({"title": "Button overlaps", "label_ids": [ui_id, bug_id, ui_id]});
    let (status, issue) = call(&app, Method::POST, &issues, &w.member, Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{issue}");
    let names = |issue: &Value| -> Vec<String> {
        issue["labels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["name"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(names(&issue), ["Bug", "UI"]);
    let (_, plain) = call(
        &app,
        Method::POST,
        &issues,
        &w.member,
        Some(json!({"title": "Write docs"})),
    )
    .await;
    assert_eq!(names(&plain), Vec::<String>::new());

    // A label of another workspace is refused.
    let (_, theirs) = call(&app, Method::GET, "/workspaces", &w.outsider, None).await;
    let other = format!("/workspaces/{}/labels", theirs[0]["id"].as_str().unwrap());
    let foreign = json!({"name": "Theirs", "color": "#111111"});
    let (_, foreign) = call(&app, Method::POST, &other, &w.outsider, Some(foreign)).await;
    let path = format!("/issues/{}", issue["id"].as_str().unwrap());
    let body = json!({"label_ids": [foreign["id"]]});
    let (status, problem) = call(&app, Method::PATCH, &path, &w.member, Some(body)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");

    // PATCH replaces the set; leaving the field out keeps it.
    let body = json!({"label_ids": [ui_id]});
    let (_, issue) = call(&app, Method::PATCH, &path, &w.member, Some(body)).await;
    assert_eq!(names(&issue), ["UI"]);
    let body = json!({"priority": 2});
    let (_, issue) = call(&app, Method::PATCH, &path, &w.member, Some(body)).await;
    assert_eq!(names(&issue), ["UI"]);

    // The list filters by label.
    let (_, list) = call(
        &app,
        Method::GET,
        &format!("{}/issues?label_id={ui_id}", w.ws),
        &w.member,
        None,
    )
    .await;
    assert_eq!(list.as_array().unwrap().len(), 1, "{list}");
    assert_eq!(list[0]["title"], "Button overlaps");
    let (_, list) = call(
        &app,
        Method::GET,
        &format!("{}/issues?label_id={bug_id}", w.ws),
        &w.member,
        None,
    )
    .await;
    assert_eq!(list.as_array().unwrap().len(), 0);

    // Renaming shows on the issue; only admins delete, and the issue loses the label.
    let one = format!("{labels}/{ui_id}");
    let body = json!({"name": "Interface"});
    let (status, _) = call(&app, Method::PATCH, &one, &w.member, Some(body)).await;
    assert_eq!(status, StatusCode::OK);
    let (_, issue) = call(&app, Method::GET, &path, &w.guest, None).await;
    assert!(
        issue["labels"].is_null(),
        "guests outside the team see no issue"
    );
    let (_, issue) = call(&app, Method::GET, &path, &w.member, None).await;
    assert_eq!(names(&issue), ["Interface"]);
    let (status, _) = call(&app, Method::DELETE, &one, &w.member, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&app, Method::DELETE, &one, &w.owner, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, issue) = call(&app, Method::GET, &path, &w.member, None).await;
    assert_eq!(names(&issue), Vec::<String>::new());
    let (_, all) = call(&app, Method::GET, &labels, &w.guest, None).await;
    assert_eq!(all.as_array().unwrap().len(), 1);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_inbox_tells_members_about_their_issues(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let inbox = format!("{}/inbox", w.ws);
    let kinds = |list: &Value| -> Vec<String> {
        list.as_array()
            .unwrap()
            .iter()
            .map(|n| n["kind"].as_str().unwrap().to_owned())
            .collect()
    };

    // The owner files an issue for the member: the member is told, the owner is not.
    let body = json!({"title": "Fix login", "assignee_id": w.member_id});
    let (status, issue) = call(
        &app,
        Method::POST,
        &format!("{}/teams/{}/issues", w.ws, w.eng),
        &w.owner,
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{issue}");
    let path = format!("/issues/{}", issue["id"].as_str().unwrap());
    let (_, mine) = call(&app, Method::GET, &inbox, &w.member, None).await;
    assert_eq!(kinds(&mine), ["assigned"]);
    assert_eq!(
        (
            mine[0]["issue"]["identifier"].as_str(),
            mine[0]["issue"]["title"].as_str(),
            &mine[0]["read_at"]
        ),
        (Some("ENG-1"), Some("Fix login"), &Value::Null)
    );
    assert!(mine[0]["actor"]["name"].is_string());
    let (_, theirs) = call(&app, Method::GET, &inbox, &w.owner, None).await;
    assert_eq!(kinds(&theirs), Vec::<String>::new());

    // The member comments and moves it: the owner (creator) hears of both, the member of neither.
    let comment = json!({"body": "On it"});
    call(
        &app,
        Method::POST,
        &format!("{path}/comments"),
        &w.member,
        Some(comment),
    )
    .await;
    let (_, states) = call(
        &app,
        Method::GET,
        &format!("{}/teams/{}/states", w.ws, w.eng),
        &w.owner,
        None,
    )
    .await;
    let done = states
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "Done")
        .unwrap()["id"]
        .clone();
    let edit = json!({"state_id": done, "priority": 2});
    call(&app, Method::PATCH, &path, &w.member, Some(edit)).await;
    let (_, theirs) = call(&app, Method::GET, &inbox, &w.owner, None).await;
    assert_eq!(kinds(&theirs), ["state", "comment"]);
    let (_, mine) = call(&app, Method::GET, &inbox, &w.member, None).await;
    assert_eq!(kinds(&mine), ["assigned"]);

    // Reading one, then all; nobody reads another member's inbox.
    let first = json!({"ids": [theirs[0]["id"]]});
    let read = format!("{inbox}/read");
    let (_, after) = call(&app, Method::POST, &read, &w.member, Some(first.clone())).await;
    assert_eq!(
        kinds(&after),
        ["assigned"],
        "the member's own inbox comes back"
    );
    let (_, theirs) = call(&app, Method::GET, &inbox, &w.owner, None).await;
    assert!(
        theirs[0]["read_at"].is_null(),
        "another member cannot mark it"
    );
    let (_, theirs) = call(&app, Method::POST, &read, &w.owner, Some(first)).await;
    assert!(theirs[0]["read_at"].is_string() && theirs[1]["read_at"].is_null());
    let (_, theirs) = call(&app, Method::POST, &read, &w.owner, Some(json!({}))).await;
    assert!(theirs[1]["read_at"].is_string());

    // Unassigning tells nobody; outsiders have no inbox here.
    let unassign = json!({"assignee_id": null});
    call(&app, Method::PATCH, &path, &w.owner, Some(unassign)).await;
    let (_, mine) = call(&app, Method::GET, &inbox, &w.member, None).await;
    assert_eq!(kinds(&mine), ["assigned"]);
    let (status, _) = call(&app, Method::GET, &inbox, &w.outsider, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn issues_can_be_split_into_sub_issues(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let eng = format!("{}/teams/{}/issues", w.ws, w.eng);
    let file = |token: String, path: String, body: Value| {
        let app = &app;
        async move {
            let (status, issue) = call(app, Method::POST, &path, &token, Some(body)).await;
            assert_eq!(status, StatusCode::CREATED, "{issue}");
            issue
        }
    };
    let parent = file(
        w.member.clone(),
        eng.clone(),
        json!({"title": "Ship login"}),
    )
    .await;
    let pid = parent["id"].as_str().unwrap();
    assert!(parent["parent"].is_null());
    assert_eq!(parent["sub_issues"], json!({"total": 0, "closed": 0}));

    // Two parts; one is closed. The parent counts them, the parts name it.
    let a = file(
        w.member.clone(),
        eng.clone(),
        json!({"title": "Form", "parent_id": pid}),
    )
    .await;
    let b = file(
        w.member.clone(),
        eng.clone(),
        json!({"title": "Session", "parent_id": pid}),
    )
    .await;
    assert_eq!(
        (
            a["parent"]["identifier"].as_str(),
            a["parent"]["title"].as_str()
        ),
        (Some("ENG-1"), Some("Ship login"))
    );
    let (_, states) = call(
        &app,
        Method::GET,
        &format!("{}/teams/{}/states", w.ws, w.eng),
        &w.owner,
        None,
    )
    .await;
    let done = states
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "Done")
        .unwrap()["id"]
        .clone();
    let a_path = format!("/issues/{}", a["id"].as_str().unwrap());
    let b_path = format!("/issues/{}", b["id"].as_str().unwrap());
    let p_path = format!("/issues/{pid}");
    call(
        &app,
        Method::PATCH,
        &a_path,
        &w.member,
        Some(json!({"state_id": done})),
    )
    .await;
    let (_, parent) = call(&app, Method::GET, &p_path, &w.member, None).await;
    assert_eq!(parent["sub_issues"], json!({"total": 2, "closed": 1}));
    let (_, parts) = call(
        &app,
        Method::GET,
        &format!("{}/issues?parent_id={pid}", w.ws),
        &w.member,
        None,
    )
    .await;
    assert_eq!(parts.as_array().unwrap().len(), 2, "{parts}");

    // No loops: not itself, not under its own part.
    for (path, parent_id) in [(&p_path, pid), (&p_path, a["id"].as_str().unwrap())] {
        let body = json!({"parent_id": parent_id});
        let (status, problem) = call(&app, Method::PATCH, path, &w.member, Some(body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    }
    // Moving a part under another part is fine; other edits keep the parent.
    let body = json!({"parent_id": a["id"]});
    let (status, moved) = call(&app, Method::PATCH, &b_path, &w.member, Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["parent"]["title"], "Form");
    let (_, moved) = call(
        &app,
        Method::PATCH,
        &b_path,
        &w.member,
        Some(json!({"priority": 1})),
    )
    .await;
    assert_eq!(moved["parent"]["title"], "Form");
    let (_, moved) = call(
        &app,
        Method::PATCH,
        &b_path,
        &w.member,
        Some(json!({"parent_id": null})),
    )
    .await;
    assert!(moved["parent"].is_null());

    // An unknown parent, or one in another workspace, is refused.
    let (_, theirs) = call(&app, Method::GET, "/workspaces", &w.outsider, None).await;
    let other = format!("/workspaces/{}", theirs[0]["id"].as_str().unwrap());
    let (_, team) = call(
        &app,
        Method::POST,
        &format!("{other}/teams"),
        &w.outsider,
        Some(json!({"name": "Other", "key": "OTH"})),
    )
    .await;
    let foreign = file(
        w.outsider.clone(),
        format!("{other}/teams/{}/issues", team["id"].as_str().unwrap()),
        json!({"title": "Theirs"}),
    )
    .await;
    let body = json!({"title": "Sneaky", "parent_id": foreign["id"]});
    let (status, _) = call(&app, Method::POST, &eng, &w.member, Some(body)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // A parent in a private team is not named to those who cannot see that team.
    let secret = file(
        w.owner.clone(),
        format!("{}/teams/{}/issues", w.ws, w.secret),
        json!({"title": "Acquisition"}),
    )
    .await;
    let body = json!({"parent_id": secret["id"]});
    let (status, linked) = call(&app, Method::PATCH, &b_path, &w.owner, Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{linked}");
    assert_eq!(linked["parent"]["title"], "Acquisition");
    let (_, seen) = call(&app, Method::GET, &b_path, &w.member, None).await;
    assert!(seen["parent"].is_null(), "{seen}");

    // Deleting the parent leaves its parts as issues of their own.
    let (status, _) = call(&app, Method::DELETE, &p_path, &w.member, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, a) = call(&app, Method::GET, &a_path, &w.member, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(a["parent"].is_null());
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn teams_plan_issues_in_cycles(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let cycles = format!("{}/teams/{}/cycles", w.ws, w.eng);
    let today = chrono::Utc::now().date_naive();
    let day = |offset: i64| (today + chrono::Duration::days(offset)).to_string();

    // Team owners and admins plan; members read; cycles are numbered per team.
    let now = json!({"starts_on": day(-3), "ends_on": day(10), "name": " Launch "});
    let (status, _) = call(&app, Method::POST, &cycles, &w.member, Some(now.clone())).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, current) = call(&app, Method::POST, &cycles, &w.owner, Some(now)).await;
    assert_eq!(status, StatusCode::CREATED, "{current}");
    assert_eq!(
        (
            current["number"].as_i64(),
            current["name"].as_str(),
            current["status"].as_str()
        ),
        (Some(1), Some("Launch"), Some("active"))
    );
    let next = json!({"starts_on": day(11), "ends_on": day(24)});
    let (_, next) = call(&app, Method::POST, &cycles, &w.owner, Some(next)).await;
    assert_eq!(
        (next["number"].as_i64(), next["status"].as_str()),
        (Some(2), Some("upcoming"))
    );

    // Dates must make sense and not share a day with another cycle.
    for (body, expected) in [
        (
            json!({"starts_on": day(10), "ends_on": day(12)}),
            StatusCode::CONFLICT,
        ),
        (
            json!({"starts_on": day(40), "ends_on": day(39)}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"starts_on": day(40), "ends_on": day(200)}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        let (status, problem) = call(&app, Method::POST, &cycles, &w.owner, Some(body)).await;
        assert_eq!(status, expected, "{problem}");
    }
    let next_path = format!("{cycles}/{}", next["id"].as_str().unwrap());
    let clash = json!({"starts_on": day(5)});
    let (status, _) = call(&app, Method::PATCH, &next_path, &w.owner, Some(clash)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let rename = json!({"name": "Polish", "ends_on": day(20)});
    let (status, renamed) = call(&app, Method::PATCH, &next_path, &w.owner, Some(rename)).await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["name"], "Polish");

    // Issues are planned in a cycle of their own team and counted there.
    let issues = format!("{}/teams/{}/issues", w.ws, w.eng);
    let body = json!({"title": "Fix login", "cycle_id": current["id"]});
    let (status, issue) = call(&app, Method::POST, &issues, &w.member, Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{issue}");
    assert_eq!(
        (
            issue["cycle"]["number"].as_i64(),
            issue["cycle"]["name"].as_str()
        ),
        (Some(1), Some("Launch"))
    );
    let (_, other) = call(
        &app,
        Method::POST,
        &issues,
        &w.member,
        Some(json!({"title": "Write docs"})),
    )
    .await;
    assert!(other["cycle"].is_null());
    let secret_cycles = format!("{}/teams/{}/cycles", w.ws, w.secret);
    let far = json!({"starts_on": day(0), "ends_on": day(6)});
    let (_, foreign) = call(&app, Method::POST, &secret_cycles, &w.owner, Some(far)).await;
    let path = format!("/issues/{}", other["id"].as_str().unwrap());
    let body = json!({"cycle_id": foreign["id"]});
    let (status, problem) = call(&app, Method::PATCH, &path, &w.owner, Some(body)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    let body = json!({"cycle_id": current["id"]});
    let (_, other) = call(&app, Method::PATCH, &path, &w.member, Some(body)).await;
    assert_eq!(other["cycle"]["number"], 1);

    let (_, states) = call(
        &app,
        Method::GET,
        &format!("{}/teams/{}/states", w.ws, w.eng),
        &w.owner,
        None,
    )
    .await;
    let done = states
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "Done")
        .unwrap()["id"]
        .clone();
    call(
        &app,
        Method::PATCH,
        &path,
        &w.member,
        Some(json!({"state_id": done})),
    )
    .await;
    let (_, list) = call(&app, Method::GET, &cycles, &w.member, None).await;
    let seen: Vec<(i64, i64, i64)> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["number"].as_i64().unwrap(),
                c["issue_count"].as_i64().unwrap(),
                c["closed_count"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(seen, [(2, 0, 0), (1, 2, 1)], "latest first");
    let filter = format!(
        "{}/issues?cycle_id={}",
        w.ws,
        current["id"].as_str().unwrap()
    );
    let (_, planned) = call(&app, Method::GET, &filter, &w.member, None).await;
    assert_eq!(planned.as_array().unwrap().len(), 2);

    // Taking an issue out, and deleting a cycle, leave the issues in place.
    let (_, other) = call(
        &app,
        Method::PATCH,
        &path,
        &w.member,
        Some(json!({"cycle_id": null})),
    )
    .await;
    assert!(other["cycle"].is_null());
    let current_path = format!("{cycles}/{}", current["id"].as_str().unwrap());
    let (status, _) = call(&app, Method::DELETE, &current_path, &w.member, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&app, Method::DELETE, &current_path, &w.owner, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let first = format!("/issues/{}", issue["id"].as_str().unwrap());
    let (status, issue) = call(&app, Method::GET, &first, &w.member, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(issue["cycle"].is_null());

    // Guests outside the team do not see its cycles.
    let (status, _) = call(&app, Method::GET, &cycles, &w.guest, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn issues_carry_a_due_date_and_are_listed_by_person(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let issues = format!("{}/teams/{}/issues", w.ws, w.eng);
    let list = format!("{}/issues", w.ws);
    let (_, owner) = call(&app, Method::GET, "/auth/me", &w.owner, None).await;
    let owner_id = owner["id"].as_str().unwrap().to_owned();

    // Filed with a due date, by the owner, for the member.
    let body =
        json!({"title": "Ship billing", "due_date": "2026-11-02", "assignee_id": w.member_id});
    let (status, dated) = call(&app, Method::POST, &issues, &w.owner, Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{dated}");
    assert_eq!(dated["due_date"], "2026-11-02");
    let id = dated["id"].as_str().unwrap().to_owned();
    let (_, plain) = call(
        &app,
        Method::POST,
        &issues,
        &w.member,
        Some(json!({"title": "No date"})),
    )
    .await;
    assert!(plain["due_date"].is_null());
    let bad = json!({"title": "x", "due_date": "next week"});
    let (status, _) = call(&app, Method::POST, &issues, &w.owner, Some(bad)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Moved, left alone by an unrelated edit, then removed; the timeline says each change.
    let issue = format!("/issues/{id}");
    let moved = json!({"due_date": "2026-11-09"});
    let (status, after) = call(&app, Method::PATCH, &issue, &w.member, Some(moved)).await;
    assert_eq!(
        (status, after["due_date"].as_str()),
        (StatusCode::OK, Some("2026-11-09"))
    );
    let (_, after) = call(
        &app,
        Method::PATCH,
        &issue,
        &w.member,
        Some(json!({"priority": 2})),
    )
    .await;
    assert_eq!(
        after["due_date"], "2026-11-09",
        "an edit of something else keeps it"
    );
    let (_, after) = call(
        &app,
        Method::PATCH,
        &issue,
        &w.member,
        Some(json!({"due_date": null})),
    )
    .await;
    assert!(after["due_date"].is_null());
    let (_, events) = call(
        &app,
        Method::GET,
        &format!("{issue}/events"),
        &w.member,
        None,
    )
    .await;
    let due: Vec<(Option<&str>, Option<&str>)> = events
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "due")
        .map(|e| (e["from"].as_str(), e["to"].as_str()))
        .collect();
    assert_eq!(
        due,
        [
            (Some("2026-11-02"), Some("2026-11-09")),
            (Some("2026-11-09"), None)
        ]
    );

    // Listed by who it is assigned to and by who filed it.
    let titles = |list: &Value| -> Vec<String> {
        let mut titles: Vec<String> = list
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["title"].as_str().unwrap().to_owned())
            .collect();
        titles.sort();
        titles
    };
    let mine = format!("{list}?assignee_id={}", w.member_id);
    let (_, assigned) = call(&app, Method::GET, &mine, &w.member, None).await;
    assert_eq!(titles(&assigned), ["Ship billing"]);
    let filed = format!("{list}?creator_id={}", w.member_id);
    let (_, created) = call(&app, Method::GET, &filed, &w.member, None).await;
    assert_eq!(titles(&created), ["No date"]);
    let filed = format!("{list}?creator_id={owner_id}");
    let (_, created) = call(&app, Method::GET, &filed, &w.member, None).await;
    assert_eq!(titles(&created), ["Ship billing"]);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_assignee_is_reminded_once_when_an_issue_is_due(pool: PgPool) {
    let app = TestApp::new(pool.clone(), &[]).await;
    let w = world(&app).await;
    let issues = format!("{}/teams/{}/issues", w.ws, w.eng);
    let day = |offset: i64| {
        (chrono::Utc::now().date_naive() + chrono::Duration::days(offset)).to_string()
    };
    let file = |title: &'static str, due: String, assignee: Option<&str>| {
        let (app, issues, owner) = (&app, &issues, &w.owner);
        let body = json!({"title": title, "due_date": due, "assignee_id": assignee});
        async move {
            let (status, issue) = call(app, Method::POST, issues, owner, Some(body)).await;
            assert_eq!(status, StatusCode::CREATED, "{issue}");
            issue["id"].as_str().unwrap().to_owned()
        }
    };
    let late = file("Late", day(-1), Some(&w.member_id)).await;
    file("Not yet", day(3), Some(&w.member_id)).await;
    file("Nobody's", day(-1), None).await;
    let inbox = format!("{}/inbox", w.ws);
    let due = |list: &Value| -> Vec<String> {
        let list = list.as_array().unwrap();
        let due = list.iter().filter(|n| n["kind"] == "due");
        due.map(|n| n["issue"]["title"].as_str().unwrap().to_owned())
            .collect()
    };

    // Only the issue that is due and has someone to tell; and only once.
    assert_eq!(nexc::repo::issues::remind_due(&pool).await.unwrap(), 1);
    assert_eq!(nexc::repo::issues::remind_due(&pool).await.unwrap(), 0);
    let (_, list) = call(&app, Method::GET, &inbox, &w.member, None).await;
    assert_eq!(due(&list), ["Late"]);
    let reminder = list
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "due")
        .unwrap();
    assert!(reminder["actor"].is_null(), "nobody did it");

    // A moved date is reminded of when it arrives; a closed issue is not.
    let issue = format!("/issues/{late}");
    call(
        &app,
        Method::PATCH,
        &issue,
        &w.owner,
        Some(json!({"due_date": day(2)})),
    )
    .await;
    assert_eq!(nexc::repo::issues::remind_due(&pool).await.unwrap(), 0);
    call(
        &app,
        Method::PATCH,
        &issue,
        &w.owner,
        Some(json!({"due_date": day(0)})),
    )
    .await;
    assert_eq!(nexc::repo::issues::remind_due(&pool).await.unwrap(), 1);
    let (_, states) = call(
        &app,
        Method::GET,
        &format!("{}/teams/{}/states", w.ws, w.eng),
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
    call(
        &app,
        Method::PATCH,
        &issue,
        &w.owner,
        Some(json!({"state_id": done, "due_date": day(-3)})),
    )
    .await;
    assert_eq!(nexc::repo::issues::remind_due(&pool).await.unwrap(), 0);
}
