//! Workspaces, roles, invitations and teams: who can see and do what.
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode};
use common::TestApp;
use serde_json::{Value, json};
use sqlx::PgPool;

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

/// The id of the personal workspace every account starts with.
async fn personal_workspace(app: &TestApp, token: &str) -> String {
    let (status, list) = call(app, Method::GET, "/workspaces", token, None).await;
    assert_eq!(status, StatusCode::OK);
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 1, "a new account has exactly one workspace");
    assert_eq!(list[0]["role"], "owner");
    list[0]["id"].as_str().unwrap().to_owned()
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn roles_gate_workspace_management(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (owner, owner_id) = user(&app, "owner@example.com").await;
    let (admin, admin_id) = user(&app, "admin@example.com").await;
    let (member, member_id) = user(&app, "member@example.com").await;
    let (outsider, _) = user(&app, "outsider@example.com").await;
    let wid = personal_workspace(&app, &owner).await;
    let ws = format!("/workspaces/{wid}");

    // People who are not members cannot tell the workspace exists.
    assert_eq!(
        call(&app, Method::GET, &ws, &outsider, None).await.0,
        StatusCode::NOT_FOUND
    );

    // Registered users join at once; unknown addresses get an invitation.
    let invite = |email: &str, role: &str| json!({"email": email, "role": role});
    let (status, joined) = call(
        &app,
        Method::POST,
        &format!("{ws}/members"),
        &owner,
        Some(invite("Admin@Example.com", "admin")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{joined}");
    assert_eq!(joined["member"]["user_id"], admin_id.as_str());
    assert_eq!(joined["invite"], Value::Null);
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("{ws}/members"),
        &admin,
        Some(invite("member@example.com", "member")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        call(
            &app,
            Method::POST,
            &format!("{ws}/members"),
            &admin,
            Some(invite("member@example.com", "member"))
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "already a member"
    );
    let (status, pending) = call(
        &app,
        Method::POST,
        &format!("{ws}/members"),
        &admin,
        Some(invite("later@example.com", "guest")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(pending["invite"]["email"], "later@example.com");

    // Members see the roster but manage nothing; admins manage everyone but owners.
    let (status, roster) = call(&app, Method::GET, &format!("{ws}/members"), &member, None).await;
    assert_eq!(status, StatusCode::OK);
    let roles: Vec<&str> = roster
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles, ["owner", "admin", "member"]);
    for (token, path, body) in [
        (
            &member,
            format!("{ws}/members"),
            Some(invite("x@example.com", "guest")),
        ),
        (&member, format!("{ws}/invites"), None),
        (
            &admin,
            format!("{ws}/members"),
            Some(invite("x@example.com", "owner")),
        ),
    ] {
        let method = if body.is_some() {
            Method::POST
        } else {
            Method::GET
        };
        assert_eq!(
            call(&app, method, &path, token, body).await.0,
            StatusCode::FORBIDDEN,
            "{path}"
        );
    }
    let set_role = |role: &str| Some(json!({"role": role}));
    let member_path = |uid: &str| format!("{ws}/members/{uid}");
    assert_eq!(
        call(
            &app,
            Method::PATCH,
            &member_path(&owner_id),
            &admin,
            set_role("member")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            Method::PATCH,
            &member_path(&member_id),
            &admin,
            set_role("owner")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            Method::PATCH,
            &member_path(&member_id),
            &admin,
            set_role("guest")
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            Method::PATCH,
            &ws,
            &member,
            Some(json!({"name": "Mine"}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            Method::PATCH,
            &ws,
            &admin,
            Some(json!({"name": "Acme"}))
        )
        .await
        .1["name"],
        "Acme"
    );
    assert_eq!(
        call(&app, Method::DELETE, &ws, &admin, None).await.0,
        StatusCode::FORBIDDEN
    );

    // Guests do not see the roster.
    assert_eq!(
        call(&app, Method::GET, &format!("{ws}/members"), &member, None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );

    // A workspace always keeps an owner, until ownership is shared.
    assert_eq!(
        call(
            &app,
            Method::PATCH,
            &member_path(&owner_id),
            &owner,
            set_role("admin")
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(&app, Method::DELETE, &member_path(&owner_id), &owner, None)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            Method::PATCH,
            &member_path(&admin_id),
            &owner,
            set_role("owner")
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, Method::DELETE, &member_path(&owner_id), &owner, None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, Method::GET, &ws, &owner, None).await.0,
        StatusCode::NOT_FOUND,
        "they left"
    );

    // An invited address joins on sign-up instead of getting a personal workspace.
    let (later, _) = user(&app, "later@example.com").await;
    let (_, list) = call(&app, Method::GET, "/workspaces", &later, None).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(
        (list[0]["id"].as_str(), list[0]["role"].as_str()),
        (Some(wid.as_str()), Some("guest"))
    );
    assert_eq!(list[0]["member_count"], 3, "admin, member and the newcomer");
    let (_, invites) = call(&app, Method::GET, &format!("{ws}/invites"), &admin, None).await;
    assert_eq!(invites, json!([]));

    // The only workspace of an account cannot be deleted; a second one can.
    assert_eq!(
        call(&app, Method::DELETE, &ws, &admin, None).await.0,
        StatusCode::NO_CONTENT,
        "admin owns two"
    );
    let own = personal_workspace(&app, &admin).await;
    assert_eq!(
        call(
            &app,
            Method::DELETE,
            &format!("/workspaces/{own}"),
            &admin,
            None
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    // Every account is registered as "Test": the first took the plain slug, later ones a suffix.
    let (_, personal) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{own}"),
        &admin,
        None,
    )
    .await;
    let slug = personal["slug"].as_str().unwrap();
    assert!(
        slug.starts_with("test-s-workspace-") && slug.len() > "test-s-workspace-".len(),
        "a taken slug gets a suffix: {slug}"
    );
    let (status, _) = call(
        &app,
        Method::POST,
        "/workspaces",
        &admin,
        Some(json!({"name": "Second"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        call(
            &app,
            Method::DELETE,
            &format!("/workspaces/{own}"),
            &admin,
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT,
        "with a second workspace the first can go"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn teams_are_public_or_private(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (owner, _) = user(&app, "owner@example.com").await;
    let (member, member_id) = user(&app, "member@example.com").await;
    let (guest, guest_id) = user(&app, "guest@example.com").await;
    let (outsider, outsider_id) = user(&app, "outsider@example.com").await;
    let wid = personal_workspace(&app, &owner).await;
    let ws = format!("/workspaces/{wid}");
    for (email, role) in [
        ("member@example.com", "member"),
        ("guest@example.com", "guest"),
    ] {
        let body = json!({"email": email, "role": role});
        assert_eq!(
            call(
                &app,
                Method::POST,
                &format!("{ws}/members"),
                &owner,
                Some(body)
            )
            .await
            .0,
            StatusCode::CREATED
        );
    }

    // Members create teams and own them; guests cannot.
    let (status, eng) = call(
        &app,
        Method::POST,
        &format!("{ws}/teams"),
        &member,
        Some(json!({"name": "Core Platform"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{eng}");
    assert_eq!(
        (eng["key"].as_str(), eng["role"].as_str()),
        (Some("CP"), Some("owner"))
    );
    let eng_path = format!("{ws}/teams/{}", eng["id"].as_str().unwrap());
    assert_eq!(
        call(
            &app,
            Method::POST,
            &format!("{ws}/teams"),
            &owner,
            Some(json!({"name": "Customer Pilots"}))
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "CP is taken"
    );
    assert_eq!(
        call(
            &app,
            Method::POST,
            &format!("{ws}/teams"),
            &owner,
            Some(json!({"name": "X", "key": "cp"}))
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            Method::POST,
            &format!("{ws}/teams"),
            &guest,
            Some(json!({"name": "Guests"}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, secret) = call(
        &app,
        Method::POST,
        &format!("{ws}/teams"),
        &owner,
        Some(json!({"name": "Leadership", "key": "LEAD", "private": true})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let secret_path = format!("{ws}/teams/{}", secret["id"].as_str().unwrap());

    let visible = |token: String| {
        let (app, path) = (&app, format!("{ws}/teams"));
        async move {
            let (_, teams) = call(app, Method::GET, &path, &token, None).await;
            let mut keys: Vec<String> = teams
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["key"].as_str().unwrap().to_owned())
                .collect();
            keys.sort();
            keys
        }
    };
    assert_eq!(
        visible(owner.clone()).await,
        ["CP", "LEAD"],
        "admins see every team"
    );
    assert_eq!(
        visible(member.clone()).await,
        ["CP"],
        "private teams are hidden from non-members"
    );
    assert!(
        visible(guest.clone()).await.is_empty(),
        "guests see only teams they were added to"
    );
    assert_eq!(
        call(&app, Method::GET, &secret_path, &member, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, Method::GET, &eng_path, &outsider, None).await.0,
        StatusCode::NOT_FOUND
    );

    // Joining: public teams are open to members; guests and private teams need an invitation.
    let me = |path: &str, uid: &str| format!("{path}/members/{uid}");
    assert_eq!(
        call(
            &app,
            Method::PUT,
            &me(&eng_path, &guest_id),
            &guest,
            Some(json!({}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            Method::PUT,
            &me(&secret_path, &member_id),
            &member,
            Some(json!({}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            Method::PUT,
            &me(&eng_path, &guest_id),
            &member,
            Some(json!({}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(visible(guest.clone()).await, ["CP"]);
    assert_eq!(
        call(
            &app,
            Method::PUT,
            &me(&eng_path, &outsider_id),
            &member,
            Some(json!({}))
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY,
        "only workspace members join teams"
    );
    // A team member cannot promote themselves or rename the team.
    assert_eq!(
        call(
            &app,
            Method::PUT,
            &me(&eng_path, &guest_id),
            &guest,
            Some(json!({"role": "owner"}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            Method::PATCH,
            &eng_path,
            &guest,
            Some(json!({"name": "Mine"}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            Method::PATCH,
            &eng_path,
            &member,
            Some(json!({"private": true}))
        )
        .await
        .1["private"],
        true
    );

    // Leaving is always allowed; removing someone from the workspace removes them from its teams.
    assert_eq!(
        call(
            &app,
            Method::DELETE,
            &me(&eng_path, &guest_id),
            &guest,
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            Method::PUT,
            &me(&secret_path, &member_id),
            &owner,
            Some(json!({}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            Method::DELETE,
            &format!("{ws}/members/{member_id}"),
            &owner,
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (_, members) = call(
        &app,
        Method::GET,
        &format!("{secret_path}/members"),
        &owner,
        None,
    )
    .await;
    assert_eq!(members.as_array().unwrap().len(), 1);
    let (_, team) = call(&app, Method::GET, &eng_path, &owner, None).await;
    assert_eq!(
        team["member_count"], 0,
        "its creator left the workspace; admins still manage it"
    );
    assert_eq!(
        call(&app, Method::DELETE, &eng_path, &owner, None).await.0,
        StatusCode::NO_CONTENT
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn graphs_are_shared_with_the_workspace_or_the_team(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (owner, _) = user(&app, "owner@example.com").await;
    let (member, member_id) = user(&app, "member@example.com").await;
    let (guest, guest_id) = user(&app, "guest@example.com").await;
    let (outsider, _) = user(&app, "outsider@example.com").await;
    let wid = personal_workspace(&app, &owner).await;
    let ws = format!("/workspaces/{wid}");
    for (email, role) in [
        ("member@example.com", "member"),
        ("guest@example.com", "guest"),
    ] {
        let body = json!({"email": email, "role": role});
        assert_eq!(
            call(
                &app,
                Method::POST,
                &format!("{ws}/members"),
                &owner,
                Some(body)
            )
            .await
            .0,
            StatusCode::CREATED
        );
    }
    let (_, team) = call(
        &app,
        Method::POST,
        &format!("{ws}/teams"),
        &owner,
        Some(json!({"name": "Leadership", "key": "LEAD", "private": true})),
    )
    .await;
    let tid = team["id"].as_str().unwrap().to_owned();

    // A graph of the workspace is open to its members, not to guests or strangers.
    let (status, shared) = call(
        &app,
        Method::POST,
        "/graphs",
        &owner,
        Some(json!({"name": "Roadmap", "workspace_id": wid})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{shared}");
    assert_eq!(
        (shared["workspace_id"].as_str(), &shared["team_id"]),
        (Some(wid.as_str()), &Value::Null)
    );
    let shared_path = format!("/graphs/{}", shared["id"].as_str().unwrap());
    assert_eq!(
        call(&app, Method::GET, &shared_path, &member, None).await.0,
        StatusCode::OK
    );
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("{shared_path}/nodes"),
        &member,
        Some(json!({"title": "From a teammate"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "members edit shared graphs");
    for stranger in [&guest, &outsider] {
        assert_eq!(
            call(&app, Method::GET, &shared_path, stranger, None)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        let node = Some(json!({"title": "x"}));
        assert_eq!(
            call(
                &app,
                Method::POST,
                &format!("{shared_path}/nodes"),
                stranger,
                node
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }

    // A graph of a team is open to that team only, whatever the workspace role.
    let (status, private) = call(
        &app,
        Method::POST,
        "/graphs",
        &owner,
        Some(json!({"name": "Reorg", "team_id": tid})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{private}");
    assert_eq!(
        private["workspace_id"],
        wid.as_str(),
        "the team decides the workspace"
    );
    let private_path = format!("/graphs/{}", private["id"].as_str().unwrap());
    assert_eq!(
        call(&app, Method::GET, &private_path, &member, None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            Method::POST,
            "/graphs",
            &member,
            Some(json!({"name": "Sneaky", "team_id": tid}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND,
        "a team you are not in does not exist for you"
    );
    let add = |uid: &str| format!("{ws}/teams/{tid}/members/{uid}");
    assert_eq!(
        call(&app, Method::PUT, &add(&guest_id), &owner, Some(json!({})))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, Method::GET, &private_path, &guest, None).await.0,
        StatusCode::OK,
        "a guest in the team"
    );
    assert_eq!(
        call(&app, Method::GET, &shared_path, &guest, None).await.0,
        StatusCode::NOT_FOUND,
        "but nothing else"
    );

    // Guests create graphs only in their teams.
    assert_eq!(
        call(
            &app,
            Method::POST,
            "/graphs",
            &guest,
            Some(json!({"name": "G", "workspace_id": wid}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            Method::POST,
            "/graphs",
            &guest,
            Some(json!({"name": "G", "team_id": tid}))
        )
        .await
        .0,
        StatusCode::CREATED
    );

    // Lists show what the caller can open, and can be narrowed to one workspace.
    let names = |token: String, query: String| {
        let app = &app;
        async move {
            let (_, list) = call(app, Method::GET, &format!("/graphs{query}"), &token, None).await;
            let mut names: Vec<String> = list
                .as_array()
                .unwrap()
                .iter()
                .map(|g| g["name"].as_str().unwrap().to_owned())
                .collect();
            names.sort();
            names
        }
    };
    let (_, own) = call(
        &app,
        Method::POST,
        "/graphs",
        &member,
        Some(json!({"name": "Private notes"})),
    )
    .await;
    assert_ne!(
        own["workspace_id"],
        wid.as_str(),
        "without a workspace, graphs go to the caller's own"
    );
    assert_eq!(
        names(member.clone(), String::new()).await,
        ["Private notes", "Roadmap"]
    );
    assert_eq!(
        names(member.clone(), format!("?workspace_id={wid}")).await,
        ["Roadmap"]
    );
    assert_eq!(
        names(owner.clone(), String::new()).await,
        ["G", "Reorg", "Roadmap"]
    );
    assert_eq!(
        names(guest.clone(), format!("?workspace_id={wid}")).await,
        ["G", "Reorg"]
    );
    assert!(
        names(outsider.clone(), format!("?workspace_id={wid}"))
            .await
            .is_empty()
    );

    // Leaving the workspace ends access; a team with graphs cannot be deleted.
    assert_eq!(
        call(
            &app,
            Method::DELETE,
            &format!("{ws}/members/{member_id}"),
            &owner,
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, Method::GET, &shared_path, &member, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            Method::DELETE,
            &format!("{ws}/teams/{tid}"),
            &owner,
            None
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_workspace_credential_backs_members_without_their_own(pool: PgPool) {
    // No server-wide key: a credential has to come from the user or the workspace.
    let app = TestApp::new(pool, &[("ANTHROPIC_API_KEY", "")]).await;
    let (owner, _) = user(&app, "owner@example.com").await;
    let (member, _) = user(&app, "member@example.com").await;
    let (outsider, _) = user(&app, "outsider@example.com").await;
    let wid = personal_workspace(&app, &owner).await;
    let invite = json!({"email": "member@example.com", "role": "member"});
    call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/members"),
        &owner,
        Some(invite),
    )
    .await;
    let (_, graph) = call(
        &app,
        Method::POST,
        "/graphs",
        &owner,
        Some(json!({"name": "Shared", "goal": "g", "workspace_id": wid})),
    )
    .await;
    let plan_path = format!("/graphs/{}/plan", graph["id"].as_str().unwrap());
    let effective = format!("/settings/llm?workspace_id={wid}");
    let ws_llm = format!("/workspaces/{wid}/llm");

    // Nothing configured: the server default applies and, without a key, planning is refused.
    let (_, settings) = call(&app, Method::GET, &effective, &member, None).await;
    assert_eq!(settings["scope"], "server");
    assert_eq!(
        call(&app, Method::GET, &ws_llm, &member, None).await.1,
        Value::Null
    );
    assert_eq!(
        call(&app, Method::POST, &plan_path, &member, Some(json!({})))
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    // Only admins set the workspace credential; once set it serves every member.
    let demo = json!({"provider": "demo", "model": "demo-1"});
    assert_eq!(
        call(&app, Method::PUT, &ws_llm, &member, Some(demo.clone()))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (status, saved) = call(&app, Method::PUT, &ws_llm, &owner, Some(demo)).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(
        (saved["scope"].as_str(), saved["provider"].as_str()),
        (Some("workspace"), Some("demo"))
    );
    let (_, settings) = call(&app, Method::GET, &effective, &member, None).await;
    assert_eq!(
        (settings["scope"].as_str(), settings["provider"].as_str()),
        (Some("workspace"), Some("demo"))
    );
    assert_eq!(
        call(&app, Method::GET, "/settings/llm", &member, None)
            .await
            .1["scope"],
        "server",
        "outside the workspace"
    );
    assert_eq!(
        call(&app, Method::POST, &plan_path, &member, Some(json!({})))
            .await
            .0,
        StatusCode::ACCEPTED
    );
    let (status, models) = call(
        &app,
        Method::GET,
        &format!("/settings/llm/models?workspace_id={wid}"),
        &member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (
            models["provider"].as_str(),
            models["models"].as_array().map(Vec::len)
        ),
        (Some("demo"), Some(0))
    );
    assert!(models["note"].is_string());

    // A member's own account wins over the workspace's, until they disconnect it.
    let own = json!({"provider": "openai_compatible", "model": "local", "base_url": "http://localhost:11434/v1", "api_key": "sk-user-12345678"});
    assert_eq!(
        call(&app, Method::PUT, "/settings/llm", &member, Some(own))
            .await
            .0,
        StatusCode::OK
    );
    let (_, settings) = call(&app, Method::GET, &effective, &member, None).await;
    assert_eq!(
        (
            settings["scope"].as_str(),
            settings["source"].as_str(),
            settings["key_hint"].as_str()
        ),
        (Some("user"), Some("user"), Some("…5678"))
    );
    assert_eq!(
        call(&app, Method::GET, &effective, &owner, None).await.1["scope"],
        "workspace",
        "others are unaffected"
    );
    assert_eq!(
        call(&app, Method::DELETE, "/settings/llm", &member, None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, Method::GET, &effective, &member, None).await.1["scope"],
        "workspace"
    );

    // The workspace key is stored sealed and never returned.
    let keyed = json!({"provider": "anthropic", "model": "some-model", "api_key": "sk-workspace-secret-9876"});
    assert_eq!(
        call(&app, Method::PUT, &ws_llm, &owner, Some(keyed))
            .await
            .0,
        StatusCode::OK
    );
    let (_, shown) = call(&app, Method::GET, &ws_llm, &member, None).await;
    assert_eq!(
        (shown["has_api_key"].as_bool(), shown["key_hint"].as_str()),
        (Some(true), Some("…9876"))
    );
    assert!(!shown.to_string().contains("sk-workspace"));
    let (_, settings) = call(&app, Method::GET, &effective, &member, None).await;
    assert_eq!(
        (settings["scope"].as_str(), settings["source"].as_str()),
        (Some("workspace"), Some("workspace"))
    );

    // Strangers learn nothing, and removing the credential restores the default.
    assert_eq!(
        call(&app, Method::GET, &ws_llm, &outsider, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, Method::GET, &effective, &outsider, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, Method::DELETE, &ws_llm, &member, None).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, Method::DELETE, &ws_llm, &owner, None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, Method::GET, &effective, &member, None).await.1["scope"],
        "server"
    );
}
