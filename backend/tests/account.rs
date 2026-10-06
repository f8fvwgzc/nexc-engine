//! A person's own account: name, password, sessions, a copy of their data,
//! and deleting it without taking a team's shared work along.
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode};
use common::{PASSWORD, TestApp, refresh_cookie};
use nexc::domain::user::Role;
use serde_json::{Value, json};
use sqlx::PgPool;

const CSRF: (&str, &str) = ("x-requested-with", "nexc");
const NEW_PASSWORD: &str = "a much better passphrase";

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

async fn login(app: &TestApp, email: &str, password: &str) -> common::Resp {
    let body = json!({"email": email, "password": password});
    app.request(Method::POST, "/api/v1/auth/login", None, Some(body))
        .await
}

async fn refresh(app: &TestApp, cookie: &str) -> StatusCode {
    app.request_with(
        Method::POST,
        "/api/v1/auth/refresh",
        None,
        None,
        &[("cookie", &format!("nexc_refresh={cookie}")), CSRF],
    )
    .await
    .status
}

async fn id_of(app: &TestApp, token: &str) -> String {
    let (_, me) = call(app, Method::GET, "/auth/me", token, None).await;
    me["id"].as_str().unwrap().to_owned()
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_person_changes_their_name_password_and_sessions(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (laptop, laptop_cookie) = app.register("ada@example.com").await;
    let phone = login(&app, "ada@example.com", PASSWORD).await;
    let phone_token = phone.body["access_token"].as_str().unwrap().to_owned();
    let phone_cookie = refresh_cookie(&phone.headers).unwrap();

    // The name: trimmed, validated, and shown from then on.
    let (status, me) = call(
        &app,
        Method::PATCH,
        "/auth/me",
        &laptop,
        Some(json!({"name": "  Ada Lovelace "})),
    )
    .await;
    assert_eq!(
        (status, me["name"].as_str()),
        (StatusCode::OK, Some("Ada Lovelace"))
    );
    let (status, _) = call(
        &app,
        Method::PATCH,
        "/auth/me",
        &laptop,
        Some(json!({"name": " "})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (_, me) = call(&app, Method::GET, "/auth/me", &phone_token, None).await;
    assert_eq!(me["name"], "Ada Lovelace");

    // Two sign-ins are alive.
    let (status, sessions) = call(&app, Method::GET, "/auth/sessions", &laptop, None).await;
    assert_eq!(
        (status, sessions["active"].as_i64()),
        (StatusCode::OK, Some(2))
    );

    // The password: the current one is asked for, and the new one must meet the rules.
    let change =
        |current: &str, new: &str| json!({"current_password": current, "new_password": new});
    for (body, field) in [
        (change("not my password", NEW_PASSWORD), "current_password"),
        (change(PASSWORD, "short"), "new_password"),
        (change(PASSWORD, PASSWORD), "new_password"),
    ] {
        let (status, problem) =
            call(&app, Method::POST, "/auth/password", &laptop, Some(body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
        assert!(problem["errors"][field].is_array(), "{field}: {problem}");
    }
    let changed = app
        .request(
            Method::POST,
            "/api/v1/auth/password",
            Some(&laptop),
            Some(change(PASSWORD, NEW_PASSWORD)),
        )
        .await;
    assert_eq!(changed.status, StatusCode::OK, "{}", changed.body);
    let fresh = changed.body["access_token"].as_str().unwrap().to_owned();
    let fresh_cookie = refresh_cookie(&changed.headers).unwrap();

    // Every session ended, on every device; this one started anew.
    for old in [&laptop, &phone_token] {
        let (status, _) = call(&app, Method::GET, "/auth/me", old, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    for old in [&laptop_cookie, &phone_cookie] {
        assert_eq!(refresh(&app, old).await, StatusCode::UNAUTHORIZED);
    }
    let (status, sessions) = call(&app, Method::GET, "/auth/sessions", &fresh, None).await;
    assert_eq!(
        (status, sessions["active"].as_i64()),
        (StatusCode::OK, Some(1))
    );
    assert_eq!(
        login(&app, "ada@example.com", PASSWORD).await.status,
        StatusCode::UNAUTHORIZED
    );
    let again = login(&app, "ada@example.com", NEW_PASSWORD).await;
    assert_eq!(again.status, StatusCode::OK);
    let other = again.body["access_token"].as_str().unwrap().to_owned();

    // Signing out everywhere ends this device too.
    let (status, _) = call(&app, Method::POST, "/auth/sessions/end", &fresh, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    for token in [&fresh, &other] {
        let (status, _) = call(&app, Method::GET, "/auth/me", token, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(refresh(&app, &fresh_cookie).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        login(&app, "ada@example.com", NEW_PASSWORD).await.status,
        StatusCode::OK
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_person_gets_a_copy_of_their_data_and_deletes_their_account(pool: PgPool) {
    let app = TestApp::new(pool.clone(), &[]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let (leaver, leaver_cookie) = app.register("leaver@example.com").await;
    let leaver_id = id_of(&app, &leaver).await;
    let (_, list) = call(&app, Method::GET, "/workspaces", &owner, None).await;
    let wid = list[0]["id"].as_str().unwrap().to_owned();
    let ws = format!("/workspaces/{wid}");
    let members = format!("{ws}/members");
    let invite = json!({"email": "leaver@example.com", "role": "owner"});
    call(&app, Method::POST, &members, &owner, Some(invite)).await;
    let team = json!({"name": "Engineering", "key": "ENG"});
    let (_, team) = call(
        &app,
        Method::POST,
        &format!("{ws}/teams"),
        &owner,
        Some(team),
    )
    .await;
    let team_id = team["id"].as_str().unwrap();
    let seat = format!("{ws}/teams/{team_id}/members/{leaver_id}");
    call(
        &app,
        Method::PUT,
        &seat,
        &owner,
        Some(json!({"role": "member"})),
    )
    .await;
    let issues = format!("{ws}/teams/{team_id}/issues");
    let (_, written) = call(
        &app,
        Method::POST,
        &issues,
        &leaver,
        Some(json!({"title": "Written by the leaver"})),
    )
    .await;
    let written_id = written["id"].as_str().unwrap().to_owned();
    let assigned = json!({"title": "Assigned to the leaver", "assignee_id": leaver_id});
    let (_, assigned) = call(&app, Method::POST, &issues, &owner, Some(assigned)).await;
    let assigned_id = assigned["id"].as_str().unwrap().to_owned();
    let comment = json!({"body": "A thought of mine"});
    call(
        &app,
        Method::POST,
        &format!("/issues/{written_id}/comments"),
        &leaver,
        Some(comment),
    )
    .await;
    let graph = json!({"name": "Shared plan", "goal": "g", "workspace_id": wid});
    let (status, graph) = call(&app, Method::POST, "/graphs", &leaver, Some(graph)).await;
    assert_eq!(status, StatusCode::CREATED);
    let graph_id = graph["id"].as_str().unwrap().to_owned();

    // The copy: the account, where it belongs and what it wrote; no credential.
    let export = app
        .request(Method::GET, "/api/v1/auth/me/export", Some(&leaver), None)
        .await;
    assert_eq!(export.status, StatusCode::OK, "{}", export.body);
    assert!(
        export.headers["content-disposition"]
            .to_str()
            .unwrap()
            .contains("nexc-account.json")
    );
    let copy = &export.body;
    assert_eq!(copy["account"]["email"], "leaver@example.com");
    let sections = &copy["sections"];
    assert_eq!(sections["workspaces"].as_array().unwrap().len(), 2);
    assert_eq!(sections["teams"][0]["key"], "ENG");
    assert_eq!(
        sections["issues_created"][0]["title"],
        "Written by the leaver"
    );
    assert_eq!(sections["issues_assigned"][0]["identifier"], "ENG-2");
    assert_eq!(sections["comments"][0]["body"], "A thought of mine");
    assert_eq!(sections["graphs"][0]["name"], "Shared plan");
    assert_eq!(copy["truncated"], json!([]));
    let text = copy.to_string();
    assert!(
        !text.contains("password") && !text.contains("argon2"),
        "no credential in the copy"
    );

    // Deleting asks for the password, and is refused while a shared workspace would lose its
    // only owner.
    let delete = |password: &str| Some(json!({"password": password}));
    let (status, _) = call(
        &app,
        Method::POST,
        "/auth/me/delete",
        &leaver,
        delete("not my password"),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, refused) = call(&app, Method::POST, "/auth/me/delete", &owner, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    let own = id_of(&app, &owner).await;
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("{members}/{own}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "the first owner leaves the workspace to the leaver"
    );
    let stays = json!({"email": "owner@example.com", "role": "member"});
    let (status, _) = call(&app, Method::POST, &members, &leaver, Some(stays)).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, held) = call(
        &app,
        Method::POST,
        "/auth/me/delete",
        &leaver,
        delete(PASSWORD),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{held}");
    assert!(held["detail"].as_str().unwrap().contains("only owner"));
    let hand_over = json!({"role": "owner"});
    let (status, _) = call(
        &app,
        Method::PATCH,
        &format!("{members}/{own}"),
        &leaver,
        Some(hand_over),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Deleted: no way in, and the address is free again.
    let (status, body) = call(
        &app,
        Method::POST,
        "/auth/me/delete",
        &leaver,
        delete(PASSWORD),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, _) = call(&app, Method::GET, "/auth/me", &leaver, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        refresh(&app, &leaver_cookie).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&app, "leaver@example.com", PASSWORD).await.status,
        StatusCode::UNAUTHORIZED
    );
    let (name, email, hash): (String, String, String) =
        sqlx::query_as("SELECT name, email, password_hash FROM users WHERE id = $1::uuid")
            .bind(&leaver_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((name.as_str(), hash.as_str()), ("Deleted account", "!"));
    assert!(email.ends_with("@deleted.invalid"));
    let solo: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workspaces w WHERE NOT EXISTS
             (SELECT 1 FROM workspace_members m WHERE m.workspace_id = w.id)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(solo, 0, "the workspace the account was alone in is gone");

    // The shared workspace keeps its work: the issue and the graph are there, the comment
    // reads as written by a deleted account, and the assignment is cleared.
    let (_, roster) = call(&app, Method::GET, &members, &owner, None).await;
    assert_eq!(roster.as_array().unwrap().len(), 1);
    let (status, kept) = call(
        &app,
        Method::GET,
        &format!("/issues/{written_id}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(
        (status, kept["title"].as_str()),
        (StatusCode::OK, Some("Written by the leaver"))
    );
    let (_, freed) = call(
        &app,
        Method::GET,
        &format!("/issues/{assigned_id}"),
        &owner,
        None,
    )
    .await;
    assert!(
        freed["assignee_id"].is_null() && freed["assignee"].is_null(),
        "{freed}"
    );
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/graphs/{graph_id}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the graph the account created stays with the workspace"
    );
    let (_, timeline) = call(&app, Method::GET, &format!("{ws}/timeline"), &owner, None).await;
    let said = timeline
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "issue_comment")
        .unwrap();
    assert_eq!(said["actor"], "Deleted account");
    let (_, audit) = call(&app, Method::GET, &format!("{ws}/audit"), &owner, None).await;
    let gone = &audit[0];
    assert_eq!(
        (
            gone["action"].as_str(),
            gone["subject"].as_str(),
            gone["detail"].as_str()
        ),
        (
            Some("member_removed"),
            Some("Deleted account"),
            Some("account deleted")
        )
    );

    // The same address registers a new account, which knows nothing of the old one.
    let (anew, _) = app.register("leaver@example.com").await;
    assert_ne!(id_of(&app, &anew).await, leaver_id);
    let (_, theirs) = call(&app, Method::GET, "/workspaces", &anew, None).await;
    assert_eq!(theirs.as_array().unwrap().len(), 1);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_platform_erases_an_account_on_request(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let hash = nexc::security::password::hash_password(PASSWORD).unwrap();
    nexc::http::handlers::auth::create_user(
        &app.state,
        "root@example.com",
        "Root",
        Role::Admin,
        &hash,
    )
    .await
    .unwrap();
    let root = login(&app, "root@example.com", PASSWORD).await.body["access_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let (person, _) = app.register("person@example.com").await;
    let person_id = id_of(&app, &person).await;
    let root_id = id_of(&app, &root).await;
    let erase = format!("/admin/users/{person_id}/erase");
    let confirm = |email: &str| Some(json!({"confirm": email}));

    // Only the platform, only with the address typed out, and never an administrator.
    let (status, _) = call(
        &app,
        Method::POST,
        &erase,
        &person,
        confirm("person@example.com"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &app,
        Method::POST,
        &erase,
        &root,
        confirm("someone@example.com"),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let own = format!("/admin/users/{root_id}/erase");
    let (status, _) = call(&app, Method::POST, &own, &root, confirm("root@example.com")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    // A platform administrator does not delete their own account while they are one.
    let (status, _) = call(
        &app,
        Method::POST,
        "/auth/me/delete",
        &root,
        Some(json!({"password": PASSWORD})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, body) = call(
        &app,
        Method::POST,
        &erase,
        &root,
        confirm("Person@Example.com"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, _) = call(&app, Method::GET, "/workspaces", &person, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        login(&app, "person@example.com", PASSWORD).await.status,
        StatusCode::UNAUTHORIZED
    );
    // Gone from the console's lists; the log keeps the id, not the name or the address.
    let (_, users) = call(&app, Method::GET, "/admin/users", &root, None).await;
    assert_eq!(users.as_array().unwrap().len(), 1);
    let (status, _) = call(
        &app,
        Method::POST,
        &erase,
        &root,
        confirm("person@example.com"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "there is nothing left to erase"
    );
    let (_, events) = call(&app, Method::GET, "/admin/events", &root, None).await;
    let entry = &events[0];
    assert_eq!(entry["action"], "account_erased");
    assert_eq!(
        entry["subject"].as_str(),
        Some(format!("account {person_id}").as_str())
    );
    assert!(!entry.to_string().contains("person@example.com"));
    let (_, workspaces) = call(&app, Method::GET, "/admin/workspaces", &root, None).await;
    assert_eq!(
        workspaces.as_array().unwrap().len(),
        0,
        "their solo workspace went with them"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_forgotten_password_is_recovered_with_a_one_time_link(pool: PgPool) {
    let app = TestApp::new(pool.clone(), &[]).await;
    let hash = nexc::security::password::hash_password(PASSWORD).unwrap();
    nexc::http::handlers::auth::create_user(
        &app.state,
        "root@example.com",
        "Root",
        Role::Admin,
        &hash,
    )
    .await
    .unwrap();
    let root = login(&app, "root@example.com", PASSWORD).await.body["access_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let (signed_in, cookie) = app.register("forgetful@example.com").await;
    let account = id_of(&app, &signed_in).await;
    let issue = format!("/admin/users/{account}/password-reset");
    let reset = |token: &str, password: &str| {
        let app = &app;
        let body = json!({"token": token, "new_password": password});
        async move {
            app.request(
                Method::POST,
                "/api/v1/auth/password/reset",
                None,
                Some(body),
            )
            .await
            .status
        }
    };

    // Only the platform issues a link; it is shown once and logged without its token.
    let (status, _) = call(&app, Method::POST, &issue, &signed_in, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, issued) = call(&app, Method::POST, &issue, &root, None).await;
    assert_eq!(status, StatusCode::OK, "{issued}");
    assert_eq!(issued["expires_in"].as_i64(), Some(3600));
    let first = issued["token"].as_str().unwrap().to_owned();
    let stored: Vec<String> = sqlx::query_scalar("SELECT token_hash FROM password_resets")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(stored.len(), 1);
    assert_ne!(stored[0], first, "only a digest is kept");
    let (_, events) = call(&app, Method::GET, "/admin/events", &root, None).await;
    assert_eq!(events[0]["action"], "password_reset_issued");
    assert_eq!(events[0]["subject"], "Test <forgetful@example.com>");
    assert!(!events.to_string().contains(&first));

    // A newer link cancels the older one; a weak password does not use the link up.
    let (_, issued) = call(&app, Method::POST, &issue, &root, None).await;
    let second = issued["token"].as_str().unwrap().to_owned();
    assert_eq!(reset(&first, NEW_PASSWORD).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        reset(&second, "short").await,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    // Used: the new password is the way in, every earlier session is over, and the link is spent.
    assert_eq!(reset(&second, NEW_PASSWORD).await, StatusCode::NO_CONTENT);
    let (status, _) = call(&app, Method::GET, "/auth/me", &signed_in, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(refresh(&app, &cookie).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        login(&app, "forgetful@example.com", NEW_PASSWORD)
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        reset(&second, "another good passphrase").await,
        StatusCode::UNAUTHORIZED
    );

    // An expired link does nothing, and neither does one of a deleted account.
    let (_, issued) = call(&app, Method::POST, &issue, &root, None).await;
    let late = issued["token"].as_str().unwrap().to_owned();
    sqlx::query(
        "UPDATE password_resets SET expires_at = now() - interval '1 minute' WHERE used_at IS NULL",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        reset(&late, "another good passphrase").await,
        StatusCode::UNAUTHORIZED
    );
    let (_, issued) = call(&app, Method::POST, &issue, &root, None).await;
    let orphan = issued["token"].as_str().unwrap().to_owned();
    let erase = format!("/admin/users/{account}/erase");
    let confirm = json!({"confirm": "forgetful@example.com"});
    let (status, _) = call(&app, Method::POST, &erase, &root, Some(confirm)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        reset(&orphan, "another good passphrase").await,
        StatusCode::UNAUTHORIZED
    );
    let (status, _) = call(&app, Method::POST, &issue, &root, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "nothing to issue a link for");

    // Guessing tokens is limited like guessing passwords: the sixth miss in a minute waits.
    let guess = reset("not-a-token", NEW_PASSWORD).await;
    assert_eq!(guess, StatusCode::UNAUTHORIZED);
    let guess = reset("not-a-token", NEW_PASSWORD).await;
    assert_eq!(guess, StatusCode::TOO_MANY_REQUESTS);
}
