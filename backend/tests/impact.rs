//! Versions and change impact of issues and projects: every change is
//! assessed by the change-impact engine — the real one, served in-process over
//! gRPC (`common::assessment_engine`) — in the transaction that writes it, and
//! a change the engine says needs a re-plan has the planner propose one.
#![forbid(unsafe_code)]

mod common;

use std::time::Duration;

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

/// A workspace with a team, its owner and a member who works on sub-issues.
struct World {
    ws: String,
    team: String,
    owner: String,
    owner_id: String,
    member: String,
    member_id: String,
    outsider: String,
    in_progress: String,
}

async fn world(app: &TestApp) -> World {
    world_tagged(app, "").await
}

/// A [`world`] of its own, its accounts' addresses marked with `tag`: several
/// apps on one database.
async fn world_tagged(app: &TestApp, tag: &str) -> World {
    let (owner, owner_id) = user(app, &format!("owner{tag}@example.com")).await;
    let member_email = format!("member{tag}@example.com");
    let (member, member_id) = user(app, &member_email).await;
    let (outsider, _) = user(app, &format!("outsider{tag}@example.com")).await;
    let (_, list) = call(app, Method::GET, "/workspaces", &owner, None).await;
    let ws = format!("/workspaces/{}", list[0]["id"].as_str().unwrap());
    let body = json!({"email": member_email, "role": "member"});
    let (status, _) = call(
        app,
        Method::POST,
        &format!("{ws}/members"),
        &owner,
        Some(body),
    )
    .await;
    assert!(status.is_success(), "{status}");
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
    let team = team["id"].as_str().unwrap().to_owned();
    let (_, states) = call(
        app,
        Method::GET,
        &format!("{ws}/teams/{team}/states"),
        &owner,
        None,
    )
    .await;
    let in_progress = states
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "In Progress")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    World {
        ws,
        team,
        owner,
        owner_id,
        member,
        member_id,
        outsider,
        in_progress,
    }
}

async fn file(app: &TestApp, w: &World, token: &str, body: Value) -> Value {
    let path = format!("{}/teams/{}/issues", w.ws, w.team);
    let (status, issue) = call(app, Method::POST, &path, token, Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{issue}");
    issue
}

fn id(v: &Value) -> String {
    v["id"].as_str().unwrap().to_owned()
}

const PARENT_SCOPE: &str = "Customers sign in to the portal.\n\n\
    - [ ] Email and password sign-up form\n\
    - [ ] Email and password login\n\
    - [ ] Password reset email flow\n\
    - [ ] Session expiry and refresh";

/// The parent "Authentication" in progress, with the sub-issue "Password
/// reset email flow" in progress too, assigned to the member.
async fn parent_and_sub_issue(app: &TestApp, w: &World) -> (Value, Value) {
    let parent = file(
        app,
        w,
        &w.owner,
        json!({"title": "Authentication for the customer portal", "description": PARENT_SCOPE,
               "state_id": w.in_progress, "priority": 2}),
    )
    .await;
    let sub = file(
        app,
        w,
        &w.owner,
        json!({"title": "Password reset email flow",
               "description": "Send a reset link by email; the link expires after an hour.",
               "parent_id": id(&parent), "state_id": w.in_progress,
               "assignee_id": w.member_id}),
    )
    .await;
    (parent, sub)
}

fn impact_on<'a>(assessment: &'a Value, issue: &str) -> &'a Value {
    assessment["impacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["entity_id"] == issue)
        .unwrap_or_else(|| panic!("no impact on {issue}: {assessment:#}"))
}

/// Rewriting a parent's description while one of its sub-issues is in
/// progress is a breaking change: the parent moves to 2.0.0, the sub-issue,
/// planned against 1.0.0, is flagged incompatible with an action, its owner
/// hears of it, and acknowledging the re-plan pins it to 2.0.0.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_parent_rewrite_during_a_sub_issue_is_major_and_flags_it(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let (parent, sub) = parent_and_sub_issue(&app, &w).await;
    let (pid, sid) = (id(&parent), id(&sub));
    assert_eq!(parent["version"], "1.0.0");
    assert_eq!(sub["version"], "1.0.0");

    // Before the rewrite the sub-issue's plan is current.
    let (status, version) = call(
        &app,
        Method::GET,
        &format!("/issues/{sid}/version"),
        &w.member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{version}");
    assert_eq!(
        (
            &version["version"],
            &version["basis"]["entity_kind"],
            &version["basis"]["entity_id"],
            &version["basis"]["pinned_version"],
            &version["basis"]["status"],
        ),
        (
            &json!("1.0.0"),
            &json!("issue"),
            &json!(pid),
            &json!("1.0.0"),
            &json!("current")
        )
    );

    let rewrite = "Customers sign in to the portal with Google only.\n\n\
        - [ ] Google OAuth login only\n\
        - [ ] No email or password accounts";
    let (status, updated) = call(
        &app,
        Method::PATCH,
        &format!("/issues/{pid}"),
        &w.owner,
        Some(json!({"description": rewrite})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["version"], "2.0.0", "a rewrite is a MAJOR bump");

    let (status, list) = call(
        &app,
        Method::GET,
        &format!("/issues/{pid}/assessments"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let a = &list[0];
    assert_eq!(
        (
            &a["entity_kind"],
            &a["entity_id"],
            &a["cause"],
            &a["bump"],
            &a["previous_version"],
            &a["next_version"],
        ),
        (
            &json!("issue"),
            &json!(pid),
            &json!("updated"),
            &json!("major"),
            &json!("1.0.0"),
            &json!("2.0.0")
        ),
        "{a:#}"
    );
    let recommendation = a["recommendation"].as_str().unwrap();
    assert!(
        ["review", "partial_replan", "full_replan"].contains(&recommendation),
        "{a:#}"
    );
    assert!(
        ["medium", "high", "critical"].contains(&a["level"].as_str().unwrap()),
        "{a:#}"
    );
    if recommendation.ends_with("replan") {
        assert!(a["replan_root"].is_string(), "{a:#}");
    }
    assert_eq!(
        a["actor"]["name"].as_str().map(|n| !n.is_empty()),
        Some(true)
    );
    let flagged = impact_on(a, &sid);
    assert_eq!(flagged["basis_status"], "incompatible", "{flagged:#}");
    assert_eq!(flagged["changed"], false);
    assert_ne!(flagged["action"], "none", "{flagged:#}");
    assert!(
        flagged["rank"].as_u64().unwrap() >= 1 && flagged["score"].as_f64().unwrap() > 0.0,
        "{flagged:#}"
    );
    assert_eq!(impact_on(a, &pid)["changed"], true);

    // The sub-issue's own history shows the parent's change, and its plan is out of date.
    let (_, of_sub) = call(
        &app,
        Method::GET,
        &format!("/issues/{sid}/assessments"),
        &w.member,
        None,
    )
    .await;
    assert!(
        of_sub
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["id"] == a["id"]),
        "{of_sub:#}"
    );
    let (_, version) = call(
        &app,
        Method::GET,
        &format!("/issues/{sid}/version"),
        &w.member,
        None,
    )
    .await;
    assert_eq!(
        (
            &version["basis"]["pinned_version"],
            &version["basis"]["current_version"],
            &version["basis"]["status"],
        ),
        (&json!("1.0.0"), &json!("2.0.0"), &json!("incompatible"))
    );

    // Its owner hears of it in their inbox, about the sub-issue.
    let (_, inbox) = call(
        &app,
        Method::GET,
        &format!("{}/inbox", w.ws),
        &w.member,
        None,
    )
    .await;
    let entry = inbox
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "impact")
        .unwrap_or_else(|| panic!("no impact entry: {inbox:#}"));
    assert_eq!(entry["issue"]["id"], json!(sid));

    // Once re-planned, acknowledging pins the sub-issue to the parent's 2.0.0.
    let (status, version) = call(
        &app,
        Method::POST,
        &format!("/issues/{sid}/version/acknowledge"),
        &w.member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{version}");
    assert_eq!(
        (
            &version["basis"]["pinned_version"],
            &version["basis"]["status"]
        ),
        (&json!("2.0.0"), &json!("current"))
    );
}

/// A typo fix is a patch: nothing anyone planned moves, the sub-issue stays
/// compatible and nobody is told.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_cosmetic_edit_is_a_patch_that_disturbs_nobody(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let (parent, sub) = parent_and_sub_issue(&app, &w).await;
    let (pid, sid) = (id(&parent), id(&sub));
    let (status, updated) = call(
        &app,
        Method::PATCH,
        &format!("/issues/{pid}"),
        &w.owner,
        Some(json!({"title": "Authentication for the customer portal."})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["version"], "1.0.1");
    let (_, version) = call(
        &app,
        Method::GET,
        &format!("/issues/{sid}/version"),
        &w.member,
        None,
    )
    .await;
    assert_eq!(version["basis"]["status"], "compatible", "{version:#}");
    let (_, inbox) = call(
        &app,
        Method::GET,
        &format!("{}/inbox", w.ws),
        &w.member,
        None,
    )
    .await;
    assert!(
        inbox
            .as_array()
            .unwrap()
            .iter()
            .all(|n| n["kind"] != "impact"),
        "{inbox:#}"
    );

    // An edit that changes nothing the engine reads moves no version.
    let (_, same) = call(
        &app,
        Method::PATCH,
        &format!("/issues/{pid}"),
        &w.owner,
        Some(json!({"github_url": "https://github.com/acme/portal/issues/1"})),
    )
    .await;
    assert_eq!(same["version"], "1.0.1");
}

/// A comment moves no version; a sub-issue created now is planned against the
/// parent's current version; moving it to another parent pins it to that one.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn new_and_moved_sub_issues_pin_the_current_version(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let (parent, _) = parent_and_sub_issue(&app, &w).await;
    let pid = id(&parent);
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/issues/{pid}/comments"),
        &w.owner,
        Some(json!({"body": "Looks good so far."})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, after_comment) =
        call(&app, Method::GET, &format!("/issues/{pid}"), &w.owner, None).await;
    assert_eq!(after_comment["version"], "1.0.0");

    let (_, bumped) = call(
        &app,
        Method::PATCH,
        &format!("/issues/{pid}"),
        &w.owner,
        Some(json!({"description": "Google OAuth login only."})),
    )
    .await;
    let parent_version = bumped["version"].as_str().unwrap().to_owned();
    assert_ne!(parent_version, "1.0.0");

    let late = file(
        &app,
        &w,
        &w.owner,
        json!({"title": "Google button", "parent_id": pid}),
    )
    .await;
    let (_, version) = call(
        &app,
        Method::GET,
        &format!("/issues/{}/version", id(&late)),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(
        (
            &version["basis"]["pinned_version"],
            &version["basis"]["status"]
        ),
        (&json!(parent_version), &json!("current"))
    );

    let other = file(&app, &w, &w.owner, json!({"title": "Billing"})).await;
    let (status, moved) = call(
        &app,
        Method::PATCH,
        &format!("/issues/{}", id(&late)),
        &w.owner,
        Some(json!({"parent_id": id(&other)})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    let (_, version) = call(
        &app,
        Method::GET,
        &format!("/issues/{}/version", id(&late)),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(
        (
            &version["basis"]["entity_id"],
            &version["basis"]["pinned_version"],
            &version["basis"]["status"]
        ),
        (&json!(id(&other)), &json!("1.0.0"), &json!("current"))
    );
}

/// Deleting a parent is assessed before it goes: the owner of a sub-issue
/// still in progress hears of it, and the sub-issue's history keeps it.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn deleting_a_parent_tells_the_owners_of_its_sub_issues(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let (parent, sub) = parent_and_sub_issue(&app, &w).await;
    let (pid, sid) = (id(&parent), id(&sub));
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/issues/{pid}"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, list) = call(
        &app,
        Method::GET,
        &format!("/issues/{sid}/assessments"),
        &w.member,
        None,
    )
    .await;
    let deletion = list
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["entity_id"] == json!(pid))
        .unwrap_or_else(|| panic!("no assessment of the deletion: {list:#}"));
    assert_eq!(
        (&deletion["cause"], &deletion["next_version"]),
        (&json!("deleted"), &Value::Null)
    );
    assert_ne!(impact_on(deletion, &sid)["action"], "none");
    let (_, inbox) = call(
        &app,
        Method::GET,
        &format!("{}/inbox", w.ws),
        &w.member,
        None,
    )
    .await;
    assert!(
        inbox
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["kind"] == "impact" && n["issue"]["id"] == json!(sid)),
        "{inbox:#}"
    );
}

/// A project is versioned and assessed too; outsiders read nothing of it.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_project_change_is_versioned_and_assessed(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let (status, project) = call(
        &app,
        Method::POST,
        &format!("{}/projects", w.ws),
        &w.owner,
        Some(json!({"name": "Customer portal", "description": PARENT_SCOPE})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{project}");
    assert_eq!(project["version"], "1.0.0");
    let prid = id(&project);
    let issue = file(
        &app,
        &w,
        &w.owner,
        json!({"title": "Password reset email flow", "project_id": prid,
               "state_id": w.in_progress, "assignee_id": w.member_id}),
    )
    .await;
    let (status, updated) = call(
        &app,
        Method::PATCH,
        &format!("{}/projects/{prid}", w.ws),
        &w.owner,
        Some(json!({"description": "Google OAuth login only."})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["version"], "2.0.0");
    let (status, list) = call(
        &app,
        Method::GET,
        &format!("{}/projects/{prid}/assessments", w.ws),
        &w.member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(
        (&list[0]["entity_kind"], &list[0]["bump"]),
        (&json!("project"), &json!("major"))
    );
    assert_eq!(
        impact_on(&list[0], &id(&issue))["basis_status"],
        "incompatible"
    );

    for path in [
        format!("{}/projects/{prid}/assessments", w.ws),
        format!("/issues/{}/assessments", id(&issue)),
        format!("/issues/{}/version", id(&issue)),
    ] {
        let (status, _) = call(&app, Method::GET, &path, &w.outsider, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
}

/// The parent's description rewritten while a sub-issue is in progress.
const REWRITE: &str = "Customers sign in to the portal with Google only.\n\n\
    - [ ] Google OAuth login only\n\
    - [ ] No email or password accounts";

async fn rewrite(app: &TestApp, w: &World, issue: &str, description: &str) -> Value {
    let (status, updated) = call(
        app,
        Method::PATCH,
        &format!("/issues/{issue}"),
        &w.owner,
        Some(json!({"description": description})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    updated
}

/// A loopback address where nothing listens.
async fn closed_port() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    url
}

/// A loopback address that takes connections and never answers.
async fn silent_server() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket);
        }
    });
    url
}

/// An engine that is down or does not answer in time never fails an edit: it
/// saves, its version stays, and nothing is assessed or proposed.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_engine_that_is_down_or_silent_never_blocks_an_edit(pool: PgPool) {
    let cases = [
        ("closed", closed_port().await, "2000"),
        ("silent", silent_server().await, "300"),
    ];
    for (tag, engine, timeout) in cases {
        let app = TestApp::new(
            pool.clone(),
            &[
                ("NEXC_ASSESSMENT_ENABLED", "true"),
                ("NEXC_ASSESSMENT_URL", &engine),
                ("NEXC_ASSESSMENT_TIMEOUT_MS", timeout),
                ("NEXC_REPLAN_PROPOSALS", "true"),
            ],
        )
        .await;
        let w = world_tagged(&app, tag).await;
        let (parent, sub) = parent_and_sub_issue(&app, &w).await;
        let started = std::time::Instant::now();
        let updated = rewrite(&app, &w, &id(&parent), REWRITE).await;
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the deadline bounds the edit: {:?}",
            started.elapsed()
        );
        assert_eq!(updated["version"], "1.0.0", "{engine}");
        assert_eq!(updated["description"], REWRITE);
        let (_, list) = call(
            &app,
            Method::GET,
            &format!("/issues/{}/assessments", id(&parent)),
            &w.owner,
            None,
        )
        .await;
        assert_eq!(list, json!([]), "{engine}");
        let (_, version) = call(
            &app,
            Method::GET,
            &format!("/issues/{}/version", id(&sub)),
            &w.member,
            None,
        )
        .await;
        assert_eq!(version["basis"]["status"], "current", "{version:#}");
        let proposals: i64 = sqlx::query_scalar("SELECT count(*) FROM replan_proposals")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(proposals, 0);
    }
}

/// The backend sends the engine's bearer token: with it the change is
/// assessed; without it, or with another, the engine refuses and the edit
/// saves unassessed.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_engine_gets_the_bearer_token(pool: PgPool) {
    let engine = common::assessment_engine(Some("s3cret-engine-token")).await;
    for (tag, token, assessed) in [
        ("none", "", false),
        ("wrong", "not-the-token", false),
        ("right", "s3cret-engine-token", true),
    ] {
        let app = TestApp::new(
            pool.clone(),
            &[
                ("NEXC_ASSESSMENT_ENABLED", "true"),
                ("NEXC_ASSESSMENT_URL", &engine),
                ("NEXC_ASSESSMENT_TOKEN", token),
            ],
        )
        .await;
        let w = world_tagged(&app, tag).await;
        let (parent, _) = parent_and_sub_issue(&app, &w).await;
        let updated = rewrite(&app, &w, &id(&parent), REWRITE).await;
        let expected = if assessed { "2.0.0" } else { "1.0.0" };
        assert_eq!(updated["version"], expected, "token {token:?}");
    }
}

/// An epic whose scope is rewritten while its work is under way: the
/// engine's own scenario, which calls for a re-plan.
struct Epic {
    parent: String,
    subs: Vec<String>,
}

const EPIC_SCOPE: &str = "Customers sign in to the portal.\n\n\
    ## Acceptance criteria\n\
    - [ ] Sign up and log in with email and password\n\
    - [ ] Password reset by email link\n\
    - [ ] Sessions expire after 30 days\n\
    - [ ] Rate-limit failed logins";

const EPIC_SECOND_REWRITE: &str = "Customers sign in to the portal through Okta.\n\n\
    ## Acceptance criteria\n\
    - [ ] Single sign-on with Okta (SAML 2.0)\n\
    - [ ] Sessions expire after one day\n\
    - [ ] Audit every sign-in\n\n\
    Neither passwords nor Google or Microsoft accounts are supported.";

const EPIC_REWRITE: &str = "Customers sign in to the portal with their company identity.\n\n\
    ## Acceptance criteria\n\
    - [ ] Log in with Google Workspace (OAuth 2.0 / OIDC)\n\
    - [ ] Log in with Microsoft Entra ID (OIDC)\n\
    - [ ] Sessions expire after 7 days\n\
    - [ ] Rate-limit failed logins\n\n\
    Password login is no longer needed.";

async fn epic(app: &TestApp, w: &World) -> Epic {
    let (status, project) = call(
        app,
        Method::POST,
        &format!("{}/projects", w.ws),
        &w.owner,
        Some(json!({"name": "Customer portal v2"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{project}");
    let parent = file(
        app,
        w,
        &w.owner,
        json!({"title": "Authentication for the customer portal", "description": EPIC_SCOPE,
               "state_id": w.in_progress, "project_id": id(&project),
               "assignee_id": w.member_id}),
    )
    .await;
    let mut subs = Vec::new();
    for (title, description, started) in [
        (
            "Email and password sign-up form",
            "Form with email, password and password confirmation; hash passwords with argon2.",
            true,
        ),
        (
            "Password reset email flow",
            "Send a reset link by email; the token expires after one hour.",
            false,
        ),
        (
            "Session expiry and refresh",
            "Sessions expire after 30 days of inactivity; refresh tokens rotate.",
            false,
        ),
    ] {
        let mut body = json!({"title": title, "description": description,
                              "parent_id": id(&parent), "assignee_id": w.member_id});
        if started {
            body["state_id"] = json!(w.in_progress);
        }
        subs.push(id(&file(app, w, &w.owner, body).await));
    }
    Epic {
        parent: id(&parent),
        subs,
    }
}

fn asks_for_replan(a: &Value) -> bool {
    a["recommendation"]
        .as_str()
        .is_some_and(|r| r.ends_with("replan"))
}

/// The issue's newest assessment that asked for a re-plan, once the plan it
/// proposed is ready to review (30 s at most).
async fn ready_replan(app: &TestApp, token: &str, issue: &str) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let (_, list) = call(
            app,
            Method::GET,
            &format!("/issues/{issue}/assessments"),
            token,
            None,
        )
        .await;
        let asked = list
            .as_array()
            .unwrap()
            .iter()
            .find(|a| asks_for_replan(a))
            .cloned();
        if let Some(a) = asked
            && a["replan"]["plan_status"] == "ready"
        {
            return a;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no ready re-plan: {list:#}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn count(pool: &PgPool, sql: &'static str, id: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(uuid::Uuid::parse_str(id).unwrap())
        .fetch_one(pool)
        .await
        .unwrap()
}

/// A rewrite that calls for a re-plan has the planner draft exactly one, as
/// the person who made it, after a burst of edits settles: in the issue's
/// graph (made for it), seeded with the assessment, readable by the team only,
/// and never applied. The issue's owner finds it in their inbox and applies
/// it; a later change supersedes the open proposal with a new one.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_replan_is_proposed_once_and_waits_for_a_person(pool: PgPool) {
    let app = TestApp::new(
        pool.clone(),
        &[
            ("NEXC_REPLAN_PROPOSALS", "true"),
            ("NEXC_REPLAN_DEBOUNCE_SECS", "1"),
        ],
    )
    .await;
    let w = world(&app).await;
    // The member works on the team's issues, so they may open its graphs.
    let member_id = w.member_id.clone();
    let join = format!("{}/teams/{}/members/{member_id}", w.ws, w.team);
    let (status, _) = call(&app, Method::PUT, &join, &w.member, Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK);
    let e = epic(&app, &w).await;
    // A sub-issue in a private team the parent's team cannot see.
    let (status, secret) = call(
        &app,
        Method::POST,
        &format!("{}/teams", w.ws),
        &w.owner,
        Some(json!({"name": "Security", "key": "SEC", "private": true})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{secret}");
    let (status, hidden) = call(
        &app,
        Method::POST,
        &format!("{}/teams/{}/issues", w.ws, id(&secret)),
        &w.owner,
        Some(json!({"title": "Classified credential vault migration", "parent_id": e.parent})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{hidden}");

    // A burst: the rewrite, then a second one a moment later, both breaking.
    rewrite(&app, &w, &e.parent, EPIC_REWRITE).await;
    let updated = rewrite(&app, &w, &e.parent, EPIC_SECOND_REWRITE).await;
    assert_eq!(updated["version"], "3.0.0", "{updated:#}");

    let a = ready_replan(&app, &w.owner, &e.parent).await;
    let recommendation = a["recommendation"].as_str().unwrap();
    assert!(
        ["partial_replan", "full_replan"].contains(&recommendation),
        "{a:#}"
    );
    let replan = &a["replan"];
    let (graph, plan) = (
        replan["graph_id"].as_str().unwrap().to_owned(),
        replan["plan_id"].as_str().unwrap().to_owned(),
    );
    assert_eq!(
        (
            &replan["status"],
            &replan["root_kind"],
            &replan["root_id"],
            &replan["error"]
        ),
        (
            &json!("started"),
            &json!("issue"),
            &a["replan_root"],
            &Value::Null
        ),
        "{a:#}"
    );
    // The burst shares one proposal and started one LLM run.
    let (_, list) = call(
        &app,
        Method::GET,
        &format!("/issues/{}/assessments", e.parent),
        &w.owner,
        None,
    )
    .await;
    let asked: Vec<&Value> = list
        .as_array()
        .unwrap()
        .iter()
        .filter(|x| asks_for_replan(x))
        .collect();
    assert_eq!(asked.len(), 2, "both rewrites ask for a re-plan: {list:#}");
    assert!(
        asked.iter().all(|x| x["replan"]["id"] == replan["id"]),
        "{list:#}"
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM replan_proposals WHERE root_id = $1",
            replan["root_id"].as_str().unwrap()
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM plans WHERE graph_id = $1",
            &graph
        )
        .await,
        1
    );
    assert_eq!(app.fake.calls.load(std::sync::atomic::Ordering::SeqCst), 1);

    // In the issue's graph, which was made for it; seeded with the assessment, as the team reads it.
    let root = replan["root_id"].as_str().unwrap();
    let (_, root_issue) = call(
        &app,
        Method::GET,
        &format!("/issues/{root}"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(root_issue["graph_id"], json!(graph));
    let instructions: String = sqlx::query_scalar("SELECT instructions FROM plans WHERE id = $1")
        .bind(uuid::Uuid::parse_str(&plan).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(instructions.starts_with("Re-plan ENG-"), "{instructions}");
    assert!(
        instructions.contains("Password reset email flow"),
        "{instructions}"
    );
    assert!(
        instructions.contains("Single sign-on with Okta"),
        "{instructions}"
    );
    assert!(
        !instructions.contains("Classified credential vault"),
        "another team's item stays out: {instructions}"
    );
    assert!(instructions.contains("left out"), "{instructions}");
    let owner_id: String = sqlx::query_scalar("SELECT owner_id::text FROM plans WHERE id = $1")
        .bind(uuid::Uuid::parse_str(&plan).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(owner_id, w.owner_id, "drafted as who made the change");

    // Nothing is applied: the plan waits, the graph is empty.
    let (status, ready) = call(
        &app,
        Method::GET,
        &format!("/graphs/{graph}/plans/{plan}"),
        &w.member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{ready}");
    assert_eq!(ready["status"], "ready");
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM nodes WHERE graph_id = $1",
            &graph
        )
        .await,
        0
    );

    // The parent's owner finds it in the inbox, linked to the assessment.
    let (_, inbox) = call(
        &app,
        Method::GET,
        &format!("{}/inbox", w.ws),
        &w.member,
        None,
    )
    .await;
    let entry = inbox
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "impact" && n["issue"]["id"] == json!(e.parent))
        .unwrap_or_else(|| panic!("no impact entry about the parent: {inbox:#}"));
    assert_eq!(entry["replan"]["id"], replan["id"], "{entry:#}");
    assert!(entry["assessment_id"].is_string(), "{entry:#}");
    // The sub-issues' owner (the member too) hears about theirs.
    assert!(
        inbox
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["kind"] == "impact"
                && e.subs
                    .contains(&n["issue"]["id"].as_str().unwrap_or_default().to_owned())),
        "{inbox:#}"
    );
    // Outsiders read nothing of it.
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/issues/{}/assessments", e.parent),
        &w.outsider,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A later change supersedes the open proposal: one open per root, a new plan drafted.
    rewrite(&app, &w, &e.parent, EPIC_REWRITE).await;
    let (_, list) = call(
        &app,
        Method::GET,
        &format!("/issues/{}/assessments", e.parent),
        &w.owner,
        None,
    )
    .await;
    assert!(asks_for_replan(&list[0]), "{:#}", list[0]);
    let newer = ready_replan(&app, &w.owner, &e.parent).await;
    {
        assert_ne!(newer["replan"]["id"], replan["id"], "{newer:#}");
        let (_, list) = call(
            &app,
            Method::GET,
            &format!("/issues/{}/assessments", e.parent),
            &w.owner,
            None,
        )
        .await;
        let old = list
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == a["id"])
            .unwrap();
        assert_eq!(old["replan"]["status"], "superseded", "{old:#}");
        assert_eq!(
            count(&pool, "SELECT count(*) FROM replan_proposals WHERE root_id = $1 AND status IN ('pending', 'starting', 'started')", root).await,
            1
        );
    }
    let newest = &newer["replan"];
    let (graph, plan) = (
        newest["graph_id"].as_str().unwrap(),
        newest["plan_id"].as_str().unwrap(),
    );

    // The owner applies it, on the graph, like any plan.
    let (status, applied) = call(
        &app,
        Method::POST,
        &format!("/graphs/{graph}/plans/{plan}/apply"),
        &w.member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert!(
        !applied["nodes"].as_array().unwrap().is_empty(),
        "{applied:#}"
    );
    let (_, list) = call(
        &app,
        Method::GET,
        &format!("/issues/{}/assessments", e.parent),
        &w.member,
        None,
    )
    .await;
    assert_eq!(list[0]["replan"]["plan_status"], "applied", "{:#}", list[0]);
}

/// A project has no graph: a re-plan of one is recorded, with the reason, and
/// starts no LLM run.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_project_replan_is_skipped_with_the_reason(pool: PgPool) {
    let app = TestApp::new(pool.clone(), &[("NEXC_REPLAN_PROPOSALS", "true")]).await;
    let w = world(&app).await;
    let (status, project) = call(
        &app,
        Method::POST,
        &format!("{}/projects", w.ws),
        &w.owner,
        Some(json!({"name": "Customer portal", "description": EPIC_SCOPE})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{project}");
    let prid = id(&project);
    for title in [
        "Email and password sign-up form",
        "Password reset email flow",
        "Session expiry and refresh",
    ] {
        file(
            &app,
            &w,
            &w.owner,
            json!({"title": title, "project_id": prid, "state_id": w.in_progress,
                   "assignee_id": w.member_id}),
        )
        .await;
    }
    let (status, _) = call(
        &app,
        Method::PATCH,
        &format!("{}/projects/{prid}", w.ws),
        &w.owner,
        Some(json!({"description": EPIC_REWRITE})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let path = format!("{}/projects/{prid}/assessments", w.ws);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let (_, list) = call(&app, Method::GET, &path, &w.owner, None).await;
        let a = &list[0];
        assert_eq!(
            (
                &a["entity_kind"],
                &a["recommendation"],
                &a["replan"]["root_kind"]
            ),
            (&json!("project"), &json!("full_replan"), &json!("project")),
            "{a:#}"
        );
        if a["replan"]["status"] == "skipped" {
            assert!(
                a["replan"]["error"]
                    .as_str()
                    .unwrap()
                    .contains("project has no graph"),
                "{a:#}"
            );
            assert_eq!(a["replan"]["plan_id"], Value::Null);
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "{a:#}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(app.fake.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}
