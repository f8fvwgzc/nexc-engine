//! nexc against a real onto-intelli (`make test-onto-live`): `onto serve`
//! from the sibling checkout, on PostgreSQL with the configuration nexc
//! ships (`deploy/onto`). Ignored unless `NEXC_ONTO_LIVE_URL` and
//! `NEXC_ONTO_LIVE_TOKEN` name the server; CI runs the same flows against
//! the in-memory stub (`tests/ontology.rs`).
//!
//! What only the real server can show: tenants are created on first use,
//! the impersonation headers are accepted within `impersonate_roles` and
//! audited as the member, governance holds (a guest is a viewer, the
//! assistant only proposes and an approver approves), browser sessions are
//! the member's own, and restricted entities stay with whom they belong to.
#![forbid(unsafe_code)]

mod common;

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use common::TestApp;
use nexc::domain::workspace::WorkspaceRole;
use nexc::ontology::grpc::GrpcOntology;
use nexc::ontology::{
    FindQuery, OntoError, OntologyBackend, Principal, export, migrate, tenant_of, workspace_name,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// The live server, or `None` when the test is not run against one.
fn live() -> Option<(String, String)> {
    let url = std::env::var("NEXC_ONTO_LIVE_URL").ok()?;
    let token = std::env::var("NEXC_ONTO_LIVE_TOKEN").ok()?;
    Some((url, token))
}

async fn app(pool: PgPool) -> (TestApp, Arc<GrpcOntology>, String) {
    let (url, token) =
        live().expect("NEXC_ONTO_LIVE_URL and NEXC_ONTO_LIVE_TOKEN (make test-onto-live)");
    let client = Arc::new(GrpcOntology::new(&url, Some(&token)).unwrap());
    let app = TestApp::with_ontology(
        pool,
        &[
            ("NEXC_ONTO_PUBLIC_URL", url.as_str()),
            ("NEXC_ONTO_VOCABULARY_TTL_SECS", "0"),
        ],
        client.clone(),
    )
    .await;
    (app, client, url)
}

async fn user(app: &TestApp, email: &str) -> (String, Uuid) {
    let (token, _) = app.register(email).await;
    let me = app
        .request(Method::GET, "/api/v1/auth/me", Some(&token), None)
        .await;
    (token, me.body["id"].as_str().unwrap().parse().unwrap())
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

struct World {
    wid: Uuid,
    ws: String,
    owner: String,
    owner_id: Uuid,
    member: String,
    member_id: Uuid,
    guest: String,
}

async fn world(app: &TestApp, tag: &str) -> World {
    let (owner, owner_id) = user(app, &format!("owner-{tag}@example.com")).await;
    let (member, member_id) = user(app, &format!("member-{tag}@example.com")).await;
    let (guest, _) = user(app, &format!("guest-{tag}@example.com")).await;
    let (_, list) = call(app, Method::GET, "/workspaces", &owner, None).await;
    let wid: Uuid = list[0]["id"].as_str().unwrap().parse().unwrap();
    let ws = format!("/workspaces/{wid}");
    for (email, role) in [
        (format!("member-{tag}@example.com"), "member"),
        (format!("guest-{tag}@example.com"), "guest"),
    ] {
        call(
            app,
            Method::POST,
            &format!("{ws}/members"),
            &owner,
            Some(json!({"email": email, "role": role})),
        )
        .await;
    }
    World {
        wid,
        ws,
        owner,
        owner_id,
        member,
        member_id,
        guest,
    }
}

/// `GET /v1/auth/whoami` with a browser session, as the embedded UI asks it.
async fn whoami(url: &str, session: &str) -> Value {
    reqwest::Client::new()
        .get(format!("{url}/v1/auth/whoami"))
        .bearer_auth(session)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
#[ignore = "needs a running onto serve: make test-onto-live"]
async fn live_vocabularies_are_kept_in_the_workspaces_tenant_as_its_members(pool: PgPool) {
    let (app, client, _) = app(pool).await;
    let w = world(&app, "vocab").await;
    let (status, graph) = call(
        &app,
        Method::POST,
        "/graphs",
        &w.owner,
        Some(json!({"name": "Launch", "workspace_id": w.wid})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{graph}");
    let gid = graph["id"].as_str().unwrap().to_owned();
    assert_eq!(graph["ontology"]["node_types"].as_array().unwrap().len(), 6);

    let mut vocabulary = graph["ontology"].clone();
    vocabulary["node_types"]
        .as_array_mut()
        .unwrap()
        .push(json!({"key": "risk", "label": "Risk", "default_role": "analyst", "stage": 1}));
    let (status, put) = call(
        &app,
        Method::PUT,
        &format!("/graphs/{gid}/ontology"),
        &w.member,
        Some(vocabulary),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{put}");

    // What onto-intelli holds, read back as the run engine.
    let runtime = Principal::runtime(&app.state.settings.onto_workspace_prefix, w.wid);
    let stored = client.vocabulary(&runtime, &gid).await.unwrap();
    assert!(stored.node_type("risk").is_some());
    assert_eq!(stored.node_type("risk").unwrap().stage, 1);

    // onto-intelli audited the member, not the service.
    let member = Principal::user(
        &app.state.settings.onto_workspace_prefix,
        w.wid,
        w.member_id,
        WorkspaceRole::Member,
    );
    let events = client
        .events(
            &member,
            nexc::ontology::EventQuery {
                actor: Some(format!("user:{}", w.member_id)),
                limit: 50,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(
        !events.is_empty(),
        "the member's write is on record as theirs"
    );

    // A guest is a viewer there: onto-intelli refuses their write.
    let (status, team) = call(
        &app,
        Method::POST,
        &format!("{}/teams", w.ws),
        &w.owner,
        Some(json!({"name": "Ops", "key": "OPS"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{team}");
    let (_, team_graph) = call(
        &app,
        Method::POST,
        "/graphs",
        &w.owner,
        Some(json!({"name": "Ops plan", "team_id": team["id"]})),
    )
    .await;
    let (_, members) = call(
        &app,
        Method::GET,
        &format!("{}/members", w.ws),
        &w.owner,
        None,
    )
    .await;
    let guest_id = members
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "guest")
        .unwrap()["user_id"]
        .as_str()
        .unwrap()
        .to_owned();
    call(
        &app,
        Method::PUT,
        &format!(
            "{}/teams/{}/members/{guest_id}",
            w.ws,
            team["id"].as_str().unwrap()
        ),
        &w.owner,
        Some(json!({"role": "member"})),
    )
    .await;
    let team_gid = team_graph["id"].as_str().unwrap();
    let (status, seen) = call(
        &app,
        Method::GET,
        &format!("/graphs/{team_gid}"),
        &w.guest,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{seen}");
    let (status, refused) = call(
        &app,
        Method::PUT,
        &format!("/graphs/{team_gid}/ontology"),
        &w.guest,
        Some(seen["ontology"].clone()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
#[ignore = "needs a running onto serve: make test-onto-live"]
async fn live_sessions_are_the_members_own_and_bound_to_their_tenant(pool: PgPool) {
    let (app, _, url) = app(pool).await;
    let w = world(&app, "session").await;
    let session = format!("{}/ontology/session", w.ws);
    let (status, owner) = call(&app, Method::POST, &session, &w.owner, None).await;
    assert_eq!(status, StatusCode::OK, "{owner}");
    assert_eq!(owner["ui_url"], format!("{url}/ui/"));
    let who = whoami(&url, owner["token"].as_str().unwrap()).await;
    assert_eq!(who["principal"]["id"], format!("user:{}", w.owner_id));
    assert_eq!(who["tenant"], tenant_of(w.wid));
    assert_eq!(who["capabilities"]["approve"], true);
    assert_eq!(who["capabilities"]["apply_schema"], true);

    let (_, member) = call(&app, Method::POST, &session, &w.member, None).await;
    let who = whoami(&url, member["token"].as_str().unwrap()).await;
    assert_eq!(who["principal"]["id"], format!("user:{}", w.member_id));
    assert_eq!(who["capabilities"]["approve"], false);
    assert_eq!(
        who["capabilities"]["write"], false,
        "the browser reads; the backend writes"
    );

    // Another tenant is refused with this session.
    let other = reqwest::Client::new()
        .get(format!("{url}/v1/auth/whoami"))
        .bearer_auth(member["token"].as_str().unwrap())
        .header("x-onto-tenant", tenant_of(Uuid::now_v7()))
        .send()
        .await
        .unwrap();
    assert_eq!(other.status().as_u16(), 403);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
#[ignore = "needs a running onto serve: make test-onto-live"]
async fn live_the_assistant_proposes_and_only_an_approver_applies(pool: PgPool) {
    let (app, client, url) = app(pool).await;
    let w = world(&app, "assistant").await;
    let (status, reply) = call(
        &app,
        Method::POST,
        &format!("{}/assistant", w.ws),
        &w.member,
        Some(json!({"message": "propose schema: reviews"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let id = reply["proposals"][0]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("a proposal was filed: {reply}"))
        .to_owned();

    // The assistant may not approve its own proposal, nor may anyone as an agent.
    let prefix = &app.state.settings.onto_workspace_prefix;
    let assistant = Principal::assistant(prefix, w.wid, w.member_id);
    let agent_session = client.mint_session(&assistant, 600).await.unwrap();
    let approve = |token: String| {
        let url = format!("{url}/v1/schema/proposals/{id}/approve");
        async move {
            reqwest::Client::new()
                .post(url)
                .bearer_auth(token)
                .json(&json!({"comment": "ok"}))
                .send()
                .await
                .unwrap()
        }
    };
    assert_eq!(approve(agent_session.token).await.status().as_u16(), 403);
    // A member is no approver either.
    let (_, member) = call(
        &app,
        Method::POST,
        &format!("{}/ontology/session", w.ws),
        &w.member,
        None,
    )
    .await;
    assert_eq!(
        approve(member["token"].as_str().unwrap().to_owned())
            .await
            .status()
            .as_u16(),
        403
    );
    // The owner approves in onto-intelli: it is applied.
    let (_, owner) = call(
        &app,
        Method::POST,
        &format!("{}/ontology/session", w.ws),
        &w.owner,
        None,
    )
    .await;
    let approved = approve(owner["token"].as_str().unwrap().to_owned()).await;
    assert!(
        approved.status().is_success(),
        "{:?}",
        approved.text().await
    );
    let (_, status) = call(
        &app,
        Method::GET,
        &format!("{}/ontology", w.ws),
        &w.member,
        None,
    )
    .await;
    assert!(
        status["proposals"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["id"] != id.as_str()),
        "no longer open: {status}"
    );
    let schema = client.schema(&assistant).await.unwrap();
    assert!(schema["link_types"]["reviews"].is_object(), "applied");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
#[ignore = "needs a running onto serve: make test-onto-live"]
async fn live_the_workspace_is_projected_and_restricted_where_it_belongs(pool: PgPool) {
    let (app, client, _) = app(pool.clone()).await;
    let w = world(&app, "projection").await;
    let (_, private) = call(
        &app,
        Method::POST,
        &format!("{}/teams", w.ws),
        &w.owner,
        Some(json!({"name": "Leadership", "key": "LEAD", "private": true})),
    )
    .await;
    let (_, public) = call(
        &app,
        Method::POST,
        &format!("{}/teams", w.ws),
        &w.owner,
        Some(json!({"name": "Engineering", "key": "ENG"})),
    )
    .await;
    let (status, issue) = call(
        &app,
        Method::POST,
        &format!("{}/teams/{}/issues", w.ws, public["id"].as_str().unwrap()),
        &w.owner,
        Some(json!({"title": "Fix login", "assignee_id": w.member_id})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{issue}");
    // A graph from the release before, with an ontology of its own: copied once.
    let legacy = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO graphs (id, owner_id, name, workspace_id, ontology) VALUES ($1, $2, 'Old', $3, $4)",
    )
    .bind(legacy)
    .bind(w.owner_id)
    .bind(w.wid)
    .bind(json!({"node_types": [{"key": "signal", "label": "Signal", "default_role": "analyst"}],
                 "relation_types": [{"key": "confirms", "label": "Confirms", "blocking": true}]}))
    .execute(&pool)
    .await
    .unwrap();

    let progress = export::pass(&app.state).await.unwrap();
    assert_eq!(progress.failed, 0, "{progress:?}");
    let report = migrate::run(&app.state).await.unwrap();
    assert_eq!(report.failed, 0, "{report:?}");
    let prefix = &app.state.settings.onto_workspace_prefix;
    let service = Principal::service(prefix, w.wid);
    let copied = client
        .vocabulary(&service, &legacy.to_string())
        .await
        .unwrap();
    assert!(copied.relation_type("confirms").unwrap().blocking);

    let owner = Principal::user(prefix, w.wid, w.owner_id, WorkspaceRole::Owner);
    let member = Principal::user(prefix, w.wid, w.member_id, WorkspaceRole::Member);
    let teams = |p: Principal| {
        let client = client.clone();
        async move {
            client
                .find(
                    &p,
                    FindQuery {
                        kind: "team".into(),
                        limit: 50,
                        ..FindQuery::default()
                    },
                )
                .await
                .unwrap()
                .into_iter()
                .map(|e| e.id)
                .collect::<Vec<_>>()
        }
    };
    let secret = format!("team:{}", private["id"].as_str().unwrap());
    assert!(
        teams(owner.clone()).await.contains(&secret),
        "admins see private teams"
    );
    assert!(
        !teams(member.clone()).await.contains(&secret),
        "a member outside the team does not"
    );
    let issues = client
        .find(
            &member,
            FindQuery {
                kind: "issue".into(),
                limit: 50,
                ..FindQuery::default()
            },
        )
        .await
        .unwrap();
    let fix = issues
        .iter()
        .find(|e| e.id == format!("issue:{}", issue["id"].as_str().unwrap()))
        .expect("the issue is on the map");
    assert_eq!(fix.title, "ENG-1 Fix login");
    assert_eq!(fix.props["identifier"], "ENG-1");
    assert_eq!(
        workspace_name(prefix, w.wid),
        service.workspace,
        "one onto workspace per nexc workspace, in its own tenant"
    );
    // Nothing reaches onto-intelli as an unknown principal.
    let stranger = GrpcOntology::new(&live().unwrap().0, Some("not-the-token-0123456789")).unwrap();
    assert!(matches!(
        stranger.vocabulary(&service, &legacy.to_string()).await,
        Err(OntoError::Denied(_))
    ));
}

/// `POST /v1/call/{action}` with a browser session, as the embedded UI (or anyone holding the
/// session) could send it.
async fn act(url: &str, session: &str, workspace: &str, action: &str, input: Value) -> u16 {
    reqwest::Client::new()
        .post(format!("{url}/v1/call/{action}"))
        .bearer_auth(session)
        .header("x-onto-workspace", workspace)
        .json(&input)
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
#[ignore = "needs a running onto serve: make test-onto-live"]
async fn live_a_members_session_cannot_write_what_nexc_projects(pool: PgPool) {
    let (app, client, url) = app(pool).await;
    let w = world(&app, "readonly").await;
    let prefix = app.state.settings.onto_workspace_prefix.clone();
    let onto_ws = workspace_name(&prefix, w.wid);
    // A private team with a graph of its own types, projected.
    let (_, private) = call(
        &app,
        Method::POST,
        &format!("{}/teams", w.ws),
        &w.owner,
        Some(json!({"name": "Leadership", "key": "LEAD", "private": true})),
    )
    .await;
    let (status, graph) = call(
        &app,
        Method::POST,
        "/graphs",
        &w.owner,
        Some(json!({"name": "Reorg", "team_id": private["id"]})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{graph}");
    let gid = graph["id"].as_str().unwrap().to_owned();
    let mut vocabulary = graph["ontology"].clone();
    vocabulary["node_types"]
        .as_array_mut()
        .unwrap()
        .push(json!({"key": "risk", "label": "Risk", "default_role": "analyst", "stage": 1}));
    let (status, put) = call(
        &app,
        Method::PUT,
        &format!("/graphs/{gid}/ontology"),
        &w.owner,
        Some(vocabulary),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{put}");
    app.node(&w.owner, &gid, json!({"title": "Assess", "kind": "risk"}))
        .await;
    let progress = export::pass(&app.state).await.unwrap();
    assert_eq!(progress.failed, 0, "{progress:?}");

    // The member's own session: they cannot put themselves on an acl...
    let (_, member) = call(
        &app,
        Method::POST,
        &format!("{}/ontology/session", w.ws),
        &w.member,
        None,
    )
    .await;
    let token = member["token"].as_str().unwrap();
    let me = format!("person:{}", w.member_id);
    for team in [
        "team:admins".to_owned(),
        format!("team:{}", private["id"].as_str().unwrap()),
    ] {
        let status = act(
            &url,
            token,
            &onto_ws,
            "link.create",
            json!({"from": me, "rel": "member_of", "to": team}),
        )
        .await;
        assert!(status == 403 || status == 404, "{team}: {status}");
    }
    // ...nor rewrite a graph's vocabulary, nor one of its types.
    let rewritten = reqwest::Client::new()
        .put(format!("{url}/v1/execution/graphs/{gid}/vocabulary"))
        .bearer_auth(token)
        .header("x-onto-workspace", &onto_ws)
        .json(&json!({"node_types": [], "relation_types": []}))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16();
    assert!(rewritten == 403 || rewritten == 404, "{rewritten}");
    let upserted = act(
        &url,
        token,
        &onto_ws,
        "entity.upsert",
        json!({"id": format!("node_type:{gid}.risk"), "kind": "node_type",
               "props": {"key": "risk", "label": "Risk", "default_executor": "agent",
                         "allow_code_exec": true}}),
    )
    .await;
    assert!(upserted == 403 || upserted == 404, "{upserted}");

    // The run engine, on neither list of the private team, still runs the graph with its
    // own types, and they are kept.
    let mut rx = app.events(&gid);
    let (status, run) = call(
        &app,
        Method::POST,
        &format!("/graphs/{gid}/runs"),
        &w.owner,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{run}");
    let events = common::collect_until(&mut rx, "run.finished").await;
    assert_eq!(events.last().unwrap().1["run"]["status"], "succeeded");
    let kept = client
        .vocabulary(&Principal::service(&prefix, w.wid), &gid)
        .await
        .unwrap();
    let risk = kept
        .node_type("risk")
        .expect("the graph's own type is kept");
    assert!(!risk.allow_code_exec);
    assert_eq!(risk.default_role, "analyst");
}
