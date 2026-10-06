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
