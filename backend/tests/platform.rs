//! The platform console: a separate API for whoever runs the installation.
//! Platform administrators see every workspace and account, act on them,
//! and never work inside a workspace; everyone else never reaches `/admin`.
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode};
use common::{PASSWORD, TestApp, refresh_cookie};
use nexc::domain::user::Role;
use serde_json::{Value, json};
use sqlx::PgPool;

const CSRF: (&str, &str) = ("x-requested-with", "nexc");

/// A registered user: `(access token, user id)`.
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

/// Signs in; returns the response (status, cookie and token).
async fn login(app: &TestApp, email: &str) -> common::Resp {
    let body = json!({"email": email, "password": PASSWORD});
    app.request(Method::POST, "/api/v1/auth/login", None, Some(body))
        .await
}

/// A platform administrator created the way the server's bootstrap and
/// command line create one: `(access token, user id)`.
async fn platform_admin(app: &TestApp, email: &str) -> (String, String) {
    let hash = nexc::security::password::hash_password(PASSWORD).unwrap();
    let created =
        nexc::http::handlers::auth::create_user(&app.state, email, "Root", Role::Admin, &hash)
            .await
            .unwrap();
    let session = login(app, email).await;
    assert_eq!(session.status, StatusCode::OK, "{}", session.body);
    (
        session.body["access_token"].as_str().unwrap().to_owned(),
        created.id.to_string(),
    )
}

async fn workspace_of(app: &TestApp, token: &str) -> (String, String) {
    let (_, list) = call(app, Method::GET, "/workspaces", token, None).await;
    (
        list[0]["id"].as_str().unwrap().to_owned(),
        list[0]["name"].as_str().unwrap().to_owned(),
    )
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_platform_and_the_workspaces_are_separate_apis(pool: PgPool) {
    let app = TestApp::new(pool.clone(), &[]).await;
    let (owner, _) = user(&app, "owner@example.com").await;
    let (root, root_id) = platform_admin(&app, "root@example.com").await;
    let (wid, _) = workspace_of(&app, &owner).await;

    // A platform administrator belongs to no workspace, and start-up seeding leaves it so.
    nexc::http::handlers::workspaces::ensure_personal(&app.state)
        .await
        .unwrap();
    let memberships: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workspace_members WHERE user_id = $1::uuid")
            .bind(&root_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(memberships, 0);

    // Both sides keep their session endpoint.
    let (status, me) = call(&app, Method::GET, "/auth/me", &root, None).await;
    assert_eq!(
        (status, me["role"].as_str()),
        (StatusCode::OK, Some("admin"))
    );

    // Nothing of a workspace answers a platform administrator, member of it or not.
    sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role)
         VALUES ($1::uuid, $2::uuid, 'owner')",
    )
    .bind(&wid)
    .bind(&root_id)
    .execute(&pool)
    .await
    .unwrap();
    // Every documented route but the session, the console and the ticket-authenticated streams.
    let spec = serde_json::to_value(nexc::http::openapi()).unwrap();
    let id = uuid::Uuid::now_v7().to_string();
    let mut refused = 0;
    for (path, methods) in spec["paths"].as_object().unwrap() {
        let open = [
            "/api/v1/auth/",
            "/api/v1/admin/",
            "/api/v1/healthz",
            "/api/v1/readyz",
        ];
        let stream =
            path.ends_with("/events") && path.contains("/graphs/") || path.ends_with("/ws");
        if open.iter().any(|p| path.starts_with(p)) || stream || path == "/metrics" {
            continue;
        }
        // Path parameters become an id; the member row above makes the real workspace reachable.
        let mut url = String::new();
        for part in path.split('/').skip(1) {
            url.push('/');
            url.push_str(match part {
                "{wid}" => &wid,
                p if p.starts_with('{') => &id,
                p => p,
            });
        }
        for method in methods.as_object().unwrap().keys() {
            let method = Method::from_bytes(method.to_uppercase().as_bytes()).unwrap();
            let r = app.request(method.clone(), &url, Some(&root), None).await;
            assert_eq!(
                r.status,
                StatusCode::FORBIDDEN,
                "{method} {path}: {}",
                r.body
            );
            assert!(
                r.body["detail"]
                    .as_str()
                    .unwrap()
                    .contains("platform console"),
                "{method} {path}: {}",
                r.body
            );
            refused += 1;
        }
    }
    assert!(
        refused > 100,
        "the whole workspace API was checked ({refused})"
    );
    let rename = json!({"name": "Taken over"});
    let (status, _) = call(
        &app,
        Method::PATCH,
        &format!("/workspaces/{wid}"),
        &root,
        Some(rename),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Nothing of the platform answers anyone else, not even a workspace's owner.
    let console = [
        "/admin/workspaces".to_owned(),
        format!("/admin/workspaces/{wid}"),
        "/admin/users".to_owned(),
        "/admin/events".to_owned(),
        "/admin/infrastructure".to_owned(),
    ];
    for path in &console {
        let (status, _) = call(&app, Method::GET, path, &owner, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
        let (status, _) = call(&app, Method::GET, path, &root, None).await;
        assert_eq!(status, StatusCode::OK, "{path}");
    }
    let delete = format!("/admin/workspaces/{wid}/delete");
    let confirm = json!({"confirm": "x"});
    let (status, _) = call(&app, Method::POST, &delete, &owner, Some(confirm)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn platform_administrators_see_every_workspace_and_account(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (owner, owner_id) = user(&app, "owner@example.com").await;
    let (_, member_id) = user(&app, "member@example.com").await;
    let (root, _) = platform_admin(&app, "root@example.com").await;
    let (wid, name) = workspace_of(&app, &owner).await;
    let invite = json!({"email": "member@example.com", "role": "member"});
    let members = format!("/workspaces/{wid}/members");
    let (status, _) = call(&app, Method::POST, &members, &owner, Some(invite)).await;
    assert_eq!(status, StatusCode::CREATED);
    let gid = app.graph(&owner, "Ship the console").await;
    assert!(!gid.is_empty());

    let (status, workspaces) = call(&app, Method::GET, "/admin/workspaces", &root, None).await;
    assert_eq!(status, StatusCode::OK, "{workspaces}");
    assert_eq!(
        workspaces.as_array().unwrap().len(),
        2,
        "one per registration, none for the administrator"
    );
    let (_, mine) = call(&app, Method::GET, "/admin/workspaces?q=owner@", &root, None).await;
    assert_eq!(mine.as_array().unwrap().len(), 1);
    assert_eq!(
        (
            mine[0]["owner_email"].as_str(),
            mine[0]["member_count"].as_i64(),
            mine[0]["graph_count"].as_i64(),
        ),
        (Some("owner@example.com"), Some(2), Some(1))
    );

    // One workspace: who is in it and how much it holds, not what it holds.
    let path = format!("/admin/workspaces/{wid}");
    let (status, detail) = call(&app, Method::GET, &path, &root, None).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["name"].as_str(), Some(name.as_str()));
    let listed: Vec<(&str, &str)> = detail["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| (m["user_id"].as_str().unwrap(), m["role"].as_str().unwrap()))
        .collect();
    assert_eq!(
        listed,
        [(owner_id.as_str(), "owner"), (member_id.as_str(), "member")]
    );
    assert_eq!(detail["footprint"]["run_count"].as_i64(), Some(0));
    assert_eq!(detail["footprint"]["document_bytes"].as_i64(), Some(0));
    assert!(detail["graphs"].is_null() && detail["issues"].is_null());
    let unknown = format!("/admin/workspaces/{}", uuid::Uuid::now_v7());
    let (status, _) = call(&app, Method::GET, &unknown, &root, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, users) = call(&app, Method::GET, "/admin/users", &root, None).await;
    assert_eq!(users.as_array().unwrap().len(), 3);
    assert!(
        users[0]["password_hash"].is_null(),
        "no credentials in the list"
    );
    let (_, found) = call(&app, Method::GET, "/admin/users?q=member@", &root, None).await;
    assert_eq!(
        (
            found[0]["workspace_count"].as_i64(),
            found[0]["owned_count"].as_i64(),
            found[0]["suspended"].as_bool(),
        ),
        (Some(2), Some(1), Some(false))
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn suspension_and_role_changes_end_sessions_at_once(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (root, root_id) = platform_admin(&app, "root@example.com").await;
    let (token, cookie) = app.register("owner@example.com").await;
    let (_, owner_id) = {
        let me = call(&app, Method::GET, "/auth/me", &token, None).await;
        (me.0, me.1["id"].as_str().unwrap().to_owned())
    };
    let account = format!("/admin/users/{owner_id}");
    let refresh = |cookie: String| {
        let app = &app;
        async move {
            app.request_with(
                Method::POST,
                "/api/v1/auth/refresh",
                None,
                None,
                &[("cookie", &format!("nexc_refresh={cookie}")), CSRF],
            )
            .await
        }
    };

    // What a change must say.
    for body in [json!({}), json!({"role": "user", "reason": "why"})] {
        let (status, _) = call(&app, Method::PATCH, &account, &root, Some(body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
    // Nobody changes their own account; nobody outside the platform changes any.
    let own = format!("/admin/users/{root_id}");
    let suspend = json!({"suspended": true, "reason": "Unpaid invoice"});
    let (status, _) = call(&app, Method::PATCH, &own, &root, Some(suspend.clone())).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = call(&app, Method::PATCH, &own, &token, Some(suspend.clone())).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Suspended: the token in hand stops working, and neither refresh nor sign-in gives another.
    let (status, _) = call(&app, Method::GET, "/workspaces", &token, None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, suspended) = call(&app, Method::PATCH, &account, &root, Some(suspend)).await;
    assert_eq!(status, StatusCode::OK, "{suspended}");
    assert_eq!(
        (
            suspended["suspended"].as_bool(),
            suspended["suspended_reason"].as_str()
        ),
        (Some(true), Some("Unpaid invoice"))
    );
    let (status, _) = call(&app, Method::GET, "/workspaces", &token, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "the old token is ended");
    assert_eq!(refresh(cookie).await.status, StatusCode::UNAUTHORIZED);
    let refused = login(&app, "owner@example.com").await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert!(
        refused.body["detail"]
            .as_str()
            .unwrap()
            .contains("suspended")
    );
    // A wrong password learns nothing about the suspension.
    let wrong = json!({"email": "owner@example.com", "password": "not the password"});
    let guess = app
        .request(Method::POST, "/api/v1/auth/login", None, Some(wrong))
        .await;
    assert_eq!(guess.status, StatusCode::UNAUTHORIZED);

    // Reactivated: signing in works again.
    let lift = json!({"suspended": false});
    let (status, lifted) = call(&app, Method::PATCH, &account, &root, Some(lift)).await;
    assert_eq!(
        (status, lifted["suspended"].as_bool()),
        (StatusCode::OK, Some(false))
    );
    let back = login(&app, "owner@example.com").await;
    assert_eq!(back.status, StatusCode::OK, "{}", back.body);
    let token = back.body["access_token"].as_str().unwrap().to_owned();
    let cookie = refresh_cookie(&back.headers).unwrap();
    let (status, _) = call(&app, Method::GET, "/workspaces", &token, None).await;
    assert_eq!(status, StatusCode::OK);

    // Promoted: the workspace token ends at once; the refreshed one is the console's.
    let promote = json!({"role": "admin"});
    let (status, promoted) = call(&app, Method::PATCH, &account, &root, Some(promote)).await;
    assert_eq!(status, StatusCode::OK, "{promoted}");
    assert_eq!(promoted["role"].as_str(), Some("admin"));
    let (status, _) = call(&app, Method::GET, "/workspaces", &token, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let renewed = refresh(cookie).await;
    assert_eq!(renewed.status, StatusCode::OK, "{}", renewed.body);
    let token = renewed.body["access_token"].as_str().unwrap().to_owned();
    let (status, _) = call(&app, Method::GET, "/workspaces", &token, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&app, Method::GET, "/admin/users", &token, None).await;
    assert_eq!(status, StatusCode::OK);

    // Every step is in the activity log, newest first, with who did it.
    let (status, events) = call(&app, Method::GET, "/admin/events", &root, None).await;
    assert_eq!(status, StatusCode::OK, "{events}");
    let actions: Vec<&str> = events
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert_eq!(
        actions,
        ["role_changed", "account_reactivated", "account_suspended"]
    );
    assert_eq!(events[0]["detail"].as_str(), Some("user → admin"));
    assert_eq!(events[2]["detail"].as_str(), Some("Unpaid invoice"));
    assert_eq!(
        events[2]["subject"].as_str(),
        Some("Test <owner@example.com>")
    );
    assert_eq!(events[2]["actor_id"].as_str(), Some(root_id.as_str()));
    let (_, found) = call(&app, Method::GET, "/admin/events?q=invoice", &root, None).await;
    assert_eq!(found.as_array().unwrap().len(), 1);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_platform_assigns_owners_and_deletes_workspaces(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (root, _) = platform_admin(&app, "root@example.com").await;
    let (owner, owner_id) = user(&app, "owner@example.com").await;
    let (member, member_id) = user(&app, "member@example.com").await;
    let (wid, name) = workspace_of(&app, &owner).await;
    let members = format!("/workspaces/{wid}/members");
    let invite = |email: &str| json!({"email": email, "role": "member"});
    for email in ["member@example.com", "newcomer@example.com"] {
        let (status, _) = call(&app, Method::POST, &members, &owner, Some(invite(email))).await;
        assert_eq!(status, StatusCode::CREATED);
    }
    // Invited before registering: this workspace is the only one the newcomer has.
    let (newcomer, _) = user(&app, "newcomer@example.com").await;
    let (_, theirs) = call(&app, Method::GET, "/workspaces", &newcomer, None).await;
    assert_eq!(theirs.as_array().unwrap().len(), 1);

    // The only owner of a workspace other people work in stays on the workspace side.
    let promote = json!({"role": "admin"});
    let account = format!("/admin/users/{owner_id}");
    let (status, refused) = call(&app, Method::PATCH, &account, &root, Some(promote)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert!(refused["detail"].as_str().unwrap().contains(&name));

    // Assigning an owner: a registered, active, workspace-side account.
    let owners = format!("/admin/workspaces/{wid}/owners");
    let assign = |email: &str| Some(json!({"email": email}));
    let (status, _) = call(
        &app,
        Method::POST,
        &owners,
        &owner,
        assign("member@example.com"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &app,
        Method::POST,
        &owners,
        &root,
        assign("nobody@example.com"),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = call(
        &app,
        Method::POST,
        &owners,
        &root,
        assign("root@example.com"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "a platform administrator");
    let (status, _) = call(
        &app,
        Method::POST,
        &owners,
        &root,
        assign("owner@example.com"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "already an owner");
    let (status, detail) = call(
        &app,
        Method::POST,
        &owners,
        &root,
        assign("Member@Example.com"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    let roles: Vec<&str> = detail["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles, ["owner", "owner", "member"]);
    // The new owner owns, and the workspace's own audit log says who made them one.
    let rename = json!({"name": "Renamed by its new owner"});
    let ws = format!("/workspaces/{wid}");
    let (status, _) = call(&app, Method::PATCH, &ws, &member, Some(rename)).await;
    assert_eq!(status, StatusCode::OK);
    let (_, audit) = call(&app, Method::GET, &format!("{ws}/audit"), &member, None).await;
    let entry = audit
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["action"] == "platform_owner_assigned")
        .expect("the assignment is in the workspace's audit log");
    assert_eq!(
        (entry["actor_name"].as_str(), entry["subject"].as_str()),
        (Some("Root"), Some("Test <member@example.com>"))
    );
    // With another owner in place the first one may move to the platform.
    let _ = member_id;
    let promote = json!({"role": "admin"});
    let (status, _) = call(&app, Method::PATCH, &account, &root, Some(promote)).await;
    assert_eq!(status, StatusCode::OK);

    // Deleting asks for the name, and leaves nobody without a workspace.
    let delete = format!("/admin/workspaces/{wid}/delete");
    let confirm = |name: &str| Some(json!({"confirm": name}));
    let (status, _) = call(&app, Method::POST, &delete, &root, confirm("wrong")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let name = "Renamed by its new owner";
    let (status, _) = call(&app, Method::POST, &delete, &member, confirm(name)).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "owners delete from their side"
    );
    let (status, body) = call(&app, Method::POST, &delete, &root, confirm(name)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, _) = call(&app, Method::GET, &ws, &member, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, left) = call(&app, Method::GET, "/workspaces", &newcomer, None).await;
    let left = left.as_array().unwrap();
    assert_eq!(
        left.len(),
        1,
        "a personal workspace replaces the deleted one"
    );
    assert_eq!(
        (left[0]["role"].as_str(), left[0]["member_count"].as_i64()),
        (Some("owner"), Some(1))
    );
    assert_ne!(left[0]["id"].as_str(), Some(wid.as_str()));

    let (_, events) = call(&app, Method::GET, "/admin/events", &root, None).await;
    let deleted = &events[0];
    assert_eq!(
        (deleted["action"].as_str(), deleted["subject"].as_str()),
        (Some("workspace_deleted"), Some("Renamed by its new owner"))
    );
    assert!(
        deleted["detail"]
            .as_str()
            .unwrap()
            .contains("owner@example.com")
    );
    assert!(events.as_array().unwrap().iter().any(|e| {
        e["action"] == "owner_assigned"
            && e["detail"]
                .as_str()
                .unwrap()
                .contains("is an owner now (was member)")
    }));
}
