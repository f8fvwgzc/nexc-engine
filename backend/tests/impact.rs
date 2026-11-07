//! Versions and change impact of issues and projects: every change is
//! assessed by the change-impact engine in the transaction that writes it.
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

/// A workspace with a team, its owner and a member who works on sub-issues.
struct World {
    ws: String,
    team: String,
    owner: String,
    member: String,
    member_id: String,
    outsider: String,
    in_progress: String,
}

async fn world(app: &TestApp) -> World {
    let (owner, _) = user(app, "owner@example.com").await;
    let (member, member_id) = user(app, "member@example.com").await;
    let (outsider, _) = user(app, "outsider@example.com").await;
    let (_, list) = call(app, Method::GET, "/workspaces", &owner, None).await;
    let ws = format!("/workspaces/{}", list[0]["id"].as_str().unwrap());
    let body = json!({"email": "member@example.com", "role": "member"});
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
