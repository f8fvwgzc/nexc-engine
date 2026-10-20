//! onto-intelli as the one place nexc's ontology lives (`docs/ONTOLOGY.md`),
//! against the in-memory stub: graph vocabularies read and written there as
//! the acting member, cached, and failing over; the one-shot migration of
//! the stored ontologies; the workspace projected as entities and links; the
//! browser session of the embedded UI; the open proposals; and the
//! assistant filing schema changes as an agent, which people approve there.
#![forbid(unsafe_code)]

mod common;

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use common::{TestApp, collect_until};
use nexc::app::caches::Key;
use nexc::ontology::stub::StubOntology;
use nexc::ontology::vocabulary::GraphRef;
use nexc::ontology::{OntoError, Principal, export, migrate, tenant_of, workspace_name};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

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

/// A workspace with a public and a private team, a member, a guest and an outsider.
struct World {
    wid: Uuid,
    ws: String,
    eng: String,
    secret: String,
    owner: String,
    owner_id: String,
    member: String,
    member_id: String,
    guest: String,
    outsider: String,
}

async fn world(app: &TestApp) -> World {
    let (owner, owner_id) = user(app, "owner@example.com").await;
    let (member, member_id) = user(app, "member@example.com").await;
    let (guest, _) = user(app, "guest@example.com").await;
    let (outsider, _) = user(app, "outsider@example.com").await;
    let (_, list) = call(app, Method::GET, "/workspaces", &owner, None).await;
    let wid: Uuid = list[0]["id"].as_str().unwrap().parse().unwrap();
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
    let mut teams = Vec::new();
    for body in [
        json!({"name": "Engineering", "key": "ENG"}),
        json!({"name": "Leadership", "key": "LEAD", "private": true}),
    ] {
        let (status, team) = call(
            app,
            Method::POST,
            &format!("{ws}/teams"),
            &owner,
            Some(body),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{team}");
        teams.push(team["id"].as_str().unwrap().to_owned());
    }
    World {
        wid,
        ws,
        eng: teams[0].clone(),
        secret: teams[1].clone(),
        owner,
        owner_id,
        member,
        member_id,
        guest,
        outsider,
    }
}

async fn graph(app: &TestApp, w: &World, token: &str, name: &str) -> (StatusCode, Value) {
    call(
        app,
        Method::POST,
        "/graphs",
        token,
        Some(json!({"name": name, "workspace_id": w.wid})),
    )
    .await
}

fn keys(vocabulary: &Value, list: &str) -> Vec<String> {
    vocabulary[list]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["key"].as_str().unwrap().to_owned())
        .collect()
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn graph_vocabularies_live_in_onto_intelli_and_are_written_as_the_member(pool: PgPool) {
    let stub = Arc::new(StubOntology::new());
    let app = TestApp::with_ontology(
        pool,
        &[("NEXC_ONTO_VOCABULARY_TTL_SECS", "0")],
        stub.clone(),
    )
    .await;
    let w = world(&app).await;
    let tenant = tenant_of(w.wid);
    assert_eq!(tenant, format!("ws_{}", w.wid.simple()));

    // A new graph's vocabulary is created in its workspace's tenant, as the backend itself.
    let (status, created) = graph(&app, &w, &w.owner, "Launch").await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let gid = created["id"].as_str().unwrap().to_owned();
    assert_eq!(
        keys(&created["ontology"], "node_types"),
        ["topic", "research", "code", "task", "document", "output"],
        "the starter set, in onto-intelli's order"
    );
    assert!(stub.stored_vocabulary(&tenant, &gid).is_some());
    let init = stub.calls_to("init_graph");
    assert!(
        init.iter().all(|p| !p.acts() && p.tenant == tenant),
        "{init:?}"
    );
    // ...and read as the owner: their own principal with the roles of an owner.
    let reads = stub.calls_to("vocabulary");
    let owner = &reads[0];
    assert_eq!(owner.id, format!("user:{}", w.owner_id));
    assert_eq!(owner.roles, ["member", "approver", "schema_admin"]);
    assert_eq!(
        owner.entity.as_deref(),
        Some(&*format!("person:{}", w.owner_id))
    );
    assert_eq!(owner.tenant, tenant);
    assert_eq!(
        owner.workspace,
        workspace_name(&app.state.settings.onto_workspace_prefix, w.wid)
    );

    // A node of the default kind is a task, whatever order the types come in.
    let url = format!("/graphs/{gid}");
    let (_, node) = call(
        &app,
        Method::POST,
        &format!("{url}/nodes"),
        &w.member,
        Some(json!({"title": "Plan"})),
    )
    .await;
    assert_eq!(node["kind"], "task");

    // The member changes the vocabulary: nexc checks its rules, onto-intelli stores it.
    let mut vocabulary = created["ontology"].clone();
    vocabulary["node_types"]
        .as_array_mut()
        .unwrap()
        .push(json!({"key": "Risk", "label": "", "default_role": "", "stage": 1}));
    let (status, put) = call(
        &app,
        Method::PUT,
        &format!("{url}/ontology"),
        &w.member,
        Some(vocabulary.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{put}");
    assert!(keys(&put["ontology"], "node_types").contains(&"risk".to_owned()));
    let stored = stub.stored_vocabulary(&tenant, &gid).unwrap();
    assert!(stored.node_type("risk").is_some());
    let writer = stub.calls_to("set_vocabulary").pop().unwrap();
    assert_eq!(writer.id, format!("user:{}", w.member_id));
    assert_eq!(writer.roles, ["member"]);

    // nexc's own rules stay nexc's: a type in use cannot go, at most TYPES_MAX types.
    let mut pruned = put["ontology"].clone();
    pruned["node_types"]
        .as_array_mut()
        .unwrap()
        .retain(|t| t["key"] != "task");
    let (status, refused) = call(
        &app,
        Method::PUT,
        &format!("{url}/ontology"),
        &w.member,
        Some(pruned),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    let mut crowded = put["ontology"].clone();
    for n in 0..45 {
        crowded["node_types"]
            .as_array_mut()
            .unwrap()
            .push(json!({"key": format!("kind_{n}"), "label": "K"}));
    }
    let (status, _) = call(
        &app,
        Method::PUT,
        &format!("{url}/ontology"),
        &w.member,
        Some(crowded),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Two nodes tied by a relation that does not order execution...
    let (_, a) = call(
        &app,
        Method::POST,
        &format!("{url}/nodes"),
        &w.member,
        Some(json!({"title": "A"})),
    )
    .await;
    let (_, b) = call(
        &app,
        Method::POST,
        &format!("{url}/nodes"),
        &w.member,
        Some(json!({"title": "B"})),
    )
    .await;
    let (status, edge) = call(
        &app,
        Method::POST,
        &format!("{url}/edges"),
        &w.member,
        Some(json!({"source": a["id"], "target": b["id"], "kind": "relates_to"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{edge}");
    assert_eq!(edge["blocking"], false);

    // ...until someone makes it blocking in onto-intelli itself: nexc follows on the next read.
    let mut edited = stub.stored_vocabulary(&tenant, &gid).unwrap();
    for relation in &mut edited.relation_types {
        if relation.key == "relates_to" {
            relation.blocking = true;
        }
    }
    edited.node_types.retain(|t| t.key != "topic");
    stub.put_vocabulary(&tenant, &gid, edited);
    let (_, read) = call(&app, Method::GET, &url, &w.member, None).await;
    assert!(!keys(&read["ontology"], "node_types").contains(&"topic".to_owned()));
    let followed = read["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == edge["id"])
        .unwrap();
    assert_eq!(followed["blocking"], true, "edges follow their relation");

    // A guest of the workspace is a viewer in onto-intelli: a team graph they are in reads,
    // but onto-intelli refuses their write.
    let (_, team_graph) = call(
        &app,
        Method::POST,
        "/graphs",
        &w.owner,
        Some(json!({"name": "Team plan", "team_id": w.eng})),
    )
    .await;
    let team_gid = team_graph["id"].as_str().unwrap();
    call(
        &app,
        Method::PUT,
        &format!(
            "{}/teams/{}/members/{}",
            w.ws,
            w.eng,
            guest_id(&app, &w).await
        ),
        &w.owner,
        Some(json!({"role": "member"})),
    )
    .await;
    let (status, seen) = call(
        &app,
        Method::GET,
        &format!("/graphs/{team_gid}"),
        &w.guest,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{seen}");
    assert_eq!(stub.calls_to("vocabulary").pop().unwrap().roles, ["viewer"]);
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
async fn a_private_teams_graph_keeps_its_own_types_whoever_reads_it(pool: PgPool) {
    let stub = Arc::new(StubOntology::new());
    let app = TestApp::with_ontology(
        pool,
        &[("NEXC_ONTO_VOCABULARY_TTL_SECS", "0")],
        stub.clone(),
    )
    .await;
    let w = world(&app).await;
    let tenant = tenant_of(w.wid);
    let onto_ws = workspace_name(&app.state.settings.onto_workspace_prefix, w.wid);

    // A graph of the private team, with a type of its own and a node of that type.
    let (status, made) = call(
        &app,
        Method::POST,
        "/graphs",
        &w.owner,
        Some(json!({"name": "Reorg plan", "team_id": w.secret})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    let gid = made["id"].as_str().unwrap().to_owned();
    let mut vocabulary = made["ontology"].clone();
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
    let node = app
        .node(&w.owner, &gid, json!({"title": "Assess", "kind": "risk"}))
        .await;

    // The projection restricts the graph to the team and to team:admins...
    export::pass(&app.state).await.unwrap();
    let graph_entity = format!("execution_graph:{gid}");
    assert_eq!(
        stub.entity(&onto_ws, &graph_entity).unwrap().props["visibility"],
        "restricted"
    );
    let writes = || stub.calls_to("init_graph").len() + stub.calls_to("set_vocabulary").len();
    let written = writes();

    // ...so the run engine (agent:runtime, on neither list) is told the graph is not there.
    // It is read as the backend instead: the run goes through with the graph's own types,
    // and nothing is written over them.
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
    let events = collect_until(&mut rx, "run.finished").await;
    assert_eq!(events.last().unwrap().1["run"]["status"], "succeeded");
    let reads = stub.calls();
    let runtime = reads
        .iter()
        .position(|(m, p)| m == "vocabulary" && p.id == "agent:runtime")
        .expect("the run engine reads as itself first");
    assert!(
        reads[runtime + 1..]
            .iter()
            .any(|(m, p)| m == "vocabulary" && !p.acts()),
        "{reads:?}"
    );
    // Dependency detection after an edit reads as the run engine too.
    let (status, _) = call(
        &app,
        Method::PATCH,
        &format!("/graphs/{gid}/nodes/{node}"),
        &w.owner,
        Some(json!({"title": "Assess the risks"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, read) = call(&app, Method::GET, &format!("/graphs/{gid}"), &w.owner, None).await;
    assert!(keys(&read["ontology"], "node_types").contains(&"risk".to_owned()));
    assert_eq!(
        writes(),
        written,
        "a vocabulary onto-intelli holds is never initialised again"
    );
    let stored = stub.stored_vocabulary(&tenant, &gid).unwrap();
    assert_eq!(stored.node_type("risk").unwrap().default_role, "analyst");

    // A member the projection has not named in the team yet is in the same place: nexc lets
    // them work on the team's graph, onto-intelli does not show it to them yet.
    call(
        &app,
        Method::PUT,
        &format!("{}/teams/{}/members/{}", w.ws, w.secret, w.member_id),
        &w.owner,
        Some(json!({"role": "member"})),
    )
    .await;
    let (status, seen) = call(
        &app,
        Method::GET,
        &format!("/graphs/{gid}"),
        &w.member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{seen}");
    assert!(keys(&seen["ontology"], "node_types").contains(&"risk".to_owned()));
    assert_eq!(writes(), written);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_plan_adds_its_types_to_what_onto_intelli_holds_when_it_is_applied(pool: PgPool) {
    let stub = Arc::new(StubOntology::new());
    // The default cache lifetime: what the planner read may be up to 30 s old.
    let app = TestApp::with_ontology(pool.clone(), &[], stub.clone()).await;
    let w = world(&app).await;
    let tenant = tenant_of(w.wid);
    let (_, made) = graph(&app, &w, &w.owner, "Report").await;
    let gid = made["id"].as_str().unwrap().to_owned();
    app.node(&w.owner, &gid, json!({"title": "Idea"})).await;

    // A plan that adds a type of its own.
    let mut rx = app.events(&gid);
    let (status, plan) = call(
        &app,
        Method::POST,
        &format!("/graphs/{gid}/plan"),
        &w.owner,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{plan}");
    collect_until(&mut rx, "plan.ready").await;
    let pid = plan["id"].as_str().unwrap();
    sqlx::query("UPDATE plans SET ontology = $2 WHERE id = $1")
        .bind(pid.parse::<Uuid>().unwrap())
        .bind(json!({"node_types": [{"key": "review", "label": "Review", "stage": 3}]}))
        .execute(&pool)
        .await
        .unwrap();

    // Meanwhile, within the cache's lifetime, someone adds a type in onto-intelli and renames
    // another; and a member removes one through the editor.
    let mut edited = stub.stored_vocabulary(&tenant, &gid).unwrap();
    edited.node_types.retain(|t| t.key != "research");
    for t in &mut edited.node_types {
        if t.key == "topic" {
            t.label = "Theme".into();
        }
    }
    let mut legal = edited.node_types[0].clone();
    legal.key = "legal".into();
    legal.label = "Legal".into();
    edited.node_types.push(legal);
    stub.put_vocabulary(&tenant, &gid, edited);

    let (status, applied) = call(
        &app,
        Method::POST,
        &format!("/graphs/{gid}/plans/{pid}/apply"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    // Only the plan's type was written, onto what onto-intelli held then.
    let stored = stub.stored_vocabulary(&tenant, &gid).unwrap();
    assert!(stored.node_type("review").is_some(), "{stored:?}");
    assert!(stored.node_type("legal").is_some(), "added there meanwhile");
    assert!(stored.node_type("research").is_none(), "removed meanwhile");
    assert_eq!(stored.node_type("topic").unwrap().label, "Theme");
    let added = stub.calls_to("init_graph").pop().unwrap();
    assert_eq!(
        added.id,
        format!("user:{}", w.owner_id),
        "written as the member"
    );
    // The canvas gets the vocabulary onto-intelli holds now.
    assert_eq!(
        keys(&applied["ontology"], "node_types"),
        keys(&serde_json::to_value(&stored).unwrap(), "node_types")
    );
}

async fn guest_id(app: &TestApp, w: &World) -> String {
    let (_, members) = call(
        app,
        Method::GET,
        &format!("{}/members", w.ws),
        &w.owner,
        None,
    )
    .await;
    members
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "guest")
        .unwrap()["user_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn without_onto_intelli_graphs_keep_the_read_only_starter_set(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let (status, created) = graph(&app, &w, &w.owner, "Plain").await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        created["ontology"]["node_types"].as_array().unwrap().len(),
        6
    );
    let gid = created["id"].as_str().unwrap();
    let (status, refused) = call(
        &app,
        Method::PUT,
        &format!("/graphs/{gid}/ontology"),
        &w.owner,
        Some(created["ontology"].clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert!(refused["detail"].as_str().unwrap().contains("read-only"));

    // The console says so, and has no session to offer.
    let (status, ontology) = call(
        &app,
        Method::GET,
        &format!("{}/ontology", w.ws),
        &w.member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ontology["mode"], "off");
    assert_eq!(ontology["ui_url"], Value::Null);
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("{}/ontology/session", w.ws),
        &w.member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_unreachable_onto_intelli_is_served_from_the_cache_or_fails_retryably(pool: PgPool) {
    let stub = Arc::new(StubOntology::new());
    let app = TestApp::with_ontology(
        pool,
        &[
            ("NEXC_ONTO_VOCABULARY_TTL_SECS", "0"),
            ("NEXC_MAX_ATTEMPTS", "1"),
        ],
        stub.clone(),
    )
    .await;
    let w = world(&app).await;
    let (_, kept) = graph(&app, &w, &w.owner, "Kept").await;
    let kept = kept["id"].as_str().unwrap().to_owned();
    let node = app.node(&w.owner, &kept, json!({"title": "Write"})).await;

    stub.fail_with(Some(OntoError::Unavailable("connection refused".into())));
    // What was read before is still served, however old.
    let (status, read) = call(
        &app,
        Method::GET,
        &format!("/graphs/{kept}"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(read["ontology"]["node_types"].as_array().unwrap().len(), 6);
    // A graph never read is not: 503, and it can still be created (its vocabulary comes later).
    let (status, fresh) = graph(&app, &w, &w.owner, "Fresh").await;
    assert_eq!(status, StatusCode::CREATED, "{fresh}");
    let fresh = fresh["id"].as_str().unwrap();
    let (status, problem) = call(
        &app,
        Method::GET,
        &format!("/graphs/{fresh}"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{problem}");
    assert!(
        problem["detail"]
            .as_str()
            .unwrap()
            .contains("ontology unavailable")
    );

    // A run whose vocabulary is not at hand fails its node, retryably, with what happened.
    app.state
        .caches
        .drop_local(&Key::Vocabulary(kept.parse().unwrap()));
    let mut rx = app.events(&kept);
    let (status, run) = call(
        &app,
        Method::POST,
        &format!("/graphs/{kept}/runs"),
        &w.owner,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{run}");
    let events = collect_until(&mut rx, "run.finished").await;
    let finished = &events.last().unwrap().1["run"];
    assert_eq!(finished["status"], "failed");
    let failed = finished["node_runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["node_id"] == node)
        .unwrap();
    assert!(
        failed["error"]
            .as_str()
            .unwrap()
            .contains("ontology unavailable"),
        "{failed}"
    );

    // Back again: the run goes through.
    stub.fail_with(None);
    let mut rx = app.events(&kept);
    call(
        &app,
        Method::POST,
        &format!("/graphs/{kept}/runs"),
        &w.owner,
        Some(json!({"force": true})),
    )
    .await;
    let events = collect_until(&mut rx, "run.finished").await;
    assert_eq!(events.last().unwrap().1["run"]["status"], "succeeded");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn attempts_spent_waiting_for_the_vocabulary_count_against_the_node(pool: PgPool) {
    let stub = Arc::new(StubOntology::new());
    let app = TestApp::with_ontology(
        pool,
        &[
            ("NEXC_ONTO_VOCABULARY_TTL_SECS", "0"),
            ("NEXC_MAX_ATTEMPTS", "2"),
        ],
        stub.clone(),
    )
    .await;
    let w = world(&app).await;
    let (_, made) = graph(&app, &w, &w.owner, "Flaky").await;
    let gid = made["id"].as_str().unwrap().to_owned();
    // A node whose first model call fails retryably: it needs both of its two attempts.
    let node = app
        .node(&w.owner, &gid, json!({"title": "FLAKY step"}))
        .await;
    // Let the dependency detection the edit scheduled read the vocabulary first.
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    app.state
        .caches
        .drop_local(&Key::Vocabulary(gid.parse().unwrap()));

    // onto-intelli does not answer the run's first read nor the node's first attempt.
    stub.fail_next(
        "vocabulary",
        2,
        OntoError::Unavailable("connection refused".into()),
    );
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
    let events = collect_until(&mut rx, "run.finished").await;
    let finished = &events.last().unwrap().1["run"];
    // The vocabulary came on the second attempt, which is the node's last: its retryable
    // failure is final, and the node was not given a fresh budget.
    assert_eq!(finished["status"], "failed", "{finished}");
    let failed = finished["node_runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["node_id"] == node)
        .unwrap();
    assert_eq!(failed["attempt"], 2, "{failed}");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn stored_ontologies_are_copied_once_idempotently_and_resumably(pool: PgPool) {
    let stub = Arc::new(StubOntology::new());
    let app = TestApp::with_ontology(
        pool.clone(),
        &[("NEXC_ONTO_VOCABULARY_TTL_SECS", "0")],
        stub.clone(),
    )
    .await;
    let w = world(&app).await;
    let tenant = tenant_of(w.wid);
    // Three graphs as the release before left them: two with an ontology of their own.
    let mut legacy = Vec::new();
    for name in ["Trading", "Hiring", "Plain"] {
        let id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO graphs (id, owner_id, name, workspace_id) VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(w.owner_id.parse::<Uuid>().unwrap())
        .bind(name)
        .bind(w.wid)
        .execute(&pool)
        .await
        .unwrap();
        legacy.push(id);
    }
    let custom = json!({
        "node_types": [{"key": "signal", "label": "Signal", "default_role": "analyst", "stage": 1}],
        "relation_types": [{"key": "confirms", "label": "Confirms", "blocking": true}]
    });
    for id in &legacy[..2] {
        sqlx::query("UPDATE graphs SET ontology = $2 WHERE id = $1")
            .bind(id)
            .bind(&custom)
            .execute(&pool)
            .await
            .unwrap();
    }

    // One is read before the migration reaches it: it is copied on that read.
    let (_, read) = call(
        &app,
        Method::GET,
        &format!("/graphs/{}", legacy[0]),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(keys(&read["ontology"], "node_types"), ["signal"]);

    // onto-intelli is down: the pass records the failures and reports them.
    stub.fail_with(Some(OntoError::Unavailable("down".into())));
    let report = migrate::run(&app.state).await.unwrap();
    assert_eq!((report.migrated, report.failed), (0, 2), "{report:?}");
    let noted: Option<String> =
        sqlx::query_scalar("SELECT last_error FROM onto_vocabulary_migrations WHERE graph_id = $1")
            .bind(legacy[1])
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(noted.unwrap().contains("not reachable"));

    // It is back: the pass resumes and copies what remains.
    stub.fail_with(None);
    let report = migrate::run(&app.state).await.unwrap();
    assert_eq!(
        (report.migrated, report.failed, report.remaining),
        (2, 0, 0)
    );
    let copied = stub
        .stored_vocabulary(&tenant, &legacy[1].to_string())
        .unwrap();
    assert!(copied.relation_type("confirms").unwrap().blocking);
    assert_eq!(
        stub.stored_vocabulary(&tenant, &legacy[2].to_string())
            .unwrap()
            .node_types
            .len(),
        6,
        "a graph that stored nothing gets the starter set"
    );

    // Edited in onto-intelli since, a migrated graph is never written again.
    let mut edited = copied.clone();
    edited.node_types[0].label = "Market signal".into();
    stub.put_vocabulary(&tenant, &legacy[1].to_string(), edited);
    let report = migrate::run(&app.state).await.unwrap();
    assert_eq!(
        (report.migrated, report.failed, report.remaining),
        (0, 0, 0)
    );
    assert_eq!(
        stub.stored_vocabulary(&tenant, &legacy[1].to_string())
            .unwrap()
            .node_types[0]
            .label,
        "Market signal"
    );
    // The writes were the backend's own, never a member's.
    assert!(stub.calls_to("set_vocabulary").iter().all(|p| !p.acts()));
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_graph_the_projection_stated_first_still_gets_its_stored_types(pool: PgPool) {
    let stub = Arc::new(StubOntology::new());
    let app = TestApp::with_ontology(
        pool.clone(),
        &[("NEXC_ONTO_VOCABULARY_TTL_SECS", "0")],
        stub.clone(),
    )
    .await;
    let w = world(&app).await;
    let tenant = tenant_of(w.wid);
    let onto_ws = workspace_name(&app.state.settings.onto_workspace_prefix, w.wid);
    let custom = json!({
        "node_types": [{"key": "signal", "label": "Signal", "default_role": "analyst", "stage": 1}],
        "relation_types": [{"key": "confirms", "label": "Confirms", "blocking": true}]
    });
    let mut legacy = Vec::new();
    for name in ["Trading", "Hiring"] {
        let id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO graphs (id, owner_id, name, workspace_id, ontology)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(id)
        .bind(w.owner_id.parse::<Uuid>().unwrap())
        .bind(name)
        .bind(w.wid)
        .bind(&custom)
        .execute(&pool)
        .await
        .unwrap();
        legacy.push(id);
    }
    // The projection states both graphs before anything copied their vocabularies: onto-intelli
    // then has their entities, which read as vocabularies without a type.
    export::pass(&app.state).await.unwrap();
    for id in &legacy {
        assert!(
            stub.entity(&onto_ws, &format!("execution_graph:{id}"))
                .is_some()
        );
        assert!(stub.stored_vocabulary(&tenant, &id.to_string()).is_none());
    }
    // Read first: the stored types are copied, not an empty vocabulary served.
    let (status, read) = call(
        &app,
        Method::GET,
        &format!("/graphs/{}", legacy[0]),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(keys(&read["ontology"], "node_types"), ["signal"]);
    // Reached by the pass first: copied the same way.
    let report = migrate::run(&app.state).await.unwrap();
    assert_eq!(
        (report.migrated, report.failed, report.remaining),
        (1, 0, 0),
        "{report:?}"
    );
    let copied = stub
        .stored_vocabulary(&tenant, &legacy[1].to_string())
        .unwrap();
    assert!(copied.relation_type("confirms").unwrap().blocking);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_migration_never_writes_over_a_vocabulary_onto_intelli_holds(pool: PgPool) {
    let stub = Arc::new(StubOntology::new());
    let app = TestApp::with_ontology(
        pool.clone(),
        &[("NEXC_ONTO_VOCABULARY_TTL_SECS", "0")],
        stub.clone(),
    )
    .await;
    let w = world(&app).await;
    let tenant = tenant_of(w.wid);
    let backend = app.state.ontology.clone().unwrap();
    let custom = json!({
        "node_types": [{"key": "signal", "label": "Signal", "default_role": "analyst", "stage": 1}],
        "relation_types": [{"key": "confirms", "label": "Confirms", "blocking": true}]
    });
    // Two graphs as the release before left them, with an ontology of their own.
    let mut legacy = Vec::new();
    for name in ["Trading", "Hiring"] {
        let id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO graphs (id, owner_id, name, workspace_id, ontology)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(id)
        .bind(w.owner_id.parse::<Uuid>().unwrap())
        .bind(name)
        .bind(w.wid)
        .bind(&custom)
        .execute(&pool)
        .await
        .unwrap();
        legacy.push(id);
    }

    // The pass has read its batch when a member opens the first graph (it is copied on that
    // read) and changes it: a type of their own, a label of their own.
    let url = format!("/graphs/{}", legacy[0]);
    let (_, read) = call(&app, Method::GET, &url, &w.owner, None).await;
    let mut vocabulary = read["ontology"].clone();
    vocabulary["node_types"][0]["label"] = json!("Market signal");
    vocabulary["node_types"]
        .as_array_mut()
        .unwrap()
        .push(json!({"key": "hedge", "label": "Hedge", "stage": 2}));
    let (status, put) = call(
        &app,
        Method::PUT,
        &format!("{url}/ontology"),
        &w.owner,
        Some(vocabulary),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{put}");
    // The pass reaches it with its stale batch: under the graph's lock it sees the graph
    // migrated, and writes nothing.
    let graph = GraphRef {
        id: legacy[0],
        workspace_id: Some(w.wid),
        name: "Trading",
    };
    assert!(
        !migrate::copy(&app.state, backend.as_ref(), graph)
            .await
            .unwrap()
    );
    let kept = stub
        .stored_vocabulary(&tenant, &legacy[0].to_string())
        .unwrap();
    assert!(kept.node_type("hedge").is_some(), "{kept:?}");
    assert_eq!(kept.node_type("signal").unwrap().label, "Market signal");

    // The second is in onto-intelli already, edited there, but was never recorded (a copy that
    // stopped before recording it): the pass only records it.
    let mut edited: nexc::domain::ontology::Ontology = serde_json::from_value(custom).unwrap();
    edited.normalize();
    edited.node_types[0].label = "Candidate".into();
    stub.put_vocabulary(&tenant, &legacy[1].to_string(), edited.clone());
    let writes = stub.calls_to("init_graph").len() + stub.calls_to("set_vocabulary").len();
    let report = migrate::run(&app.state).await.unwrap();
    assert_eq!(
        (report.migrated, report.already, report.remaining),
        (0, 1, 0),
        "{report:?}"
    );
    assert_eq!(
        stub.calls_to("init_graph").len() + stub.calls_to("set_vocabulary").len(),
        writes
    );
    assert_eq!(
        stub.stored_vocabulary(&tenant, &legacy[1].to_string())
            .unwrap()
            .node_types[0]
            .label,
        "Candidate"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_workspace_is_projected_as_entities_and_links_without_content(pool: PgPool) {
    let stub = Arc::new(StubOntology::new());
    let app = TestApp::with_ontology(pool, &[], stub.clone()).await;
    let w = world(&app).await;
    let onto_ws = workspace_name(&app.state.settings.onto_workspace_prefix, w.wid);
    let (_, issue) = call(
        &app,
        Method::POST,
        &format!("{}/teams/{}/issues", w.ws, w.eng),
        &w.member,
        Some(
            json!({"title": "Fix login", "description": "SECRET-DESCRIPTION",
                    "assignee_id": w.member_id}),
        ),
    )
    .await;
    let issue_id = issue["id"].as_str().unwrap().to_owned();
    let (_, hidden) = call(
        &app,
        Method::POST,
        &format!("{}/teams/{}/issues", w.ws, w.secret),
        &w.owner,
        Some(json!({"title": "Reorg"})),
    )
    .await;

    // One pass: the backfill states the whole workspace, as the backend's service principal.
    export::pass(&app.state).await.unwrap();
    let person = format!("person:{}", w.member_id);
    let entity = |id: &str| stub.entity(&onto_ws, id);
    let member = entity(&person).expect("people are entities");
    assert_eq!(
        (member.title.as_str(), &member.props["role"]),
        ("Test", &json!("member"))
    );
    let fix = entity(&format!("issue:{issue_id}")).expect("the issue is an entity");
    assert_eq!(fix.title, "ENG-1 Fix login");
    assert_eq!(fix.state, "triaged", "an unstarted issue");
    assert!(stub.has_link(
        &onto_ws,
        &format!("issue:{issue_id}"),
        "assigned_to",
        &person
    ));
    assert!(stub.has_link(
        &onto_ws,
        &format!("issue:{issue_id}"),
        "belongs_to",
        &format!("team:{}", w.eng)
    ));
    let secret_team = entity(&format!("team:{}", w.secret)).unwrap();
    assert_eq!(secret_team.props["visibility"], "restricted");
    assert_eq!(
        secret_team.props["acl"],
        json!([format!("team:{}", w.secret), "team:admins"]),
        "the team, and the workspace's owners and admins"
    );
    let owner_person = format!("person:{}", w.owner_id);
    assert!(stub.has_link(&onto_ws, &owner_person, "member_of", "team:admins"));
    assert!(!stub.has_link(&onto_ws, &person, "member_of", "team:admins"));
    let reorg = entity(&format!("issue:{}", hidden["id"].as_str().unwrap())).unwrap();
    assert_eq!(reorg.props["visibility"], "restricted");
    let text = serde_json::to_string(&stub.records()).unwrap();
    assert!(!text.contains("SECRET-DESCRIPTION"), "no content leaves");
    assert!(!text.contains("@example.com"), "no e-mail address leaves");
    let ingests = stub.calls_to("ingest");
    assert!(
        ingests
            .iter()
            .all(|p| !p.acts() && p.tenant.starts_with("ws_")),
        "the projection writes as the backend, each workspace into its own tenant"
    );
    assert!(
        ingests
            .iter()
            .any(|p| p.tenant == tenant_of(w.wid) && p.workspace == onto_ws)
    );

    // Changes follow through the queue: a new assignee replaces the old link...
    call(
        &app,
        Method::PATCH,
        &format!("/issues/{issue_id}"),
        &w.member,
        Some(json!({"assignee_id": w.owner_id})),
    )
    .await;
    export::pass(&app.state).await.unwrap();
    assert!(stub.has_link(
        &onto_ws,
        &format!("issue:{issue_id}"),
        "assigned_to",
        &owner_person
    ));
    assert!(!stub.has_link(
        &onto_ws,
        &format!("issue:{issue_id}"),
        "assigned_to",
        &person
    ));
    // ...and a deleted issue is deleted there too.
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/issues/{issue_id}"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    export::pass(&app.state).await.unwrap();
    assert!(entity(&format!("issue:{issue_id}")).is_none());
    // A graph is the same entity as its vocabulary, tied to whoever made it.
    let (_, made) = graph(&app, &w, &w.owner, "Roadmap").await;
    export::pass(&app.state).await.unwrap();
    let graph_entity = format!("execution_graph:{}", made["id"].as_str().unwrap());
    assert_eq!(entity(&graph_entity).unwrap().title, "Roadmap");
    assert!(stub.has_link(&onto_ws, &graph_entity, "created_by", &owner_person));
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_projection_follows_privacy_and_what_goes_with_its_parent(pool: PgPool) {
    let stub = Arc::new(StubOntology::new());
    let app = TestApp::with_ontology(pool.clone(), &[], stub.clone()).await;
    let w = world(&app).await;
    let tenant = tenant_of(w.wid);
    let onto_ws = workspace_name(&app.state.settings.onto_workspace_prefix, w.wid);
    let entity = |id: &str| stub.entity(&onto_ws, id);
    let restricted = |id: &str| {
        let props = entity(id).unwrap_or_else(|| panic!("{id} is there")).props;
        (props["visibility"] == "restricted").then(|| props["acl"].clone())
    };

    // The public team's issue, cycle, graph and a run of it.
    let (_, issue) = call(
        &app,
        Method::POST,
        &format!("{}/teams/{}/issues", w.ws, w.eng),
        &w.owner,
        Some(json!({"title": "Ship it"})),
    )
    .await;
    let issue = format!("issue:{}", issue["id"].as_str().unwrap());
    let (status, cycle) = call(
        &app,
        Method::POST,
        &format!("{}/teams/{}/cycles", w.ws, w.eng),
        &w.owner,
        Some(json!({"starts_on": "2026-10-19", "ends_on": "2026-10-30"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{cycle}");
    let cycle = format!("cycle:{}", cycle["id"].as_str().unwrap());
    let (_, made) = call(
        &app,
        Method::POST,
        "/graphs",
        &w.owner,
        Some(json!({"name": "Launch", "team_id": w.eng})),
    )
    .await;
    let gid = made["id"].as_str().unwrap().to_owned();
    app.node(&w.owner, &gid, json!({"title": "Plan"})).await;
    let mut rx = app.events(&gid);
    let (_, run) = call(
        &app,
        Method::POST,
        &format!("/graphs/{gid}/runs"),
        &w.owner,
        Some(json!({})),
    )
    .await;
    collect_until(&mut rx, "run.finished").await;
    let run = format!("run:{}", run["id"].as_str().unwrap());
    let graph = format!("execution_graph:{gid}");
    export::pass(&app.state).await.unwrap();
    let things = [&issue, &cycle, &graph, &run];
    for id in things {
        assert_eq!(restricted(id), None, "{id} of a public team");
    }
    // The workspace's admins are a team only they see.
    assert_eq!(restricted("team:admins"), Some(json!(["team:admins"])));

    // The team turns private: everything of it is restricted to it, its runs included.
    let team_url = format!("{}/teams/{}", w.ws, w.eng);
    let (status, _) = call(
        &app,
        Method::PATCH,
        &team_url,
        &w.owner,
        Some(json!({"private": true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    export::pass(&app.state).await.unwrap();
    let acl = json!([format!("team:{}", w.eng), "team:admins"]);
    for id in things {
        assert_eq!(restricted(id), Some(acl.clone()), "{id} of a private team");
    }
    // ...and public again: open to the workspace once more.
    call(
        &app,
        Method::PATCH,
        &team_url,
        &w.owner,
        Some(json!({"private": false})),
    )
    .await;
    export::pass(&app.state).await.unwrap();
    for id in things {
        assert_eq!(restricted(id), None, "{id} public again");
    }

    // A deleted graph takes its runs and its vocabulary along.
    assert!(!stub.stored_vocabulary(&tenant, &gid).unwrap().is_empty());
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/graphs/{gid}"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    export::pass(&app.state).await.unwrap();
    assert!(entity(&graph).is_none() && entity(&run).is_none());
    assert!(stub.stored_vocabulary(&tenant, &gid).unwrap().is_empty());

    // The deleted graph keeps its runs' evidence, and its team, until the retention purge.
    let (status, _) = call(&app, Method::DELETE, &team_url, &w.owner, None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    for sql in [
        "UPDATE graphs SET deleted_at = now() - interval '31 days' WHERE id = $1::uuid",
        "UPDATE runs SET finished_at = now() - interval '31 days' WHERE graph_id = $1::uuid",
    ] {
        sqlx::query(sql)
            .bind(&gid)
            .execute(&app.state.db)
            .await
            .unwrap();
    }
    nexc::engine::evidence::purge_expired(&app.state)
        .await
        .unwrap();

    // A deleted team takes its cycles along, and its members are no longer in it.
    let (status, _) = call(&app, Method::DELETE, &team_url, &w.owner, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    export::pass(&app.state).await.unwrap();
    assert!(entity(&cycle).is_none());
    assert!(entity(&format!("team:{}", w.eng)).is_none());
    assert!(
        !stub
            .links(&onto_ws)
            .iter()
            .any(|(_, _, to)| to == &format!("team:{}", w.eng))
    );

    // A deleted workspace's tenant is retired: what the projection put there goes, and the
    // export does not stumble over what was queued for it.
    let person = format!("person:{}", w.member_id);
    assert!(entity(&person).is_some());
    call(
        &app,
        Method::POST,
        "/workspaces",
        &w.owner,
        Some(json!({"name": "Elsewhere"})),
    )
    .await;
    let (status, problem) = call(&app, Method::DELETE, &w.ws, &w.owner, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{problem}");
    let progress = export::pass(&app.state).await.unwrap();
    assert_eq!(progress.failed, 0, "{progress:?}");
    assert!(entity(&person).is_none() && entity(&issue).is_none());
    assert!(entity("team:admins").is_none());
    let (cleared, queued): (bool, i64) = sqlx::query_as(
        "SELECT (SELECT cleared_at IS NOT NULL FROM onto_retired_tenants WHERE workspace_id = $1),
                (SELECT count(*) FROM onto_export_queue WHERE workspace_id = $1)",
    )
    .bind(w.wid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(cleared);
    assert_eq!(queued, 0);
    assert_eq!(export::pass(&app.state).await.unwrap().failed, 0);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn members_get_their_own_short_lived_session_for_the_embedded_ui(pool: PgPool) {
    let stub = Arc::new(StubOntology::new());
    let app = TestApp::with_ontology(
        pool,
        &[("NEXC_ONTO_PUBLIC_URL", "https://onto.example.com/")],
        stub.clone(),
    )
    .await;
    let w = world(&app).await;
    let session = format!("{}/ontology/session", w.ws);
    let (status, minted) = call(&app, Method::POST, &session, &w.member, None).await;
    assert_eq!(status, StatusCode::OK, "{minted}");
    assert_eq!(minted["ui_url"], "https://onto.example.com/ui/");
    assert_eq!(minted["tenant"], tenant_of(w.wid));
    assert_eq!(
        minted["workspace"],
        workspace_name(&app.state.settings.onto_workspace_prefix, w.wid)
    );
    assert!(minted["token"].as_str().unwrap().starts_with("onto1."));
    assert!(minted["expires_at"].as_str().is_some());
    let principal: Principal = stub.calls_to("mint_session").pop().unwrap();
    assert_eq!(principal.id, format!("user:{}", w.member_id));
    assert_eq!(
        principal.roles,
        ["viewer"],
        "the browser reads; the backend writes"
    );
    // Owners approve and apply schema changes there, but write no data either.
    let (status, _) = call(&app, Method::POST, &session, &w.owner, None).await;
    assert_eq!(status, StatusCode::OK);
    let principal: Principal = stub.calls_to("mint_session").pop().unwrap();
    assert_eq!(principal.roles, ["viewer", "approver", "schema_admin"]);
    // Guests and outsiders get none.
    let (status, _) = call(&app, Method::POST, &session, &w.guest, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&app, Method::POST, &session, &w.outsider, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // onto-intelli down: 503, nothing leaks.
    stub.fail_with(Some(OntoError::Unavailable("down".into())));
    let (status, problem) = call(&app, Method::POST, &session, &w.owner, None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{problem}");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_assistant_proposes_schema_changes_as_an_agent_and_people_review_them(pool: PgPool) {
    let stub = Arc::new(StubOntology::new());
    let app = TestApp::with_ontology(
        pool,
        &[("NEXC_ONTO_PUBLIC_URL", "http://onto.local:50051")],
        stub.clone(),
    )
    .await;
    let w = world(&app).await;
    let ask = format!("{}/assistant", w.ws);
    let (status, reply) = call(
        &app,
        Method::POST,
        &ask,
        &w.member,
        Some(json!({"message": "propose schema: track reviews"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let filed = &reply["proposals"][0];
    assert_eq!(filed["title"], "track reviews");
    let id = filed["id"].as_str().unwrap();
    assert_eq!(
        filed["review_url"],
        format!("http://onto.local:50051/ui/#/proposals/{id}")
    );
    // Read, dry-run and filed as the member's assistant: an agent, never an approver.
    for method in ["schema", "plan_schema", "propose_schema"] {
        let made_as = stub.calls_to(method);
        assert_eq!(made_as.len(), 1, "{method}");
        assert_eq!(made_as[0].id, format!("assistant:{}", w.member_id));
        assert_eq!(made_as[0].roles, ["agent"]);
    }
    let stored = stub.proposals_of(&tenant_of(w.wid));
    assert_eq!(stored[0]["source"], "nexc-assistant");
    assert_eq!(stored[0]["status"], "open");

    // Settings → Ontology lists it, with the link to review it in onto-intelli.
    let (status, ontology) = call(
        &app,
        Method::GET,
        &format!("{}/ontology", w.ws),
        &w.member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{ontology}");
    assert_eq!(ontology["mode"], "service");
    assert_eq!(ontology["proposals"][0]["id"], id);
    assert_eq!(ontology["proposals"][0]["by_agent"], true);
    assert_eq!(ontology["proposals"][0]["status"], "open");
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("{}/ontology", w.ws),
        &w.guest,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // A guest's assistant reads no schema and proposes nothing.
    let (_, guest_reply) = call(
        &app,
        Method::POST,
        &ask,
        &w.guest,
        Some(json!({"message": "propose schema: track budgets"})),
    )
    .await;
    assert_eq!(guest_reply["proposals"], json!([]));
    assert_eq!(stub.proposals_of(&tenant_of(w.wid)).len(), 1);
}

/// The real client against the stub behind the real wire: every new call
/// carries the tenant and, when made for someone, who; the session goes over
/// the same port's HTTP API; the server's refusals come back as errors.
#[tokio::test]
async fn the_client_carries_the_member_and_the_tenant_on_every_call() {
    use nexc::domain::ontology::Ontology;
    use nexc::domain::workspace::WorkspaceRole;
    use nexc::ontology::grpc::GrpcOntology;
    use nexc::ontology::stub::server;
    use nexc::ontology::{OntologyBackend, SchemaChange, SchemaProposalDraft};

    let stub = Arc::new(StubOntology::new());
    let token = "onto-token-0123456789";
    let running = server::serve(stub.clone(), Some(token)).await.unwrap();
    let client = GrpcOntology::new(&running.url(), Some(token)).unwrap();
    let (w, u) = (Uuid::now_v7(), Uuid::now_v7());
    let admin = Principal::user("nexc-", w, u, WorkspaceRole::Admin);
    let service = Principal::service("nexc-", w);

    assert!(matches!(
        client.vocabulary(&admin, "g1").await,
        Err(OntoError::NotFound(_))
    ));
    client
        .init_graph(&service, "g1", "G", Some(&Ontology::starter()))
        .await
        .unwrap();
    let read = client.vocabulary(&admin, "g1").await.unwrap();
    assert_eq!(read.node_types.len(), 6);
    assert_eq!(read.node_types[0].stage, 0, "numbers come back as numbers");
    let mut less = read.clone();
    less.node_types.retain(|t| t.key != "output");
    let stored = client.set_vocabulary(&admin, "g1", &less).await.unwrap();
    assert_eq!(stored.node_types.len(), 5);
    assert_eq!(stub.calls_to("set_vocabulary").pop().unwrap(), admin);
    let init = stub.calls_to("init_graph").pop().unwrap();
    assert!(!init.acts() && init.tenant == admin.tenant);
    let mut bad = less.clone();
    bad.node_types[0].color = "red".into();
    assert!(matches!(
        client.set_vocabulary(&admin, "g1", &bad).await,
        Err(OntoError::Invalid(_))
    ));

    let assistant = Principal::assistant("nexc-", w, u);
    let schema = client.schema(&assistant).await.unwrap();
    assert!(schema["object_types"].is_object());
    let change = json!({"op": "put_link_type", "name": "reviews", "def": {}});
    let plan = client
        .plan_schema(
            &assistant,
            &SchemaChange {
                changes: vec![change.clone()],
                ..SchemaChange::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(plan["valid"], true);
    let filed = client
        .propose_schema(
            &assistant,
            &SchemaProposalDraft {
                title: "Reviews".into(),
                rationale: "asked".into(),
                changes: vec![change],
                migrations: vec![],
                source: "nexc-assistant".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(filed["proposer"]["id"], assistant.id);
    let open = client.proposals(&admin, Some("open"), 10).await.unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(stub.calls_to("propose_schema").pop().unwrap(), assistant);

    let session = client.mint_session(&admin, 600).await.unwrap();
    assert!(session.token.starts_with("onto1."));
    assert_eq!(session.tenant, admin.tenant);
    let minted_for = stub.calls_to("mint_session").pop().unwrap();
    assert_eq!(
        (minted_for.id.as_str(), minted_for.roles.clone()),
        (admin.id.as_str(), admin.roles.clone())
    );

    // A token the server does not know is refused, on both wires.
    let stranger = GrpcOntology::new(&running.url(), Some("not-the-token-012345")).unwrap();
    assert!(matches!(
        stranger.vocabulary(&admin, "g1").await,
        Err(OntoError::Denied(_))
    ));
    assert!(matches!(
        stranger.mint_session(&admin, 600).await,
        Err(OntoError::Denied(_))
    ));
    running.stop().await;
}
