//! Workspaces, roles, invitations and teams: who can see and do what.
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode};
use common::{TestApp, collect_until};
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

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn agents_and_memory_belong_to_the_workspace(pool: PgPool) {
    use nexc::domain::memory::{MemoryKind, MemoryScope};
    use nexc::repo::memories::{self, NewMemory};

    let app = TestApp::new(pool, &[]).await;
    let (owner, owner_id) = user(&app, "owner@example.com").await;
    let (member, _) = user(&app, "member@example.com").await;
    let (guest, guest_id) = user(&app, "guest@example.com").await;
    let (outsider, _) = user(&app, "outsider@example.com").await;
    let wid = personal_workspace(&app, &owner).await;
    let ws = format!("/workspaces/{wid}");
    for (email, role) in [
        ("member@example.com", "member"),
        ("guest@example.com", "guest"),
    ] {
        let body = json!({"email": email, "role": role});
        call(
            &app,
            Method::POST,
            &format!("{ws}/members"),
            &owner,
            Some(body),
        )
        .await;
    }

    // Every workspace starts with the default organisation, shared by its members.
    let agents = format!("/agents?workspace_id={wid}");
    let (_, seeded) = call(&app, Method::GET, &agents, &owner, None).await;
    let seeded = seeded.as_array().unwrap().len();
    assert!(seeded > 0, "a new workspace has agents");
    let new_agent = json!({"name": "Quant", "role": "quant", "workspace_id": wid});
    let (status, quant) = call(
        &app,
        Method::POST,
        "/agents",
        &member,
        Some(new_agent.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{quant}");
    let quant_path = format!("/agents/{}", quant["id"].as_str().unwrap());
    let (_, list) = call(&app, Method::GET, &agents, &owner, None).await;
    assert_eq!(
        list.as_array().unwrap().len(),
        seeded + 1,
        "the owner sees the member's agent"
    );
    assert_eq!(
        call(
            &app,
            Method::PATCH,
            &quant_path,
            &owner,
            Some(json!({"title": "Lead"}))
        )
        .await
        .1["title"],
        "Lead"
    );
    // Guests may look, not change; strangers see nothing at all.
    assert_eq!(
        call(&app, Method::GET, &agents, &guest, None).await.0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            Method::POST,
            "/agents",
            &guest,
            Some(new_agent.clone())
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            Method::PATCH,
            &quant_path,
            &guest,
            Some(json!({"title": "x"}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, Method::GET, &agents, &outsider, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, Method::DELETE, &quant_path, &outsider, None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    // The same name is free in another workspace.
    let (_, mine) = call(&app, Method::GET, "/agents", &member, None).await;
    assert_eq!(
        mine.as_array().unwrap().len(),
        seeded,
        "the member's own workspace is untouched"
    );

    // Memory: what a graph learned is readable by whoever can open the graph.
    let (_, team) = call(
        &app,
        Method::POST,
        &format!("{ws}/teams"),
        &owner,
        Some(json!({"name": "Lead", "private": true})),
    )
    .await;
    let tid = team["id"].as_str().unwrap();
    let (_, shared) = call(
        &app,
        Method::POST,
        "/graphs",
        &owner,
        Some(json!({"name": "Shared", "workspace_id": wid})),
    )
    .await;
    let (_, secret) = call(
        &app,
        Method::POST,
        "/graphs",
        &owner,
        Some(json!({"name": "Secret", "team_id": tid})),
    )
    .await;
    let learn = |graph: &Value, content: &'static str| {
        let (db, owner_id, wid) = (app.state.db.clone(), owner_id.clone(), wid.clone());
        let graph_id = graph["id"].as_str().unwrap().parse().unwrap();
        async move {
            let embedding = nexc::kernel::embed(content);
            let new = NewMemory {
                owner_id: owner_id.parse().unwrap(),
                workspace_id: Some(wid.parse().unwrap()),
                scope: MemoryScope::Graph,
                graph_id: Some(graph_id),
                node_id: None,
                kind: MemoryKind::Fact,
                content,
                embedding: &embedding,
                importance: 0.8,
            };
            memories::insert(&db, &new).await.unwrap()
        }
    };
    let shared_memory = learn(&shared, "Gold reacts to real yields and the dollar index").await;
    learn(&secret, "The reorganisation is announced in March").await;

    let contents = |token: String, query: String| {
        let app = &app;
        async move {
            let (status, list) =
                call(app, Method::GET, &format!("/memories{query}"), &token, None).await;
            assert_eq!(status, StatusCode::OK, "{list}");
            let mut contents: Vec<String> = list
                .as_array()
                .unwrap()
                .iter()
                .map(|m| m["content"].as_str().unwrap()[..4].to_owned())
                .collect();
            contents.sort();
            contents
        }
    };
    let in_ws = format!("?workspace_id={wid}");
    assert_eq!(
        contents(owner.clone(), in_ws.clone()).await,
        ["Gold", "The "]
    );

    // A list pages and can carry previews; a memory is read in full by those who may read it.
    let paged = format!("/memories{in_ws}&limit=1&offset=1&preview=20");
    let (_, page) = call(&app, Method::GET, &paged, &owner, None).await;
    assert_eq!(page.as_array().unwrap().len(), 1, "{page}");
    assert_eq!(page[0]["content"], "Gold reacts to real…");
    let beyond = format!("/memories{in_ws}&offset=2");
    let (_, none) = call(&app, Method::GET, &beyond, &owner, None).await;
    assert_eq!(none.as_array().unwrap().len(), 0);
    let one = format!("/memories/{shared_memory}");
    let (status, full) = call(&app, Method::GET, &one, &member, None).await;
    assert_eq!(status, StatusCode::OK, "{full}");
    assert_eq!(
        full["content"],
        "Gold reacts to real yields and the dollar index"
    );
    let (status, _) = call(&app, Method::GET, &one, &outsider, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        contents(member.clone(), in_ws.clone()).await,
        ["Gold"],
        "a teammate reads the shared graph's memory"
    );
    assert_eq!(
        contents(member.clone(), format!("{in_ws}&q=reorganisation%20march")).await,
        ["Gold"],
        "search does not leak the private team"
    );
    assert!(contents(guest.clone(), in_ws.clone()).await.is_empty());
    assert!(
        contents(member.clone(), String::new()).await.is_empty(),
        "nothing in their own workspace"
    );
    assert_eq!(
        call(
            &app,
            Method::GET,
            &format!("/memories{in_ws}"),
            &outsider,
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    // Joining the team opens its memory.
    call(
        &app,
        Method::PUT,
        &format!("{ws}/teams/{tid}/members/{guest_id}"),
        &owner,
        Some(json!({})),
    )
    .await;
    assert_eq!(contents(guest.clone(), in_ws.clone()).await, ["The "]);
    // Anyone who can work on the graph may forget what it learned; others cannot.
    let memory_path = format!("/memories/{shared_memory}");
    assert_eq!(
        call(&app, Method::DELETE, &memory_path, &guest, None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, Method::DELETE, &memory_path, &member, None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        contents(owner.clone(), in_ws).await,
        ["The "],
        "the index was refreshed"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn usage_is_booked_to_the_member_and_the_paying_account(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (owner, _) = user(&app, "owner@example.com").await;
    let (member, member_id) = user(&app, "member@example.com").await;
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
    let usage = format!("/workspaces/{wid}/usage");

    let (status, empty) = call(&app, Method::GET, &usage, &owner, None).await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert_eq!(
        (empty["totals"]["calls"].as_i64(), empty["days"].as_i64()),
        (Some(0), Some(30))
    );
    assert_eq!(empty["by_day"], json!([]));

    // The member runs a shared graph; the test server's key pays.
    let (_, graph) = call(
        &app,
        Method::POST,
        "/graphs",
        &owner,
        Some(json!({"name": "Shared", "workspace_id": wid})),
    )
    .await;
    let gid = graph["id"].as_str().unwrap().to_owned();
    app.node(&member, &gid, json!({"title": "Only step"})).await;
    let mut events = app.events(&gid);
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/graphs/{gid}/runs"),
        &member,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let finished = collect_until(&mut events, "run.finished").await;
    let run = &finished.last().unwrap().1["run"];
    assert_eq!(run["status"], "succeeded");
    let (run_in, run_out) = (
        run["tokens_in"].as_i64().unwrap(),
        run["tokens_out"].as_i64().unwrap(),
    );
    assert!(run_in > 0 && run_out > 0);

    let (status, report) = call(&app, Method::GET, &format!("{usage}?days=7"), &owner, None).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(
        (report["scope"].as_str(), report["days"].as_i64()),
        (Some("workspace"), Some(7))
    );
    let slice = |group: &str, key: &dyn Fn(&Value) -> bool| -> Value {
        report[group]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| key(&s["key"]))
            .cloned()
            .unwrap_or(Value::Null)
    };
    let node = slice("by_purpose", &|k| k == "node");
    assert_eq!(
        (
            node["calls"].as_i64(),
            node["tokens_in"].as_i64(),
            node["tokens_out"].as_i64()
        ),
        (Some(1), Some(run_in), Some(run_out))
    );
    assert!(node["cost_usd"].as_f64().unwrap() > 0.0);
    let by_member = slice("by_member", &|k| k["user_id"] == member_id.as_str());
    assert_eq!(by_member["key"]["name"], "Test");
    assert!(
        by_member["calls"].as_i64().unwrap() >= 1,
        "booked to who ran it, not who owns the graph"
    );
    assert_eq!(report["by_member"].as_array().unwrap().len(), 1);
    assert!(
        slice("by_credential", &|k| k == "server")["calls"]
            .as_i64()
            .unwrap()
            >= 1
    );
    assert_eq!(report["by_day"].as_array().unwrap().len(), 1);
    assert!(report["totals"]["tokens_in"].as_i64().unwrap() >= run_in);
    assert!(!report["by_model"].as_array().unwrap().is_empty());

    // Members see their own usage only; the owner spent nothing; strangers get nothing.
    let (_, own) = call(&app, Method::GET, &usage, &member, None).await;
    assert_eq!(own["scope"], "own");
    assert!(own["totals"]["calls"].as_i64().unwrap() >= 1);
    let invite = json!({"email": "outsider@example.com", "role": "member"});
    call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/members"),
        &owner,
        Some(invite),
    )
    .await;
    let (_, idle) = call(&app, Method::GET, &usage, &outsider, None).await;
    assert_eq!(
        (idle["scope"].as_str(), idle["totals"]["calls"].as_i64()),
        (Some("own"), Some(0))
    );
    assert_eq!(
        idle["by_member"],
        json!([]),
        "another member's usage is not shown"
    );
    let (stranger, _) = user(&app, "stranger@example.com").await;
    assert_eq!(
        call(&app, Method::GET, &usage, &stranger, None).await.0,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn guardrails_stop_work_before_it_spends(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (owner, _) = user(&app, "owner@example.com").await;
    let (member, _) = user(&app, "member@example.com").await;
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
    let rails = format!("/workspaces/{wid}/guardrails");

    // Defaults allow everything; only admins change the policy, and it is validated.
    let (_, defaults) = call(&app, Method::GET, &rails, &member, None).await;
    assert_eq!(
        defaults,
        json!({"monthly_token_budget": null, "member_monthly_token_budget": null,
        "allowed_providers": [], "allow_code_exec": true, "redact_secrets": true,
            "memory_limit": null, "memory_forget_after_days": null})
    );
    assert_eq!(
        call(&app, Method::PUT, &rails, &member, Some(defaults.clone()))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    for bad in [
        json!({"monthly_token_budget": -1}),
        json!({"allowed_providers": ["nope"]}),
        json!({"typo": true}),
    ] {
        assert_eq!(
            call(&app, Method::PUT, &rails, &owner, Some(bad)).await.0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let (status, saved) = call(
        &app,
        Method::PUT,
        &rails,
        &owner,
        Some(json!({"member_monthly_token_budget": 50})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(
        (
            saved["member_monthly_token_budget"].as_i64(),
            saved["redact_secrets"].as_bool()
        ),
        (Some(50), Some(true))
    );

    // The member's first run fits the budget and spends it; the next one is refused, as is planning.
    let (_, graph) = call(
        &app,
        Method::POST,
        "/graphs",
        &owner,
        Some(json!({"name": "Shared", "goal": "g", "workspace_id": wid})),
    )
    .await;
    let gid = graph["id"].as_str().unwrap().to_owned();
    app.node(&member, &gid, json!({"title": "Only step"})).await;
    let runs = format!("/graphs/{gid}/runs");
    let mut events = app.events(&gid);
    assert_eq!(
        call(&app, Method::POST, &runs, &member, Some(json!({})))
            .await
            .0,
        StatusCode::ACCEPTED
    );
    collect_until(&mut events, "run.finished").await;
    let (status, refused) = call(
        &app,
        Method::POST,
        &runs,
        &member,
        Some(json!({"force": true})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    assert!(
        refused["detail"]
            .as_str()
            .unwrap()
            .contains("your monthly budget of 50 tokens"),
        "{refused}"
    );
    assert_eq!(
        call(
            &app,
            Method::POST,
            &format!("/graphs/{gid}/plan"),
            &member,
            Some(json!({}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    // The budget is per member: the owner has spent nothing.
    let mut events = app.events(&gid);
    assert_eq!(
        call(
            &app,
            Method::POST,
            &runs,
            &owner,
            Some(json!({"force": true}))
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
    collect_until(&mut events, "run.finished").await;

    // A workspace budget covers everyone; a provider allow-list refuses the rest.
    call(
        &app,
        Method::PUT,
        &rails,
        &owner,
        Some(json!({"monthly_token_budget": 10})),
    )
    .await;
    let (status, refused) = call(
        &app,
        Method::POST,
        &runs,
        &owner,
        Some(json!({"force": true})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        refused["detail"]
            .as_str()
            .unwrap()
            .contains("workspace has used its monthly budget")
    );
    call(
        &app,
        Method::PUT,
        &rails,
        &owner,
        Some(json!({"allowed_providers": ["demo"]})),
    )
    .await;
    let (status, refused) = call(
        &app,
        Method::POST,
        &runs,
        &owner,
        Some(json!({"force": true})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        refused["detail"]
            .as_str()
            .unwrap()
            .contains("only allows the providers demo"),
        "{refused}"
    );
    // Lifting the policy lets work through again.
    call(&app, Method::PUT, &rails, &owner, Some(json!({}))).await;
    assert_eq!(
        call(
            &app,
            Method::POST,
            &runs,
            &owner,
            Some(json!({"force": true}))
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_audit_log_records_who_changed_the_workspace(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (owner, _) = user(&app, "owner@example.com").await;
    let (member, member_id) = user(&app, "member@example.com").await;
    let wid = personal_workspace(&app, &owner).await;
    let ws = format!("/workspaces/{wid}");

    // A new workspace has an empty log.
    let (status, log) = call(&app, Method::GET, &format!("{ws}/audit"), &owner, None).await;
    assert_eq!((status, log.as_array().unwrap().len()), (StatusCode::OK, 0));

    let invite = json!({"email": "member@example.com", "role": "member"});
    call(
        &app,
        Method::POST,
        &format!("{ws}/members"),
        &owner,
        Some(invite),
    )
    .await;
    let later = json!({"email": "later@example.com", "role": "guest"});
    call(
        &app,
        Method::POST,
        &format!("{ws}/members"),
        &owner,
        Some(later),
    )
    .await;
    let (_, team) = call(
        &app,
        Method::POST,
        &format!("{ws}/teams"),
        &member,
        Some(json!({"name": "Engineering", "key": "ENG"})),
    )
    .await;
    let promote = json!({"role": "admin"});
    let one = format!("{ws}/members/{member_id}");
    call(&app, Method::PATCH, &one, &owner, Some(promote.clone())).await;
    // Setting the same role again is not a change.
    call(&app, Method::PATCH, &one, &owner, Some(promote)).await;
    let guardrails = json!({"monthly_token_budget": 1000});
    let (status, body) = call(
        &app,
        Method::PUT,
        &format!("{ws}/guardrails"),
        &owner,
        Some(guardrails),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let team_path = format!("{ws}/teams/{}", team["id"].as_str().unwrap());
    let (status, _) = call(&app, Method::DELETE, &team_path, &owner, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // A refused change leaves no entry.
    let (status, _) = call(&app, Method::DELETE, &team_path, &owner, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, log) = call(&app, Method::GET, &format!("{ws}/audit"), &owner, None).await;
    assert_eq!(status, StatusCode::OK);
    let seen: Vec<(&str, &str, &str, &str)> = log
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .map(|e| {
            (
                e["action"].as_str().unwrap(),
                e["actor_name"].as_str().unwrap(),
                e["subject"].as_str().unwrap(),
                e["detail"].as_str().unwrap(),
            )
        })
        .collect();
    let (owner_name, member_name) = (seen[0].1, seen[2].1);
    assert!(!owner_name.is_empty() && !member_name.is_empty());
    let created = log
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["action"] == "team_created");
    assert_eq!(created.unwrap()["actor_id"], member_id.as_str());
    let member_subject = format!("{member_name} <member@example.com>");
    assert_eq!(
        seen,
        vec![
            (
                "member_added",
                owner_name,
                member_subject.as_str(),
                "as member"
            ),
            (
                "member_invited",
                owner_name,
                "later@example.com",
                "as guest"
            ),
            ("team_created", member_name, "Engineering", ""),
            (
                "member_role_changed",
                owner_name,
                member_subject.as_str(),
                "member -> admin"
            ),
            ("guardrails_changed", owner_name, "Guardrails", ""),
            ("team_deleted", owner_name, "Engineering", ""),
        ]
    );

    // Paging: entries older than the newest one.
    let newest = log[0]["created_at"].as_str().unwrap();
    let page = format!("{ws}/audit?limit=2&before={}", newest.replace('+', "%2B"));
    let (_, older) = call(&app, Method::GET, &page, &owner, None).await;
    let actions: Vec<&str> = older
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert_eq!(actions, ["guardrails_changed", "member_role_changed"]);

    // Admins read it; plain members and outsiders do not.
    let (status, _) = call(&app, Method::GET, &format!("{ws}/audit"), &member, None).await;
    assert_eq!(status, StatusCode::OK);
    let demote = json!({"role": "member"});
    call(&app, Method::PATCH, &one, &owner, Some(demote)).await;
    let (status, _) = call(&app, Method::GET, &format!("{ws}/audit"), &member, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (outsider, _) = user(&app, "outsider@example.com").await;
    let (status, _) = call(&app, Method::GET, &format!("{ws}/audit"), &outsider, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn owners_see_what_happened_and_how_things_relate(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (owner, _) = user(&app, "owner@example.com").await;
    let (member, _) = user(&app, "member@example.com").await;
    let wid = personal_workspace(&app, &owner).await;
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
    let (_, team) = call(
        &app,
        Method::POST,
        &format!("{ws}/teams"),
        &owner,
        Some(json!({"name": "Engineering", "key": "ENG"})),
    )
    .await;
    let issues = format!("{ws}/teams/{}/issues", team["id"].as_str().unwrap());
    let (_, parent) = call(
        &app,
        Method::POST,
        &issues,
        &member,
        Some(json!({"title": "Ship login"})),
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

    // Today's timeline, newest first, across the audit log and the issues.
    let (status, timeline) = call(&app, Method::GET, &format!("{ws}/timeline"), &owner, None).await;
    assert_eq!(status, StatusCode::OK, "{timeline}");
    let kinds: Vec<&str> = timeline
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        [
            "issue_comment",
            "issue_created",
            "issue_created",
            "team_created",
            "member_added"
        ]
    );
    assert_eq!(timeline[0]["title"], "ENG-1 Ship login");
    assert_eq!(timeline[0]["detail"], "Looks good");
    assert_eq!(timeline[0]["entity_id"], parent["id"]);
    assert!(timeline[1]["actor"].is_string());
    // Another day is empty; the last days show today's count.
    let (_, old) = call(
        &app,
        Method::GET,
        &format!("{ws}/timeline?day=2020-01-01"),
        &owner,
        None,
    )
    .await;
    assert_eq!(old.as_array().unwrap().len(), 0);
    let (_, days) = call(
        &app,
        Method::GET,
        &format!("{ws}/timeline/days?days=7"),
        &owner,
        None,
    )
    .await;
    assert_eq!(days.as_array().unwrap().len(), 1, "{days}");
    assert_eq!(days[0]["events"], 5);

    // The map counts the kinds of things and the ties between them.
    let (status, map) = call(&app, Method::GET, &format!("{ws}/map"), &owner, None).await;
    assert_eq!(status, StatusCode::OK, "{map}");
    let count = |list: &str, pick: &dyn Fn(&Value) -> bool| -> i64 {
        map[list]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| pick(e))
            .unwrap()["count"]
            .as_i64()
            .unwrap()
    };
    assert_eq!(count("entities", &|e| e["key"] == "member"), 2);
    assert_eq!(count("entities", &|e| e["key"] == "issue"), 2);
    assert_eq!(
        count("relations", &|r| r["from"] == "issue" && r["to"] == "issue"),
        1,
        "one sub-issue"
    );
    assert_eq!(
        count("relations", &|r| r["from"] == "member" && r["to"] == "team"),
        1
    );

    // All of it is for admins and owners; the server's infrastructure for its administrators.
    for path in ["timeline", "timeline/days", "map"] {
        let (status, _) = call(&app, Method::GET, &format!("{ws}/{path}"), &member, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
    }
    let (status, _) = call(&app, Method::GET, "/admin/infrastructure", &owner, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let check = json!({"url": "postgres://nobody@127.0.0.1:9/none"});
    let (status, _) = call(
        &app,
        Method::POST,
        "/admin/infrastructure/check",
        &owner,
        Some(check),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
