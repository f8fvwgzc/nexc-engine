//! "Ask the audit" (AgentSecOps phase 3): the trace becomes facts in
//! onto-intelli and questions are answered from them. The onto-intelli here
//! is the in-memory stub, directly as the backend or behind its in-process
//! gRPC server, so the export, the questions and the wire are all exercised
//! without a real server.
#![forbid(unsafe_code)]

mod common;

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use chrono::Utc;
use common::TestApp;
use futures::StreamExt;
use nexc::domain::trace::{GENESIS, Span, SpanKind, SpanStatus};
use nexc::domain::user::Role;
use nexc::ontology::stub::{StubOntology, server};
use nexc::ontology::{
    AskAnswer, AskMode, AskRequest, AskStreamEvent, Direction, EventQuery, FindQuery, OntoError,
    OntologyBackend, Principal, TraverseQuery, ask, export, facts, workspace_name,
};
use nexc::repo;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

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

/// The caller's workspace id.
async fn workspace(app: &TestApp, token: &str) -> Uuid {
    let (_, list) = call(app, Method::GET, "/workspaces", token, None).await;
    list[0]["id"].as_str().unwrap().parse().unwrap()
}

async fn agent(app: &TestApp, token: &str, name: &str, role: &str, capabilities: Value) -> Value {
    let (status, agent) = call(
        app,
        Method::POST,
        "/agents",
        token,
        Some(json!({
            "name": name, "role": role, "runtime": "python",
            "purpose": format!("Handles {role} work in tests."),
            "system_prompt": "SECRET-PROMPT: never a fact",
            "capabilities": capabilities,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{agent}");
    agent
}

/// A member of `wid` with `role`, as a bearer token.
async fn member(app: &TestApp, pool: &PgPool, wid: Uuid, role: &str) -> String {
    let email = format!("{role}-{}@example.com", Uuid::now_v7());
    let user =
        nexc::http::handlers::auth::create_user(&app.state, &email, "Member", Role::User, "!")
            .await
            .unwrap();
    sqlx::query("INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3)")
        .bind(wid)
        .bind(user.id)
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
    app.state.jwt.issue(user.id, Role::User, 0).unwrap()
}

/// What a workspace with one traced run looks like.
struct World {
    app: TestApp,
    stub: Arc<StubOntology>,
    owner: String,
    wid: Uuid,
    onto_ws: String,
    writer: Value,
    coder: Value,
    run_id: Uuid,
    node_id: Uuid,
    document_id: Uuid,
    memory_id: Uuid,
}

impl World {
    fn principal(&self, agent: &Value) -> String {
        agent["principal_id"].as_str().unwrap().to_owned()
    }

    fn agent_entity(&self, agent: &Value) -> String {
        facts::agent_id(&self.principal(agent))
    }
}

#[allow(clippy::too_many_arguments)]
fn span(
    run_id: Uuid,
    node_id: Uuid,
    agent_id: Uuid,
    principal: &str,
    seq: i32,
    kind: SpanKind,
    name: &str,
    status: SpanStatus,
    attributes: Value,
) -> Span {
    let now = Utc::now();
    Span {
        id: Uuid::now_v7(),
        run_id,
        node_id,
        agent_id: Some(agent_id),
        principal: principal.to_owned(),
        parent_id: None,
        kind,
        name: name.to_owned(),
        started_at: now,
        ended_at: Some(now),
        status,
        attributes,
        seq,
        prev_hash: GENESIS,
        hash: GENESIS,
    }
}

/// A running run of a new graph with one node, as rows.
async fn run_row(pool: &PgPool, app: &TestApp, token: &str) -> (Uuid, Uuid, Uuid) {
    let gid = app.graph(token, "Audit the audit").await;
    let node = app
        .node(
            token,
            &gid,
            json!({"title": "SECRET-NODE-TITLE", "content": "SECRET-NODE-CONTENT"}),
        )
        .await;
    let graph_id: Uuid = gid.parse().unwrap();
    let owner: Uuid = sqlx::query_scalar("SELECT owner_id FROM graphs WHERE id = $1")
        .bind(graph_id)
        .fetch_one(pool)
        .await
        .unwrap();
    let run_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO runs (id, graph_id, owner_id, status, max_concurrency, started_at)
         VALUES ($1, $2, $3, 'running', 1, now())",
    )
    .bind(run_id)
    .bind(graph_id)
    .bind(owner)
    .execute(pool)
    .await
    .unwrap();
    (graph_id, run_id, node.parse().unwrap())
}

/// A ready document with one passage, labelled; returns `(document, chunk)`.
async fn document(pool: &PgPool, wid: Uuid, label: &str) -> (Uuid, Uuid) {
    let document = Uuid::now_v7();
    let chunk = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO documents (id, workspace_id, name, size_bytes, sha256, status, label)
         VALUES ($1, $2, 'SECRET-DOCUMENT-NAME.pdf', 10, $3, 'ready', $4)",
    )
    .bind(document)
    .bind(wid)
    .bind(Uuid::now_v7().to_string())
    .bind(label)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO document_chunks (id, document_id, workspace_id, ordinal, kind, content)
         VALUES ($1, $2, $3, 0, 'text', 'SECRET-PASSAGE about customs')",
    )
    .bind(chunk)
    .bind(document)
    .bind(wid)
    .execute(pool)
    .await
    .unwrap();
    (document, chunk)
}

async fn memory(pool: &PgPool, wid: Uuid, owner: Uuid, label: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO memories (id, owner_id, scope, kind, content, embedding, workspace_id, label)
         VALUES ($1, $2, 'user', 'fact', 'SECRET-MEMORY-CONTENT', decode(repeat('00', 1024), 'hex'), $3, $4)",
    )
    .bind(id)
    .bind(owner)
    .bind(wid)
    .bind(label)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn world(pool: PgPool) -> World {
    let stub = Arc::new(StubOntology::new());
    let app = TestApp::with_ontology(pool.clone(), &[], stub.clone()).await;
    let (owner, _) = app.register("auditor@example.com").await;
    let (_, me) = call(&app, Method::GET, "/auth/me", &owner, None).await;
    let owner_id: Uuid = me["id"].as_str().unwrap().parse().unwrap();
    let wid = workspace(&app, &owner).await;
    let writer = agent(&app, &owner, "writer", "writer", json!({})).await;
    let coder = agent(
        &app,
        &owner,
        "coder",
        "coder",
        json!({"tools": ["run_python", "write_file"], "egress_hosts": ["api.example.com"]}),
    )
    .await;
    let (_, run_id, node_id) = run_row(&pool, &app, &owner).await;
    let (document_id, chunk_id) = document(&pool, wid, "restricted").await;
    let memory_id = memory(&pool, wid, owner_id, "internal").await;
    let writer_id: Uuid = writer["id"].as_str().unwrap().parse().unwrap();
    let coder_id: Uuid = coder["id"].as_str().unwrap().parse().unwrap();
    let writer_principal = writer["principal_id"].as_str().unwrap();
    let coder_principal = coder["principal_id"].as_str().unwrap();
    let child = format!("{writer_principal}/researcher-1");
    let spans = vec![
        span(
            run_id,
            node_id,
            writer_id,
            writer_principal,
            0,
            SpanKind::DataRead,
            "context",
            SpanStatus::Ok,
            json!({"document_chunk_ids": [chunk_id], "memory_ids": [memory_id], "upstream_node_ids": [],
                   "documents": 1, "memories": 1}),
        ),
        span(
            run_id,
            node_id,
            writer_id,
            writer_principal,
            1,
            SpanKind::LlmCall,
            "claude-test",
            SpanStatus::Ok,
            json!({"model": "claude-test", "tokens_in": 100, "tokens_out": 20, "prompt": "SECRET-PROMPT-TEXT"}),
        ),
        span(
            run_id,
            node_id,
            writer_id,
            writer_principal,
            2,
            SpanKind::ToolCall,
            "write_file",
            SpanStatus::Ok,
            json!({"args_sha256": "ab", "result_bytes": 12, "args": "SECRET-ARGS"}),
        ),
        span(
            run_id,
            node_id,
            writer_id,
            writer_principal,
            3,
            SpanKind::Spawn,
            &child,
            SpanStatus::Ok,
            json!({"child": child, "role": "researcher", "depth": 1}),
        ),
        span(
            run_id,
            node_id,
            writer_id,
            &child,
            4,
            SpanKind::ToolCall,
            "web_search",
            SpanStatus::Error,
            json!({"args_sha256": "cd", "error": true}),
        ),
        span(
            run_id,
            node_id,
            coder_id,
            coder_principal,
            5,
            SpanKind::Policy,
            "evil.example",
            SpanStatus::Error,
            json!({"decision": "deny", "rule": "egress.hosts", "action": "egress",
                   "reason": "SECRET-REASON host evil.example is not allowed", "policy_version": 0}),
        ),
        span(
            run_id,
            node_id,
            coder_id,
            coder_principal,
            6,
            SpanKind::Egress,
            "api.example.com",
            SpanStatus::Ok,
            json!({"host": "api.example.com", "status": 200}),
        ),
        span(
            run_id,
            node_id,
            coder_id,
            coder_principal,
            7,
            SpanKind::Policy,
            "run_python",
            SpanStatus::Ok,
            json!({"decision": "allow", "rule": "tools.allow", "action": "tool"}),
        ),
        span(
            run_id,
            node_id,
            coder_id,
            coder_principal,
            8,
            SpanKind::Approval,
            "deploy",
            SpanStatus::Ok,
            json!({"decision": "approved", "approver": "user:ops"}),
        ),
    ];
    repo::spans::insert(&pool, &spans).await.unwrap();
    World {
        onto_ws: workspace_name(&app.state.settings.onto_workspace_prefix, wid),
        app,
        stub,
        owner,
        wid,
        writer,
        coder,
        run_id,
        node_id,
        document_id,
        memory_id,
    }
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn every_span_kind_is_exported_as_facts_and_the_cursor_moves(pool: PgPool) {
    let w = world(pool.clone()).await;
    let queued_before = repo::onto_export::queued(&pool).await.unwrap();
    assert!(
        queued_before >= 11,
        "the tables queued what they wrote: {queued_before}"
    );

    let progress = export::pass(&w.app.state).await.unwrap();
    assert_eq!(progress.failed, 0);
    assert_eq!(progress.workspaces, 1);
    // The backfill sends what the tables had before the queue took over, the
    // queue the rest: each span once.
    assert_eq!(progress.spans, 9, "{progress:?}");
    assert_eq!(
        repo::onto_export::queued(&pool).await.unwrap(),
        0,
        "the queue is drained"
    );
    let cursor = repo::onto_export::cursor(&pool, w.wid)
        .await
        .unwrap()
        .unwrap();
    assert!(cursor.backfilled_at.is_some());
    assert!(cursor.spans_exported >= 9 && cursor.records_exported > cursor.spans_exported);
    assert!(cursor.last_error.is_none());

    let ws = &w.onto_ws;
    let writer = w.agent_entity(&w.writer);
    let coder = w.agent_entity(&w.coder);
    let run = facts::run_id(w.run_id);
    let node = facts::node_id(w.node_id);
    let child = format!("{writer}/researcher-1");
    let document = facts::document_id(w.document_id);
    let memory = facts::memory_id(w.memory_id);

    let entity = w.stub.entity(ws, &writer).expect("the writer is an entity");
    assert_eq!(entity.title, "writer");
    assert_eq!(entity.props["nexc_agent_id"], w.writer["id"]);
    assert_eq!(entity.props["sub_agent"], false);
    assert_eq!(w.stub.entity(ws, &child).unwrap().props["sub_agent"], true);
    let run_entity = w.stub.entity(ws, &run).unwrap();
    assert_eq!(run_entity.state, "running");
    assert_eq!(
        w.stub.entity(ws, &document).unwrap().props["label"],
        "restricted"
    );
    assert_eq!(
        w.stub.entity(ws, &document).unwrap().props["visibility"],
        "restricted"
    );
    assert_eq!(
        w.stub.entity(ws, &memory).unwrap().props["label"],
        "internal"
    );

    for (from, rel, to) in [
        (writer.as_str(), "ran", run.as_str()),
        (run.as_str(), "executed", node.as_str()),
        (writer.as_str(), "used_tool", "tool:write_file"),
        (child.as_str(), "used_tool", "tool:web_search"),
        (writer.as_str(), "used_model", "model:claude-test"),
        (writer.as_str(), "read", document.as_str()),
        (writer.as_str(), "read", memory.as_str()),
        (coder.as_str(), "called", "host:api.example.com"),
        (writer.as_str(), "delegated_to", child.as_str()),
        (
            &facts::policy_id(&w.principal(&w.coder)),
            "denied",
            "action:egress/evil.example",
        ),
        ("action:egress/evil.example", "denied_in", run.as_str()),
        (
            coder.as_str(),
            "governed_by",
            &facts::policy_id(&w.principal(&w.coder)),
        ),
        (
            &facts::policy_id(&w.principal(&w.coder)),
            "allows",
            "tool:run_python",
        ),
    ] {
        assert!(
            w.stub.has_link(ws, from, rel, to),
            "{from} —{rel}→ {to}: {:?}",
            w.stub.links(ws)
        );
    }

    let events = w.stub.events_of(ws);
    let actions: Vec<&str> = events.iter().map(|e| e.action.as_str()).collect();
    for action in [
        "agent.read",
        "agent.llm",
        "agent.tool",
        "agent.delegated",
        "policy.denied",
        "agent.egress",
        "agent.approval",
        "policy.changed",
    ] {
        assert!(actions.contains(&action), "{action} in {actions:?}");
    }
    let denied = events.iter().find(|e| e.action == "policy.denied").unwrap();
    assert_eq!(denied.target, run);
    assert_eq!(denied.payload["rule"], "egress.hosts");
    assert_eq!(denied.payload["subject"], "evil.example");
    assert!(
        !actions.contains(&"policy.allowed"),
        "allows are not events"
    );
    let reads: Vec<_> = events.iter().filter(|e| e.action == "agent.read").collect();
    assert_eq!(reads.len(), 2);
    assert_eq!(
        events.iter().filter(|e| e.action == "agent.llm").count(),
        1,
        "each span once"
    );

    // Nothing new: nothing goes out.
    let ingests = w.stub.ingests();
    let again = export::pass(&w.app.state).await.unwrap();
    assert_eq!(again.records, 0);
    assert_eq!(w.stub.ingests(), ingests);

    // The run finishes: its status and cost follow.
    sqlx::query("UPDATE runs SET status = 'succeeded', finished_at = now(), cost_usd = 0.5, tokens_in = 100, tokens_out = 20 WHERE id = $1")
        .bind(w.run_id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(repo::onto_export::queued(&pool).await.unwrap(), 1);
    export::pass(&w.app.state).await.unwrap();
    let run_entity = w.stub.entity(ws, &run).unwrap();
    assert_eq!(run_entity.state, "succeeded");
    assert_eq!(run_entity.props["cost_usd"], 0.5);
    assert!(
        w.stub
            .events_of(ws)
            .iter()
            .any(|e| e.action == "run.finished" && e.target == run)
    );

    // A policy change follows too.
    let (status, _) = call(
        &w.app,
        Method::PATCH,
        &format!("/agents/{}", w.coder["id"].as_str().unwrap()),
        &w.owner,
        Some(json!({"capabilities": {"tools": ["write_file"]}})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    export::pass(&w.app.state).await.unwrap();
    let policy = w
        .stub
        .entity(ws, &facts::policy_id(&w.principal(&w.coder)))
        .unwrap();
    assert_eq!(policy.props["version"], 1);
    assert_eq!(policy.props["tools"], json!(["write_file"]));
    assert!(w.stub.has_link(
        ws,
        &facts::policy_id(&w.principal(&w.coder)),
        "allows",
        "tool:write_file"
    ));
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_export_resumes_after_a_failure_and_a_restart(pool: PgPool) {
    let w = world(pool.clone()).await;
    let queued = repo::onto_export::queued(&pool).await.unwrap();

    // onto-intelli is down: nothing is lost, the failure is on record.
    w.stub
        .fail_with(Some(OntoError::Unavailable("connection refused".into())));
    let progress = export::pass(&w.app.state).await.unwrap();
    assert_eq!(progress.failed, 1);
    assert_eq!(
        repo::onto_export::queued(&pool).await.unwrap(),
        queued,
        "claimed rows came back"
    );
    let cursor = repo::onto_export::cursor(&pool, w.wid)
        .await
        .unwrap()
        .unwrap();
    assert!(cursor.backfilled_at.is_none());
    assert!(
        cursor
            .last_error
            .as_deref()
            .unwrap()
            .contains("connection refused")
    );
    assert!(w.stub.entity_ids(&w.onto_ws).is_empty());

    // Back: everything arrives, once each.
    w.stub.fail_with(None);
    let progress = export::pass(&w.app.state).await.unwrap();
    assert_eq!(progress.failed, 0);
    assert_eq!(progress.spans, 9, "{progress:?}");
    assert_eq!(repo::onto_export::queued(&pool).await.unwrap(), 0);
    let entities = w.stub.entity_ids(&w.onto_ws);
    let links = w.stub.links(&w.onto_ws).len();
    let events = w.stub.events_of(&w.onto_ws).len();

    // A restart that forgot where it was (the cursor row is gone) backfills
    // again; onto-intelli ends up with the same graph, not a doubled one.
    sqlx::query("DELETE FROM onto_export_cursors WHERE workspace_id = $1")
        .bind(w.wid)
        .execute(&pool)
        .await
        .unwrap();
    let progress = export::pass(&w.app.state).await.unwrap();
    assert_eq!(
        progress.spans, 9,
        "the backfill sends them again (the queue is empty)"
    );
    assert_eq!(w.stub.entity_ids(&w.onto_ws), entities);
    assert_eq!(w.stub.links(&w.onto_ws).len(), links);
    assert!(
        w.stub.events_of(&w.onto_ws).len() >= events,
        "events are appended, never lost"
    );

    // More spans than one backfill stream holds still all arrive.
    sqlx::query("DELETE FROM onto_export_cursors WHERE workspace_id = $1")
        .bind(w.wid)
        .execute(&pool)
        .await
        .unwrap();
    w.stub.clear();
    let principal = w.principal(&w.writer);
    let agent_id: Uuid = w.writer["id"].as_str().unwrap().parse().unwrap();
    let many: Vec<Span> = (0..(export::BACKFILL_BATCH as i32 + 50))
        .map(|i| {
            span(
                w.run_id,
                w.node_id,
                agent_id,
                &principal,
                100 + i,
                SpanKind::ToolCall,
                &format!("tool-{}", i % 7),
                SpanStatus::Ok,
                json!({"args_sha256": "ab"}),
            )
        })
        .collect();
    repo::spans::insert(&pool, &many).await.unwrap();
    let progress = export::pass(&w.app.state).await.unwrap();
    assert_eq!(progress.failed, 0);
    assert_eq!(
        progress.spans,
        9 + many.len(),
        "the old spans by the backfill, the new by the queue: {progress:?}"
    );
    assert_eq!(repo::onto_export::queued(&pool).await.unwrap(), 0);
    for i in 0..7 {
        assert!(w.stub.has_link(
            &w.onto_ws,
            &facts::agent_id(&principal),
            "used_tool",
            &format!("tool:tool-{i}")
        ));
    }
    let cursor = repo::onto_export::cursor(&pool, w.wid)
        .await
        .unwrap()
        .unwrap();
    assert!(cursor.backfilled_at.is_some());

    // With the feature off, housekeeping empties what the tables queue.
    let off = TestApp::new(pool.clone(), &[]).await;
    repo::spans::insert(
        &pool,
        &[span(
            w.run_id,
            w.node_id,
            agent_id,
            &principal,
            5000,
            SpanKind::ToolCall,
            "late",
            SpanStatus::Ok,
            json!({}),
        )],
    )
    .await
    .unwrap();
    assert_eq!(repo::onto_export::queued(&pool).await.unwrap(), 1);
    export::purge_when_off(&off.state).await.unwrap();
    assert_eq!(repo::onto_export::queued(&pool).await.unwrap(), 0);
    assert_eq!(
        export::pass(&off.state).await.unwrap(),
        export::Progress::default(),
        "off: no pass"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn nothing_but_ids_names_hosts_counts_and_decisions_leaves(pool: PgPool) {
    let w = world(pool.clone()).await;
    export::pass(&w.app.state).await.unwrap();
    let text = serde_json::to_string(&w.stub.records()).unwrap();
    for secret in [
        "SECRET-PROMPT",
        "SECRET-ARGS",
        "SECRET-REASON",
        "SECRET-NODE-TITLE",
        "SECRET-NODE-CONTENT",
        "SECRET-DOCUMENT-NAME",
        "SECRET-PASSAGE",
        "SECRET-MEMORY-CONTENT",
        "Handles writer work",
    ] {
        assert!(
            !text.contains(secret),
            "{secret} reached onto-intelli: {text}"
        );
    }
    // What does reach it: ids, the tool, the host, the rule, the decision, the label.
    for fact in [
        "tool:write_file",
        "host:api.example.com",
        "egress.hosts",
        "\"deny\"",
        "restricted",
        "claude-test",
    ] {
        assert!(text.contains(fact), "{fact} is a fact");
    }
}

/// Asks and expects an answer.
async fn ask_ok(app: &TestApp, token: &str, wid: Uuid, question: &str) -> Value {
    let (status, body) = call(
        app,
        Method::POST,
        &format!("/workspaces/{wid}/audit/ask"),
        token,
        Some(json!({"question": question})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{question}: {body}");
    body
}

fn fact_ids(answer: &Value) -> Vec<String> {
    answer["facts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap().to_owned())
        .collect()
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_canned_questions_are_answered_from_the_graph(pool: PgPool) {
    let w = world(pool.clone()).await;
    export::pass(&w.app.state).await.unwrap();
    let writer_link = format!("/app/agents?agent={}", w.writer["id"].as_str().unwrap());
    let coder_link = format!("/app/agents?agent={}", w.coder["id"].as_str().unwrap());

    let (status, chips) = call(
        &w.app,
        Method::GET,
        &format!("/workspaces/{}/audit/questions", w.wid),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(chips.as_array().unwrap().len(), 6);
    assert_eq!(chips[0]["label"], ask::CANNED[0].label);

    // 1. Restricted reads: the writer, with the document it read.
    let answer = ask_ok(
        &w.app,
        &w.owner,
        w.wid,
        "Which agents read restricted data this week?",
    )
    .await;
    assert_eq!(answer["mode"], "deterministic");
    assert_eq!(answer["tokens"], 0);
    assert_eq!(answer["unverified"], false);
    assert_eq!(answer["question"], "restricted_reads");
    assert!(
        answer["answer"]
            .as_str()
            .unwrap()
            .contains("writer read 1 item"),
        "{answer}"
    );
    let facts = answer["facts"].as_array().unwrap();
    let writer_fact = facts.iter().find(|f| f["kind"] == "agent").unwrap();
    assert_eq!(writer_fact["label"], "writer");
    assert_eq!(writer_fact["link"], writer_link);
    let document_fact = facts.iter().find(|f| f["kind"] == "document").unwrap();
    assert_eq!(document_fact["link"], "/app/settings/knowledge");
    assert!(
        document_fact["label"]
            .as_str()
            .unwrap()
            .contains("restricted")
    );
    assert!(answer["latency_ms"].as_f64().unwrap() >= 0.0);

    // 2. Hosts outside the allow-list: the coder's refused egress.
    let answer = ask_ok(
        &w.app,
        &w.owner,
        w.wid,
        "Which agents called hosts outside the allow-list?",
    )
    .await;
    assert_eq!(answer["question"], "egress_denied");
    assert!(
        answer["answer"]
            .as_str()
            .unwrap()
            .contains("coder tried evil.example"),
        "{answer}"
    );
    let ids = fact_ids(&answer);
    assert!(
        ids.contains(&"action:egress/evil.example".to_owned())
            && ids.contains(&facts::run_id(w.run_id))
    );
    let coder_fact = answer["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["kind"] == "agent")
        .unwrap();
    assert_eq!(coder_fact["link"], coder_link);

    // 3. Why was the run denied: the rule, the policy, the action.
    let answer = ask_ok(
        &w.app,
        &w.owner,
        w.wid,
        &format!("Why was run {} denied?", w.run_id),
    )
    .await;
    assert_eq!(answer["question"], "run_denied");
    let text = answer["answer"].as_str().unwrap();
    assert!(
        text.contains("coder was refused egress evil.example by rule egress.hosts"),
        "{text}"
    );
    let ids = fact_ids(&answer);
    assert_eq!(ids[0], facts::run_id(w.run_id));
    assert!(ids.contains(&facts::policy_id(&w.principal(&w.coder))));
    let run_fact = &answer["facts"][0];
    assert_eq!(run_fact["link"], format!("/app/runs/{}", w.run_id));
    let policy_fact = answer["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["kind"] == "policy")
        .unwrap();
    assert_eq!(policy_fact["link"], coder_link);
    let answer = ask_ok(&w.app, &w.owner, w.wid, "Why was run denied?").await;
    assert!(
        answer["answer"]
            .as_str()
            .unwrap()
            .starts_with("Name the run")
    );
    assert_eq!(answer["facts"].as_array().unwrap().len(), 0);
    assert_eq!(answer["unverified"], false);
    let (status, _) = call(
        &w.app,
        Method::POST,
        &format!("/workspaces/{}/audit/ask", w.wid),
        &w.owner,
        Some(json!({"question": format!("Why was run {} denied?", Uuid::now_v7())})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a run of another workspace, or none, is not found"
    );

    // 4. What an agent did today.
    let answer = ask_ok(&w.app, &w.owner, w.wid, "What did agent writer do today?").await;
    assert_eq!(answer["question"], "agent_day");
    let text = answer["answer"].as_str().unwrap();
    // The workspace was seeded with an org chart that has a "Writer" too.
    assert!(text.contains("today: worked in 1 run"), "{text}");
    assert!(
        text.contains("took 1 model turn")
            && text.contains("1 tool call")
            && text.contains("read 2 items"),
        "{text}"
    );
    assert!(text.contains("delegated to 1 sub-agent"), "{text}");
    let ids = fact_ids(&answer);
    assert!(ids.contains(&w.agent_entity(&w.writer)), "{ids:?}");
    assert!(ids.contains(&"tool:write_file".to_owned()));
    let answer = ask_ok(&w.app, &w.owner, w.wid, "What did agent nobody do today?").await;
    assert!(
        answer["answer"]
            .as_str()
            .unwrap()
            .contains("no agent named nobody")
    );

    // 5. Policies that allow code execution: the writer's (every tool) and the coder's (listed).
    let answer = ask_ok(
        &w.app,
        &w.owner,
        w.wid,
        "Which policies allow code execution?",
    )
    .await;
    assert_eq!(answer["question"], "code_policies");
    let text = answer["answer"].as_str().unwrap();
    let (_, agents) = call(
        &w.app,
        Method::GET,
        &format!("/agents?workspace_id={}", w.wid),
        &w.owner,
        None,
    )
    .await;
    let unrestricted = agents
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["capabilities"]["tools"].is_null())
        .count();
    assert!(
        text.starts_with(&format!(
            "{} policies allow code execution",
            unrestricted + 1
        )),
        "{text}"
    );
    assert!(
        text.contains("writer v0 (every tool)") && text.contains("coder v0 (run_python listed)"),
        "{text}"
    );
    let ids = fact_ids(&answer);
    assert!(ids.contains(&facts::policy_id(&w.principal(&w.writer))));
    assert!(ids.contains(&facts::policy_id(&w.principal(&w.coder))));

    // 6. Approvals this month.
    let answer = ask_ok(&w.app, &w.owner, w.wid, "Who approved what this month?").await;
    assert_eq!(answer["question"], "approvals");
    let text = answer["answer"].as_str().unwrap();
    assert!(
        text.contains("user:ops approved deploy for coder"),
        "{text}"
    );
    assert!(fact_ids(&answer).contains(&facts::run_id(w.run_id)));

    // A member is not cleared for restricted data: the document is withheld,
    // in the facts and in the words; the agent stays.
    let member = member(&w.app, &pool, w.wid, "member").await;
    let answer = ask_ok(
        &w.app,
        &member,
        w.wid,
        "Which agents read restricted data this week?",
    )
    .await;
    assert_eq!(answer["mode"], "deterministic");
    let facts = answer["facts"].as_array().unwrap();
    assert!(
        facts
            .iter()
            .any(|f| f["kind"] == "agent" && f["link"] == writer_link)
    );
    let withheld = facts.iter().find(|f| f["kind"] == "withheld").unwrap();
    assert!(
        withheld["label"]
            .as_str()
            .unwrap()
            .contains("restricted document")
    );
    assert!(withheld["link"].is_null());
    assert!(!facts.iter().any(|f| f["kind"] == "document"));
    assert!(
        !answer.to_string().contains(&w.document_id.to_string()),
        "{answer}"
    );
    // Internal data is within a member's clearance.
    let answer = ask_ok(&w.app, &member, w.wid, "What did agent writer do today?").await;
    assert!(fact_ids(&answer).contains(&facts::memory_id(w.memory_id)));
    assert!(!fact_ids(&answer).contains(&facts::document_id(w.document_id)));

    // The streaming variant answers a canned question in one `done` event.
    let r = w
        .app
        .request(
            Method::POST,
            &format!("/api/v1/workspaces/{}/audit/ask/stream", w.wid),
            Some(&w.owner),
            Some(json!({"question": "Which policies allow code execution?"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        r.headers["content-type"].to_str().unwrap(),
        "text/event-stream"
    );
    let body = r.body.as_str().unwrap();
    assert!(body.contains("event: done\ndata: {\"answer\":\""), "{body}");
    assert!(
        body.contains("policies allow code execution")
            && body.contains("\"mode\":\"deterministic\""),
        "{body}"
    );
    assert!(!body.contains("event: delta"));
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn free_questions_go_to_onto_intelli_and_answers_without_facts_are_marked(pool: PgPool) {
    let w = world(pool.clone()).await;
    export::pass(&w.app.state).await.unwrap();
    let writer = w.agent_entity(&w.writer);
    let document = facts::document_id(w.document_id);
    let cited = format!("{writer} read {document}");
    let cited_document = document.clone();
    w.stub.answer_with(Box::new(move |principal, req| {
        assert_eq!(req.mode, AskMode::Auto);
        assert!(principal.id.starts_with("user:"));
        if req.question.contains("cite") {
            AskAnswer {
                answer: format!("The writer read one document ({cited_document})."),
                mode: "llm".into(),
                intent: "reasoning".into(),
                facts: vec![cited.clone()],
                prompt_tokens: 900,
                completion_tokens: 40,
                total_ms: 3.5,
                ..AskAnswer::default()
            }
        } else if req.question.contains("count") {
            AskAnswer {
                answer: "Workspace has 1 run.".into(),
                mode: "deterministic".into(),
                intent: "count".into(),
                facts: vec![],
                ..AskAnswer::default()
            }
        } else {
            AskAnswer {
                answer: "I believe the writer leaked the quarterly numbers.".into(),
                mode: "llm".into(),
                intent: "reasoning".into(),
                facts: vec![],
                prompt_tokens: 1200,
                completion_tokens: 60,
                ..AskAnswer::default()
            }
        }
    }));

    // Claims with facts: verified, the facts resolved and linked.
    let answer = ask_ok(&w.app, &w.owner, w.wid, "Please cite what the writer read").await;
    assert_eq!(answer["mode"], "llm");
    assert_eq!(answer["tokens"], 940);
    assert_eq!(answer["unverified"], false);
    assert!(answer["question"].is_null());
    let ids = fact_ids(&answer);
    assert_eq!(ids, vec![writer.clone(), document.clone()]);
    assert_eq!(
        answer["facts"][0]["link"],
        format!("/app/agents?agent={}", w.writer["id"].as_str().unwrap())
    );
    let asks = w.stub.asks();
    assert_eq!(asks.last().unwrap().0.workspace, w.onto_ws);

    // Claims without a fact: unverified.
    let answer = ask_ok(&w.app, &w.owner, w.wid, "Did anything leak?").await;
    assert_eq!(answer["mode"], "llm");
    assert_eq!(answer["unverified"], true);
    assert_eq!(answer["facts"].as_array().unwrap().len(), 0);
    assert_eq!(answer["tokens"], 1260);

    // A deterministic answer of onto-intelli's own keeps its mode.
    let answer = ask_ok(
        &w.app,
        &w.owner,
        w.wid,
        "How many runs does the workspace count?",
    )
    .await;
    assert_eq!(answer["mode"], "deterministic");
    assert_eq!(answer["tokens"], 0);
    assert_eq!(
        answer["unverified"], true,
        "a count with no fact cited is still unverified"
    );

    // A member gets the clearance filter on a model's words: the restricted
    // document id is replaced, the fact withheld.
    let member = member(&w.app, &pool, w.wid, "member").await;
    let answer = ask_ok(&w.app, &member, w.wid, "Please cite what the writer read").await;
    assert!(
        !answer.to_string().contains(&w.document_id.to_string()),
        "{answer}"
    );
    assert!(
        answer["answer"]
            .as_str()
            .unwrap()
            .contains("a restricted document")
    );
    assert_eq!(answer["facts"][1]["kind"], "withheld");

    // Streamed, for an admin: stages and deltas, then the whole answer.
    let r = w
        .app
        .request(
            Method::POST,
            &format!("/api/v1/workspaces/{}/audit/ask/stream", w.wid),
            Some(&w.owner),
            Some(json!({"question": "Please cite what the writer read"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let body = r.body.as_str().unwrap();
    assert!(
        body.contains("event: stage\ndata: {\"text\":\"intent\"}"),
        "{body}"
    );
    assert!(
        body.contains("event: delta\ndata: {\"text\":\"The writer read one document"),
        "{body}"
    );
    assert!(
        body.contains("event: done\ndata: {\"answer\":\"The writer read one document"),
        "{body}"
    );
    assert!(body.contains("\"unverified\":false"));
    // Streamed, for a member: one `done`, filtered.
    let r = w
        .app
        .request(
            Method::POST,
            &format!("/api/v1/workspaces/{}/audit/ask/stream", w.wid),
            Some(&member),
            Some(json!({"question": "Please cite what the writer read"})),
        )
        .await;
    let body = r.body.as_str().unwrap();
    assert!(
        !body.contains("event: delta") && body.contains("event: done"),
        "{body}"
    );
    assert!(!body.contains(&w.document_id.to_string()));
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_ask_endpoint_fails_safely(pool: PgPool) {
    // Off: a clear 503, as problem+json; the chips are still there.
    let off = TestApp::new(pool.clone(), &[]).await;
    let (owner, _) = off.register("owner@example.com").await;
    let wid = workspace(&off, &owner).await;
    let r = off
        .request(
            Method::POST,
            &format!("/api/v1/workspaces/{wid}/audit/ask"),
            Some(&owner),
            Some(json!({"question": "Which agents read restricted data this week?"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(r.headers["content-type"], "application/problem+json");
    assert_eq!(r.body["status"], 503);
    assert!(
        r.body["detail"].as_str().unwrap().contains("NEXC_ONTO_URL"),
        "{}",
        r.body
    );
    let (status, chips) = call(
        &off,
        Method::GET,
        &format!("/workspaces/{wid}/audit/questions"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(chips.as_array().unwrap().len(), 6);
    let r = off
        .request(
            Method::POST,
            &format!("/api/v1/workspaces/{wid}/audit/ask/stream"),
            Some(&owner),
            Some(json!({"question": "Which agents read restricted data this week?"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::SERVICE_UNAVAILABLE);
    drop(off);

    let w = world(pool.clone()).await;
    let path = format!("/workspaces/{}/audit/ask", w.wid);
    let ask = |token: String, body: Value| {
        let app = &w.app;
        let path = path.clone();
        async move { call(app, Method::POST, &path, &token, Some(body)).await }
    };

    // onto-intelli down: 503 with the reason; denied: 403, passed through.
    w.stub
        .fail_with(Some(OntoError::Unavailable("connection refused".into())));
    let (status, body) = ask(
        w.owner.clone(),
        json!({"question": "Which agents read restricted data this week?"}),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(
        body["detail"].as_str().unwrap().contains("not reachable"),
        "{body}"
    );
    let (status, body) = ask(w.owner.clone(), json!({"question": "Tell me everything"})).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    w.stub.fail_with(Some(OntoError::Denied(
        "user:x has no access to workspace w".into(),
    )));
    let (status, body) = ask(w.owner.clone(), json!({"question": "Tell me everything"})).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(
        body["detail"].as_str().unwrap().contains("no access"),
        "{body}"
    );
    w.stub
        .fail_with(Some(OntoError::Invalid("unknown entity".into())));
    let (status, _) = ask(w.owner.clone(), json!({"question": "Tell me everything"})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    w.stub.fail_with(None);

    // Oversized, empty, wrong shape.
    let (status, body) = ask(
        w.owner.clone(),
        json!({"question": "x".repeat(ask::QUESTION_MAX + 1)}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["errors"]["question"][0]
            .as_str()
            .unwrap()
            .contains("1000")
    );
    let (status, _) = ask(w.owner.clone(), json!({"question": "   "})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = ask(w.owner.clone(), json!({"question": "fine", "mode": "llm"})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = ask(w.owner.clone(), json!({"q": 1})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = ask(w.owner.clone(), json!("not an object")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = ask(
        w.owner.clone(),
        json!({"question": "x".repeat(ask::QUESTION_MAX)}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the limit itself is fine");

    // Who may ask: members and above; guests 403; outsiders 404; nobody without a token.
    let guest = member(&w.app, &pool, w.wid, "guest").await;
    let (status, body) = ask(guest, json!({"question": "Who approved what this month?"})).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (outsider, _) = w.app.register("outsider@example.com").await;
    let (status, _) = ask(
        outsider.clone(),
        json!({"question": "Who approved what this month?"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &w.app,
        Method::GET,
        &format!("/workspaces/{}/audit/questions", w.wid),
        &outsider,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let r = w
        .app
        .request(
            Method::POST,
            &format!("/api/v1{path}"),
            None,
            Some(json!({"question": "x"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = w
        .app
        .request(
            Method::POST,
            &format!("/api/v1{path}"),
            Some("not-a-token"),
            Some(json!({"question": "x"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_grpc_client_speaks_to_a_server_end_to_end(pool: PgPool) {
    let stub = Arc::new(StubOntology::new());
    let running = server::serve(stub.clone(), Some("onto-token"))
        .await
        .unwrap();
    let url = running.url();
    let client = nexc::ontology::grpc::GrpcOntology::new(&url, Some("onto-token")).unwrap();
    let principal = Principal::system("ws-1");

    let health = client.health().await.unwrap();
    assert_eq!(health.version, "stub");
    let accepted = client
        .ingest(
            "ws-1",
            vec![
                json!({"op": "upsert", "kind": "agent", "key": "w/a", "title": "A", "props": {"n": 1}}),
                json!({"op": "link", "from": "agent:w/a", "rel": "ran", "to": "run:r"}),
                json!({"op": "event", "action": "agent.tool", "actor": "agent:w/a", "target": "tool:t", "ts": 7, "payload": {"run": "r"}}),
            ],
        )
        .await
        .unwrap();
    assert_eq!(accepted.accepted, 3);
    assert!(
        stub.has_link("ws-1", "agent:w/a", "ran", "run:r"),
        "the workspace header selected ws-1"
    );
    let found = client
        .find(
            &principal,
            FindQuery {
                kind: "agent".into(),
                props: json!({"n": 1}),
                state: vec![],
                limit: 10,
            },
        )
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].title, "A");
    assert_eq!(found[0].props["n"], 1);
    let walked = client
        .traverse(
            &principal,
            TraverseQuery {
                start: vec!["run:r".into()],
                rels: vec!["ran".into()],
                direction: Direction::In,
                depth: 2,
                limit: 10,
                kinds: vec![],
            },
        )
        .await
        .unwrap();
    assert_eq!(walked.nodes.len(), 2);
    assert_eq!(walked.edges[0].rel, "ran");
    let events = client
        .events(
            &principal,
            EventQuery {
                action: Some("agent".into()),
                since_ms: Some(1),
                limit: 10,
                ..EventQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].payload["run"], "r");
    assert_eq!(events[0].ts, 7);

    stub.answer_with(Box::new(|_, req| AskAnswer {
        answer: format!("You asked: {}", req.question),
        mode: "llm".into(),
        intent: "reasoning".into(),
        facts: vec!["agent:w/a ran run:r".into()],
        prompt_tokens: 12,
        completion_tokens: 3,
        total_ms: 1.5,
        ..AskAnswer::default()
    }));
    let answer = client
        .ask(
            &principal,
            AskRequest {
                question: "hello".into(),
                mode: AskMode::Auto,
                token_budget: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(answer.answer, "You asked: hello");
    assert_eq!(answer.tokens(), 15);
    assert_eq!(answer.facts, vec!["agent:w/a ran run:r".to_owned()]);
    let mut stream = client
        .ask_stream(
            &principal,
            AskRequest {
                question: "hello".into(),
                mode: AskMode::Auto,
                token_budget: None,
            },
        )
        .await
        .unwrap();
    let mut kinds = Vec::new();
    while let Some(event) = stream.next().await {
        kinds.push(match event {
            AskStreamEvent::Stage(_) => "stage",
            AskStreamEvent::Entities(_) => "entities",
            AskStreamEvent::Delta(_) => "delta",
            AskStreamEvent::Done(done) => {
                assert_eq!(done.answer, "You asked: hello");
                assert_eq!(done.prompt_tokens, 12);
                "done"
            }
            AskStreamEvent::Error(_) => "error",
        });
    }
    assert_eq!(kinds, ["stage", "entities", "delta", "done"]);

    // The wrong token is refused; a server failure is a status, not a crash.
    let wrong = nexc::ontology::grpc::GrpcOntology::new(&url, Some("nope")).unwrap();
    assert!(matches!(wrong.health().await, Err(OntoError::Denied(_))));
    stub.fail_with(Some(OntoError::Unavailable("busy".into())));
    assert!(matches!(client.health().await, Err(OntoError::Unavailable(m)) if m == "busy"));
    stub.fail_with(None);

    // The whole app over the wire: export, then a question.
    let app = TestApp::with_ontology(pool.clone(), &[], Arc::new(client)).await;
    let (owner, _) = app.register("wire@example.com").await;
    let wid = workspace(&app, &owner).await;
    let writer = agent(&app, &owner, "writer", "writer", json!({})).await;
    let (_, run_id, node_id) = run_row(&pool, &app, &owner).await;
    let writer_id: Uuid = writer["id"].as_str().unwrap().parse().unwrap();
    let principal_id = writer["principal_id"].as_str().unwrap();
    repo::spans::insert(
        &pool,
        &[span(
            run_id,
            node_id,
            writer_id,
            principal_id,
            0,
            SpanKind::Policy,
            "evil.example",
            SpanStatus::Error,
            json!({"decision": "deny", "rule": "egress.hosts", "action": "egress"}),
        )],
    )
    .await
    .unwrap();
    let progress = export::pass(&app.state).await.unwrap();
    assert_eq!(progress.failed, 0, "{progress:?}");
    let ws = workspace_name(&app.state.settings.onto_workspace_prefix, wid);
    assert!(stub.has_link(
        &ws,
        &facts::agent_id(principal_id),
        "ran",
        &facts::run_id(run_id)
    ));
    let answer = ask_ok(&app, &owner, wid, &format!("Why was run {run_id} denied?")).await;
    assert!(
        answer["answer"]
            .as_str()
            .unwrap()
            .contains("writer was refused egress evil.example"),
        "{answer}"
    );
    assert_eq!(answer["mode"], "deterministic");

    // The server gone: 503, not a hang.
    running.stop().await;
    let r = app
        .request(
            Method::POST,
            &format!("/api/v1/workspaces/{wid}/audit/ask"),
            Some(&owner),
            Some(json!({"question": "Who approved what this month?"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::SERVICE_UNAVAILABLE, "{}", r.body);
    let health = app.state.health.ontology(&app.state).await;
    assert!(health.enabled && !health.ok, "{health:?}");
}

fn pct(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((p / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

/// Ask latency end to end through the HTTP router, with the stub answering
/// deterministically: in process (the backend's own cost) and over gRPC to
/// the in-process server (what a loopback onto-intelli adds). The network to
/// a real onto-intelli is not part of it.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
#[ignore = "a measurement: run with `make prove-audit`"]
async fn prove_audit(pool: PgPool) {
    use std::time::Instant;

    const ASKS: usize = 2_000;
    let w = world(pool.clone()).await;
    export::pass(&w.app.state).await.unwrap();
    let path = format!("/api/v1/workspaces/{}/audit/ask", w.wid);
    let questions = [
        "Which agents read restricted data this week?",
        &format!("Why was run {} denied?", w.run_id),
        "What did agent writer do today?",
        "Which policies allow code execution?",
    ];
    let mut report = Vec::new();
    async fn measure(
        app: &TestApp,
        path: &str,
        token: &str,
        question: &str,
        label: &str,
    ) -> String {
        let mut samples = Vec::with_capacity(ASKS);
        for _ in 0..ASKS {
            let started = Instant::now();
            let r = app
                .request(
                    Method::POST,
                    path,
                    Some(token),
                    Some(json!({"question": question})),
                )
                .await;
            samples.push(started.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(r.status, StatusCode::OK, "{}", r.body);
            assert_eq!(r.body["mode"], "deterministic");
            assert_eq!(r.body["tokens"], 0);
        }
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        format!(
            "prove-audit: {label:<30} {:<46} {ASKS} asks: p50 {:.3} ms, p99 {:.3} ms, max {:.3} ms",
            question.chars().take(44).collect::<String>(),
            pct(&samples, 50.0),
            pct(&samples, 99.0),
            samples.last().copied().unwrap_or(0.0)
        )
    }
    for question in questions {
        report.push(measure(&w.app, &path, &w.owner, question, "in process, stub").await);
    }

    let running = server::serve(w.stub.clone(), Some("t")).await.unwrap();
    let client = nexc::ontology::grpc::GrpcOntology::new(&running.url(), Some("t")).unwrap();
    let wired = TestApp::with_ontology(pool.clone(), &[], Arc::new(client)).await;
    let (owner, _) = wired.register("prover@example.com").await;
    // The facts are in the stub already: point the wired app's user at the same workspace.
    let (_, me) = call(&wired, Method::GET, "/auth/me", &owner, None).await;
    let user_id: Uuid = me["id"].as_str().unwrap().parse().unwrap();
    sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'admin')",
    )
    .bind(w.wid)
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();
    report.push(
        measure(
            &wired,
            &path,
            &owner,
            questions[0],
            "over gRPC, in-process server",
        )
        .await,
    );
    running.stop().await;
    for line in report {
        println!("{line}");
    }
}
