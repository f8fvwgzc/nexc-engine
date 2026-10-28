//! The A2A adapter: an agent an admin exposes serves its card and takes
//! tasks over JSON-RPC from registered counterparts, each task a one-node
//! run of the agent with the caller and the firewall's decision on its
//! trace, its output and files answered as A2A artifacts; wrong, missing
//! and foreign tokens are refused as JSON-RPC errors with 401 and 403.
//! Counterparts are registered with their card fetched, and agents call
//! them back through the gateway as `a2a:<name>` under the policy, with the
//! hops in the task's metadata; a counterpart off the list, a host off the
//! list, a restricted-labelled message, a hop too deep and a loop are
//! refused as policy spans.
#![forbid(unsafe_code)]

mod common;

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use base64::Engine as _;
use common::grpc_runtime::fake_grpc_runtime;
use common::{TestApp, collect_until};
use nexc::a2a::percent_encode;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// What the remote A2A stub saw: every JSON-RPC request's method and params, and the auth headers.
#[derive(Default)]
struct StubSeen {
    methods: Vec<String>,
    params: Vec<Value>,
    auth: Vec<String>,
    /// Tasks taken by `tasks/send`, answered `working` first and `completed` on `tasks/get`.
    pending: Vec<String>,
}

#[derive(Clone)]
struct StubState {
    seen: Arc<Mutex<StubSeen>>,
    url: String,
    streaming: bool,
}

fn stub_task(id: &str, state: &str, message: &str) -> Value {
    let artifacts = if state == "completed" {
        json!([
            {"name": "answer", "index": 0, "parts": [{"type": "text", "text": format!("Remote says: {message}")}]},
            {"name": "figures.csv", "index": 1, "parts": [{"type": "file", "file": {"name": "figures.csv", "mimeType": "text/csv",
                "bytes": base64::engine::general_purpose::STANDARD.encode("quarter,revenue\nQ3,12.4\n")}}]}
        ])
    } else {
        json!([])
    };
    json!({"id": id, "sessionId": null, "status": {"state": state, "timestamp": "2026-10-18T12:00:00Z"}, "artifacts": artifacts})
}

/// The remote agent's JSON-RPC endpoint: `tasks/sendSubscribe` streams a working update, the
/// artifacts and a final status; `tasks/send` answers `working` and `tasks/get` `completed`.
async fn stub_rpc(
    State(state): State<StubState>,
    headers: HeaderMap,
    body: String,
) -> axum::response::Response {
    let request: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let method = request["method"].as_str().unwrap_or("").to_owned();
    let params = request["params"].clone();
    let id = request["id"].clone();
    {
        let mut seen = state.seen.lock().unwrap();
        seen.methods.push(method.clone());
        seen.params.push(params.clone());
        seen.auth.push(
            headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_owned(),
        );
    }
    let task_id = params["id"].as_str().unwrap_or("none").to_owned();
    let message = params["message"]["parts"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    match method.as_str() {
        "tasks/sendSubscribe" => {
            let event = |result: Value| {
                format!(
                    "data: {}\n\n",
                    json!({"jsonrpc": "2.0", "id": id, "result": result})
                )
            };
            let completed = stub_task(&task_id, "completed", &message);
            let mut body = String::new();
            body.push_str(&event(json!({"id": task_id, "status": {"state": "working"}, "final": false})));
            for artifact in completed["artifacts"].as_array().unwrap() {
                body.push_str(&event(json!({"id": task_id, "artifact": artifact})));
            }
            body.push_str(&event(json!({"id": task_id, "status": {"state": "completed"}, "final": true})));
            ([("content-type", "text/event-stream")], body).into_response()
        }
        "tasks/send" => {
            state.seen.lock().unwrap().pending.push(task_id.clone());
            axum::Json(json!({"jsonrpc": "2.0", "id": id, "result": stub_task(&task_id, "working", &message)}))
                .into_response()
        }
        "tasks/get" => {
            let message = state
                .seen
                .lock()
                .unwrap()
                .params
                .iter()
                .find(|p| p["id"] == task_id && p.get("message").is_some())
                .map(|p| p["message"]["parts"][0]["text"].as_str().unwrap_or("").to_owned())
                .unwrap_or_default();
            axum::Json(json!({"jsonrpc": "2.0", "id": id, "result": stub_task(&task_id, "completed", &message)}))
                .into_response()
        }
        _ => axum::Json(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "method not found"}}))
            .into_response(),
    }
}

async fn stub_card(State(state): State<StubState>) -> axum::Json<Value> {
    axum::Json(json!({
        "name": "Remote Stub",
        "description": "A remote A2A agent standing in for a partner.",
        "url": format!("{}/rpc", state.url),
        "capabilities": {"streaming": state.streaming, "pushNotifications": false},
        "defaultInputModes": ["text"],
        "defaultOutputModes": ["text", "file"],
        "skills": [{"id": "summary", "name": "Summaries", "description": "Summarises what it is sent."}],
    }))
}

/// Starts a remote A2A agent on this machine; returns its card URL and what it sees.
async fn a2a_stub(streaming: bool) -> (String, Arc<Mutex<StubSeen>>) {
    let seen: Arc<Mutex<StubSeen>> = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/.well-known/agent.json", get(stub_card))
        .route("/rpc", post(stub_rpc))
        .with_state(StubState {
            seen: seen.clone(),
            url: url.clone(),
            streaming,
        });
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("{url}/.well-known/agent.json"), seen)
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

async fn workspace(app: &TestApp, token: &str) -> String {
    let (_, list) = call(app, Method::GET, "/workspaces", token, None).await;
    list[0]["id"].as_str().unwrap().to_owned()
}

/// The settings every app here runs with: a public URL, and counterparts on this machine allowed.
const A2A_VARS: [(&str, &str); 2] = [
    ("NEXC_PUBLIC_URL", "https://nexc.example"),
    ("NEXC_A2A_ALLOW_PRIVATE_HOSTS", "true"),
];

async fn a2a_app(pool: PgPool) -> TestApp {
    TestApp::new(pool, &A2A_VARS).await
}

async fn grpc_app(pool: PgPool, url: &str) -> TestApp {
    let mut vars = vec![
        ("NEXC_RUNTIME_PROTOCOL", "grpc"),
        ("NEXC_RUNTIME_GRPC_URL", url),
    ];
    vars.extend_from_slice(&A2A_VARS);
    TestApp::new(pool, &vars).await
}

/// An Analyst from the role template, exposed over A2A, allowed to hear from `counterparts`.
async fn exposed_analyst(app: &TestApp, token: &str, counterparts: &[&str]) -> Value {
    let (status, card) = call(
        app,
        Method::POST,
        "/agents/from-template/analyst",
        token,
        Some(json!({"evaluate": false})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{card}");
    let id = card["agent"]["id"].as_str().unwrap().to_owned();
    let mut capabilities = card["agent"]["capabilities"].clone();
    capabilities["counterparts"] = json!(counterparts);
    capabilities["delegation"] = json!({"max_depth": 1, "max_fan_out": 1});
    let (status, agent) = call(
        app,
        Method::PATCH,
        &format!("/agents/{id}"),
        token,
        Some(json!({"capabilities": capabilities, "a2a_exposed": true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{agent}");
    assert_eq!(agent["a2a_exposed"], true);
    agent
}

/// Registers a counterpart at `card_url` (a stub started here); returns the
/// counterpart's principal (an external agent, as `{principal_id, id}`) and
/// the token its inbound calls present.
async fn counterpart(
    app: &TestApp,
    token: &str,
    wid: &str,
    name: &str,
    card_url: &str,
) -> (Value, String) {
    let (status, registered) = call(
        app,
        Method::POST,
        &format!("/workspaces/{wid}/a2a-counterparts"),
        token,
        Some(json!({"name": name, "card_url": card_url, "auth_header": "Bearer remote-secret"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{registered}");
    let counterpart = &registered["counterpart"];
    let agent = json!({
        "id": counterpart["agent_id"],
        "principal_id": counterpart["principal_id"],
        "counterpart_id": counterpart["id"],
    });
    (
        agent,
        registered["token"]["secret"].as_str().unwrap().to_owned(),
    )
}

/// A JSON-RPC call to an exposed agent's endpoint.
async fn rpc(
    app: &TestApp,
    principal: &str,
    token: Option<&str>,
    method: &str,
    params: Value,
) -> (StatusCode, Value) {
    let r = app
        .request(
            Method::POST,
            &format!("/a2a/agents/{}", percent_encode(principal)),
            token,
            Some(json!({"jsonrpc": "2.0", "id": 7, "method": method, "params": params})),
        )
        .await;
    (r.status, r.body)
}

fn text_message(text: &str) -> Value {
    json!({"role": "user", "parts": [{"type": "text", "text": text}]})
}

async fn trace_of(app: &TestApp, token: &str, run_id: &str) -> Vec<Value> {
    let (status, page) = call(
        app,
        Method::GET,
        &format!("/runs/{run_id}/trace?limit=1000"),
        token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    page["spans"].as_array().unwrap().clone()
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_exposed_agent_serves_its_card(pool: PgPool) {
    let app = a2a_app(pool).await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    let (member, _) = app.register("member@example.com").await;
    call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/members"),
        &owner,
        Some(json!({"email": "member@example.com", "role": "member"})),
    )
    .await;
    let (status, card) = call(
        &app,
        Method::POST,
        "/agents/from-template/analyst",
        &owner,
        Some(json!({"evaluate": false})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{card}");
    let agent = card["agent"].clone();
    let id = agent["id"].as_str().unwrap();
    let principal = agent["principal_id"].as_str().unwrap();
    assert_eq!(agent["a2a_exposed"], false);

    // Not exposed: no card anywhere, for anyone.
    let encoded = percent_encode(principal);
    let r = app
        .request(
            Method::GET,
            &format!("/a2a/agents/{encoded}/card"),
            None,
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.body);
    let r = app
        .request(
            Method::GET,
            &format!("/.well-known/agent.json?agent={encoded}"),
            None,
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.body);

    // A member may not publish it; an admin may, and it is audited.
    let (status, body) = call(
        &app,
        Method::PATCH,
        &format!("/agents/{id}"),
        &member,
        Some(json!({"a2a_exposed": true})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, exposed) = call(
        &app,
        Method::PATCH,
        &format!("/agents/{id}"),
        &owner,
        Some(json!({"a2a_exposed": true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{exposed}");
    assert_eq!(exposed["a2a_exposed"], true);
    let (_, audit) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/audit"),
        &owner,
        None,
    )
    .await;
    let entry = audit
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["action"] == "agent_a2a_changed")
        .expect("the change is audited");
    assert_eq!(entry["subject"], principal);
    assert!(entry["detail"].as_str().unwrap().contains("published"));

    // The card, at both addresses, without a token.
    let r = app
        .request(
            Method::GET,
            &format!("/a2a/agents/{encoded}/card"),
            None,
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let card = r.body;
    assert_eq!(card["name"], "Financial Analyst");
    assert_eq!(card["principal"], principal);
    assert_eq!(
        card["url"],
        format!("https://nexc.example/a2a/agents/{encoded}")
    );
    assert_eq!(card["capabilities"]["streaming"], true);
    assert_eq!(card["authentication"]["schemes"][0], "Bearer");
    assert_eq!(card["defaultInputModes"], json!(["text", "file"]));
    let skills = card["skills"].as_array().unwrap();
    assert_eq!(skills[0]["id"], "role:analyst");
    assert!(
        skills[0]["tags"]
            .as_array()
            .unwrap()
            .contains(&json!("analyst"))
    );
    assert!(
        skills.iter().any(|s| s["id"] == "tool:make_docx"),
        "the capabilities' tools are skills: {skills:?}"
    );
    assert!(
        skills
            .iter()
            .any(|s| s["id"] == "skill:fin-equity-valuation"
                && s["tags"].as_array().unwrap().contains(&json!("finance"))),
        "the agent's skills are A2A skills: {skills:?}"
    );
    let r = app
        .request(
            Method::GET,
            &format!("/.well-known/agent.json?agent={encoded}"),
            None,
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["name"], "Financial Analyst");
    // By id too.
    let r = app
        .request(Method::GET, &format!("/a2a/agents/{id}/card"), None, None)
        .await;
    assert_eq!(r.status, StatusCode::OK);

    // An external agent cannot be exposed: it runs elsewhere.
    let (_, external) = call(
        &app,
        Method::POST,
        "/agents",
        &owner,
        Some(json!({"name": "Courier", "role": "courier", "runtime": "external"})),
    )
    .await;
    let (status, body) = call(
        &app,
        Method::PATCH,
        &format!("/agents/{}", external["id"].as_str().unwrap()),
        &owner,
        Some(json!({"a2a_exposed": true})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // Withdrawn: gone again.
    call(
        &app,
        Method::PATCH,
        &format!("/agents/{id}"),
        &owner,
        Some(json!({"a2a_exposed": false})),
    )
    .await;
    let r = app
        .request(
            Method::GET,
            &format!("/a2a/agents/{encoded}/card"),
            None,
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_counterpart_sends_a_task_and_gets_the_artifacts_back(pool: PgPool) {
    let (url, _seen) = fake_grpc_runtime().await;
    let app = grpc_app(pool, &url).await;
    let (card_url, _) = a2a_stub(true).await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    let analyst = exposed_analyst(&app, &owner, &["stub"]).await;
    let principal = analyst["principal_id"].as_str().unwrap();
    let (stub, secret) = counterpart(&app, &owner, &wid, "stub", &card_url).await;

    // A task whose first line asks for a document: one run, the .docx back as an artifact.
    let task_id = Uuid::now_v7();
    let (status, reply) = rpc(
        &app,
        principal,
        Some(&secret),
        "tasks/send",
        json!({
            "id": task_id.to_string(),
            "sessionId": "session-1",
            "message": {"role": "user", "parts": [
                {"type": "text", "text": "DOCX quarterly report\nWrite the quarterly report as a Word document."},
                {"type": "data", "data": {"quarter": "Q3 2026"}}
            ]},
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["jsonrpc"], "2.0");
    assert_eq!(reply["id"], 7);
    let task = &reply["result"];
    assert_eq!(task["id"], task_id.to_string());
    assert_eq!(task["sessionId"], "session-1");
    assert_eq!(task["status"]["state"], "completed", "{task}");
    assert_eq!(task["metadata"]["nexc.agent"], principal);
    assert_eq!(task["metadata"]["nexc.hop"], 0);
    let artifacts = task["artifacts"].as_array().unwrap();
    assert_eq!(artifacts[0]["name"], "answer");
    assert!(
        artifacts[0]["parts"][0]["text"]
            .as_str()
            .unwrap()
            .contains("wrote report.docx"),
        "{artifacts:?}"
    );
    let docx = artifacts
        .iter()
        .find(|a| a["name"] == "report.docx")
        .expect("the .docx is an artifact");
    let file = &docx["parts"][0]["file"];
    assert_eq!(
        file["mimeType"],
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    );
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(file["bytes"].as_str().unwrap())
        .unwrap();
    assert_eq!(bytes, b"PK demo");
    assert_eq!(
        task["history"][0]["parts"][0]["text"]
            .as_str()
            .unwrap()
            .lines()
            .next(),
        Some("DOCX quarterly report")
    );
    // The same task sent again is the same task.
    let (status, again) = rpc(
        &app,
        principal,
        Some(&secret),
        "tasks/send",
        json!({"id": task_id.to_string(), "message": text_message("again")}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["result"]["id"], task_id.to_string());
    assert_eq!(again["result"]["status"]["state"], "completed");

    // The run: one node of the Analyst in its A2A graph; the trace opens with the inbound
    // call and the decision that admitted it, then the agent's own spans.
    let run_id = task["metadata"]["nexc.run_id"].as_str().unwrap();
    let (status, run) = call(&app, Method::GET, &format!("/runs/{run_id}"), &owner, None).await;
    assert_eq!(status, StatusCode::OK, "{run}");
    assert_eq!(run["status"], "succeeded");
    assert_eq!(run["node_runs"].as_array().unwrap().len(), 1);
    let spans = trace_of(&app, &owner, run_id).await;
    assert_eq!(spans[0]["kind"], "tool_call");
    assert_eq!(spans[0]["name"], "a2a:stub");
    assert_eq!(spans[0]["principal"], principal);
    assert_eq!(spans[0]["attributes"]["inbound"], true);
    assert_eq!(spans[0]["attributes"]["counterpart"], stub["principal_id"]);
    assert_eq!(spans[0]["attributes"]["task_id"], task_id.to_string());
    assert_eq!(spans[0]["attributes"]["parts"], 2);
    assert_eq!(spans[1]["kind"], "policy");
    assert_eq!(spans[1]["parent_id"], spans[0]["id"]);
    assert_eq!(spans[1]["attributes"]["decision"], "allow");
    assert_eq!(spans[1]["attributes"]["rule"], "counterparts");
    assert_eq!(spans[1]["attributes"]["firewall"], true);
    assert_eq!(spans[1]["attributes"]["action"], "counterpart");
    assert!(
        spans
            .iter()
            .any(|s| s["kind"] == "tool_call" && s["name"] == "make_docx"),
        "the agent's own spans follow"
    );
    let (_, verified) = call(
        &app,
        Method::GET,
        &format!("/runs/{run_id}/trace/verify"),
        &owner,
        None,
    )
    .await;
    assert_eq!(verified["ok"], true, "{verified}");
    let (_, graph) = call(
        &app,
        Method::GET,
        &format!("/graphs/{}", run["graph_id"].as_str().unwrap()),
        &owner,
        None,
    )
    .await;
    assert_eq!(graph["name"], "A2A: Financial Analyst");
    assert_eq!(graph["nodes"][0]["title"], "DOCX quarterly report");
    assert_eq!(graph["nodes"][0]["agent_role"], "analyst");

    // The console lists the task on the agent.
    let (status, tasks) = call(
        &app,
        Method::GET,
        &format!("/agents/{}/a2a/tasks", analyst["id"].as_str().unwrap()),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{tasks}");
    assert_eq!(tasks[0]["id"], task_id.to_string());
    assert_eq!(tasks[0]["caller_principal"], stub["principal_id"]);
    assert_eq!(tasks[0]["state"], "completed");
    assert_eq!(tasks[0]["title"], "DOCX quarterly report");
    assert_eq!(tasks[0]["run_id"], run_id);
    assert!(tasks[0]["refused_rule"].is_null());

    // The trace explorer narrows to inbound tasks (and refuses a kind it does not know).
    let (status, page) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/secops/spans?kind=inbound"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let listed = page["spans"].as_array().unwrap();
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0]["id"], spans[0]["id"]);
    assert_eq!(listed[0]["inbound"], true);
    assert_eq!(listed[0]["kind"], "tool_call");
    let (status, page) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/secops/spans?kind=tool_call"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(page["spans"].as_array().unwrap().len() > 1);
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/secops/spans?kind=teleport"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // tasks/get says the same; the artifact is also fetched by URI with the token.
    let (status, got) = rpc(
        &app,
        principal,
        Some(&secret),
        "tasks/get",
        json!({"id": task_id.to_string(), "historyLength": 0}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(got["result"]["status"]["state"], "completed");
    assert_eq!(got["result"]["history"].as_array().unwrap().len(), 0);
    let artifact_id = docx["metadata"]["nexc.artifact_id"].as_str().unwrap();
    let r = app
        .request(
            Method::GET,
            &format!("/a2a/tasks/{task_id}/artifacts/{artifact_id}"),
            Some(&secret),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body, "PK demo");
    assert!(
        r.headers["content-disposition"]
            .to_str()
            .unwrap()
            .contains("report.docx")
    );

    // Unknown task, unknown method, broken request: JSON-RPC errors, HTTP 200.
    let (status, missing) = rpc(
        &app,
        principal,
        Some(&secret),
        "tasks/get",
        json!({"id": Uuid::now_v7().to_string()}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(missing["error"]["code"], -32001);
    let (_, unknown) = rpc(&app, principal, Some(&secret), "tasks/fly", json!({})).await;
    assert_eq!(unknown["error"]["code"], -32601);
    let (_, bad) = rpc(
        &app,
        principal,
        Some(&secret),
        "tasks/send",
        json!({"message": {"parts": []}}),
    )
    .await;
    assert_eq!(bad["error"]["code"], -32602);
    let (_, push) = rpc(
        &app,
        principal,
        Some(&secret),
        "tasks/pushNotification/set",
        json!({}),
    )
    .await;
    assert_eq!(push["error"]["code"], -32003);
    let r = app
        .request_with(
            Method::POST,
            &format!("/a2a/agents/{}", percent_encode(principal)),
            Some(&secret),
            None,
            &[("content-type", "application/json")],
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["error"]["code"], -32700);
    let r = app
        .request(
            Method::POST,
            &format!("/a2a/agents/{}", percent_encode(principal)),
            Some(&secret),
            Some(json!({"jsonrpc": "1.0", "method": "tasks/get"})),
        )
        .await;
    assert_eq!(r.body["error"]["code"], -32600);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn streaming_cancelling_and_files(pool: PgPool) {
    let (url, _seen) = fake_grpc_runtime().await;
    let app = grpc_app(pool, &url).await;
    let (card_url, _) = a2a_stub(true).await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    let analyst = exposed_analyst(&app, &owner, &["stub"]).await;
    let principal = analyst["principal_id"].as_str().unwrap();
    let (_, secret) = counterpart(&app, &owner, &wid, "stub", &card_url).await;

    // tasks/sendSubscribe: status updates and artifacts as events, a final one at the end.
    let task_id = Uuid::now_v7();
    let r = app
        .request(
            Method::POST,
            &format!("/a2a/agents/{}", percent_encode(principal)),
            Some(&secret),
            Some(json!({"jsonrpc": "2.0", "id": "s", "method": "tasks/sendSubscribe", "params": {
                "id": task_id.to_string(),
                "message": {"role": "user", "parts": [
                    {"type": "text", "text": "DOCX streamed report"},
                    {"type": "file", "file": {"name": "figures.csv", "mimeType": "text/csv",
                        "bytes": base64::engine::general_purpose::STANDARD.encode("quarter,revenue\nQ3,12.4\n")}}
                ]},
            }})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(
        r.headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    let body = r.body.as_str().unwrap().to_owned();
    let events: Vec<Value> = body
        .lines()
        .filter_map(|l| l.strip_prefix("data:"))
        .map(|d| serde_json::from_str(d.trim()).unwrap())
        .collect();
    assert!(events.len() >= 3, "{events:?}");
    assert!(
        events
            .iter()
            .all(|e| e["id"] == "s" && e["jsonrpc"] == "2.0")
    );
    assert_eq!(events[0]["result"]["status"]["state"], "submitted");
    assert!(
        events
            .iter()
            .any(|e| e["result"]["status"]["state"] == "working")
    );
    assert!(
        events
            .iter()
            .any(|e| e["result"]["artifact"]["name"] == "report.docx")
    );
    assert!(
        events
            .iter()
            .any(|e| e["result"]["artifact"]["name"] == "answer")
    );
    let last = events.last().unwrap();
    assert_eq!(last["result"]["final"], true);
    assert_eq!(last["result"]["status"]["state"], "completed");

    // The file became a document of the workspace, and the node was told about it.
    let (_, documents) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/documents"),
        &owner,
        None,
    )
    .await;
    let document = documents
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == "figures.csv")
        .expect("the file is a document");
    assert_eq!(document["label"], "internal");
    let (_, got) = rpc(
        &app,
        principal,
        Some(&secret),
        "tasks/get",
        json!({"id": task_id.to_string()}),
    )
    .await;
    assert_eq!(
        got["result"]["history"][0]["parts"][1]["file"]["documentId"],
        document["id"]
    );
    assert!(got["result"]["history"][0]["parts"][1]["file"]["bytes"].is_null());
    let run_id = got["result"]["metadata"]["nexc.run_id"].as_str().unwrap();
    let (_, run) = call(&app, Method::GET, &format!("/runs/{run_id}"), &owner, None).await;
    let (_, graph) = call(
        &app,
        Method::GET,
        &format!("/graphs/{}", run["graph_id"].as_str().unwrap()),
        &owner,
        None,
    )
    .await;
    let node = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["title"] == "DOCX streamed report")
        .unwrap();
    assert!(node["content"].as_str().unwrap().contains(&format!(
        "Attached: figures.csv (text/csv), in the knowledge base as document {}",
        document["id"].as_str().unwrap()
    )));
    let spans = trace_of(&app, &owner, run_id).await;
    assert_eq!(spans[0]["attributes"]["files"], 1);
    assert_eq!(spans[0]["attributes"]["document_ids"][0], document["id"]);

    // A slow task is cancelled while its stream is open; a finished one cannot be.
    let slow = Uuid::now_v7();
    let router = app.router.clone();
    let (path, bearer) = (
        format!("/a2a/agents/{}", percent_encode(principal)),
        format!("Bearer {secret}"),
    );
    let streaming = tokio::spawn(async move {
        let request = axum::http::Request::builder()
            .method(Method::POST)
            .uri(path)
            .header("authorization", bearer)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                json!({"jsonrpc": "2.0", "id": 1, "method": "tasks/sendSubscribe", "params": {
                    "id": slow.to_string(), "message": text_message("SLOW task")}})
                .to_string(),
            ))
            .unwrap();
        let response = tower::ServiceExt::oneshot(router, request).await.unwrap();
        let status = response.status();
        let bytes = http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    });
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let (_, got) = rpc(
            &app,
            principal,
            Some(&secret),
            "tasks/get",
            json!({"id": slow.to_string()}),
        )
        .await;
        if got["result"]["status"]["state"] == "working" {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the slow task never started: {got}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let (status, cancelled) = rpc(
        &app,
        principal,
        Some(&secret),
        "tasks/cancel",
        json!({"id": slow.to_string()}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cancelled}");
    assert_eq!(
        cancelled["result"]["status"]["state"], "canceled",
        "{cancelled}"
    );
    let (status, body) = tokio::time::timeout(std::time::Duration::from_secs(30), streaming)
        .await
        .expect("the stream ends once the run is cancelled")
        .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"canceled\""), "{body}");
    assert!(body.contains("\"final\":true"), "{body}");
    let (_, again) = rpc(
        &app,
        principal,
        Some(&secret),
        "tasks/cancel",
        json!({"id": task_id.to_string()}),
    )
    .await;
    assert_eq!(again["error"]["code"], -32002);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn wrong_tokens_and_foreign_callers_are_refused(pool: PgPool) {
    let app = a2a_app(pool).await;
    let (card_url, _) = a2a_stub(true).await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    let analyst = exposed_analyst(&app, &owner, &["stub"]).await;
    let principal = analyst["principal_id"].as_str().unwrap();
    let (stub, secret) = counterpart(&app, &owner, &wid, "stub", &card_url).await;
    let send = json!({"message": text_message("hello")});

    // No token, a token of the wrong shape, a user's JWT: 401 as JSON-RPC errors.
    let (status, body) = rpc(&app, principal, None, "tasks/send", send.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], -32010);
    assert_eq!(body["jsonrpc"], "2.0");
    let (status, body) = rpc(&app, principal, Some("garbage"), "tasks/send", send.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], -32010);
    let (status, _) = rpc(&app, principal, Some(&owner), "tasks/send", send.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = rpc(
        &app,
        principal,
        Some("nexc_agt_notissuedbyanyone0123456789abcdefghijkl"),
        "tasks/send",
        send.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // A token of an external agent that is no counterpart: 403.
    let (_, courier) = call(
        &app,
        Method::POST,
        "/agents",
        &owner,
        Some(json!({"name": "Courier", "role": "courier", "runtime": "external"})),
    )
    .await;
    let (_, issued) = call(
        &app,
        Method::POST,
        &format!("/agents/{}/tokens", courier["id"].as_str().unwrap()),
        &owner,
        Some(json!({})),
    )
    .await;
    let (status, body) = rpc(
        &app,
        principal,
        Some(issued["secret"].as_str().unwrap()),
        "tasks/send",
        send.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"]["code"], -32011);

    // A counterpart of another workspace: the agent is not found for it.
    let (other, _) = app.register("other@example.com").await;
    let other_wid = workspace(&app, &other).await;
    let (_, foreign) = counterpart(&app, &other, &other_wid, "stub", &card_url).await;
    let (status, body) = rpc(&app, principal, Some(&foreign), "tasks/send", send.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"]["code"], -32012);

    // A quarantined counterpart: 403. A revoked token: 401.
    let stub_id = stub["id"].as_str().unwrap();
    call(
        &app,
        Method::POST,
        &format!("/agents/{stub_id}/quarantine"),
        &owner,
        None,
    )
    .await;
    let (status, body) = rpc(
        &app,
        principal,
        Some(&secret),
        "tasks/get",
        json!({"id": Uuid::now_v7().to_string()}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    call(
        &app,
        Method::POST,
        &format!("/agents/{stub_id}/release"),
        &owner,
        None,
    )
    .await;
    let (_, tokens) = call(
        &app,
        Method::GET,
        &format!("/agents/{stub_id}/tokens"),
        &owner,
        None,
    )
    .await;
    call(
        &app,
        Method::DELETE,
        &format!(
            "/agents/{stub_id}/tokens/{}",
            tokens[0]["id"].as_str().unwrap()
        ),
        &owner,
        None,
    )
    .await;
    let (status, _) = rpc(
        &app,
        principal,
        Some(&secret),
        "tasks/get",
        json!({"id": Uuid::now_v7().to_string()}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // An agent that is not exposed, or does not exist: 404 for a valid caller.
    let (_, fresh) = counterpart(&app, &owner, &wid, "fresh", &card_url).await;
    let (_, hidden) = call(
        &app,
        Method::POST,
        "/agents",
        &owner,
        Some(json!({"name": "Hidden", "role": "hidden"})),
    )
    .await;
    let (status, body) = rpc(
        &app,
        hidden["principal_id"].as_str().unwrap(),
        Some(&fresh),
        "tasks/get",
        json!({"id": Uuid::now_v7().to_string()}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, _) = rpc(
        &app,
        "agent://nope/Nobody",
        Some(&fresh),
        "tasks/get",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A counterpart not on the agent's list: the task is on the record, refused by the firewall.
    let (status, body) = rpc(
        &app,
        principal,
        Some(&fresh),
        "tasks/send",
        json!({"message": text_message("let me in")}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let task = &body["result"];
    assert_eq!(task["status"]["state"], "failed");
    assert_eq!(task["metadata"]["nexc.refused_rule"], "counterparts");
    assert!(
        task["status"]["message"]["parts"][0]["text"]
            .as_str()
            .unwrap()
            .contains("not in the allow-list")
    );
    let run_id = task["metadata"]["nexc.run_id"].as_str().unwrap();
    let spans = trace_of(&app, &owner, run_id).await;
    assert_eq!(spans.len(), 2, "{spans:?}");
    assert_eq!(spans[0]["kind"], "tool_call");
    assert_eq!(spans[0]["status"], "error");
    assert_eq!(spans[1]["kind"], "policy");
    assert_eq!(spans[1]["attributes"]["decision"], "deny");
    assert_eq!(spans[1]["attributes"]["rule"], "counterparts");
    let (_, run) = call(&app, Method::GET, &format!("/runs/{run_id}"), &owner, None).await;
    assert_eq!(run["status"], "failed");
    assert!(
        run["node_runs"][0]["error"]
            .as_str()
            .unwrap()
            .contains("counterparts")
    );
    let (_, tasks) = call(
        &app,
        Method::GET,
        &format!("/agents/{}/a2a/tasks", analyst["id"].as_str().unwrap()),
        &owner,
        None,
    )
    .await;
    assert_eq!(tasks[0]["refused_rule"], "counterparts");
    assert_eq!(tasks[0]["state"], "failed");
    // Members read the list; outsiders and guests do not.
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/agents/{}/a2a/tasks", analyst["id"].as_str().unwrap()),
        &other,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// An Analyst from the template that may call `counterparts` on this machine as `a2a:<name>`.
async fn calling_analyst(app: &TestApp, token: &str, counterparts: &[&str], depth: u8) -> Value {
    let (status, card) = call(
        app,
        Method::POST,
        "/agents/from-template/analyst",
        token,
        Some(json!({"evaluate": false})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{card}");
    let id = card["agent"]["id"].as_str().unwrap().to_owned();
    let mut capabilities = card["agent"]["capabilities"].clone();
    let mut tools: Vec<Value> = capabilities["tools"].as_array().unwrap().clone();
    tools.extend(counterparts.iter().map(|c| json!(format!("a2a:{c}"))));
    capabilities["tools"] = json!(tools);
    capabilities["counterparts"] = json!(counterparts);
    capabilities["egress_hosts"] = json!(["127.0.0.1"]);
    capabilities["delegation"] = json!({"max_depth": depth, "max_fan_out": 2});
    let (status, agent) = call(
        app,
        Method::PATCH,
        &format!("/agents/{id}"),
        token,
        Some(json!({"capabilities": capabilities, "a2a_exposed": true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{agent}");
    agent
}

/// Runs a graph of one `TOOL <tool>` node of `role` with `arguments`; returns the run id,
/// the final run and the spans.
async fn tool_run(
    app: &TestApp,
    token: &str,
    role: &str,
    tool: &str,
    arguments: Value,
) -> (String, Value, Vec<Value>) {
    let gid = app.graph(token, "Ask the partner").await;
    app.node(
        token,
        &gid,
        json!({"title": format!("TOOL {tool}"), "kind": "task", "executor": "agent", "agent_role": role,
            "content": arguments.to_string()}),
    )
    .await;
    let mut rx = app.events(&gid);
    let (status, body) = call(
        app,
        Method::POST,
        &format!("/graphs/{gid}/runs"),
        token,
        Some(json!({"force": true})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let rid = body["id"].as_str().unwrap().to_owned();
    let events = collect_until(&mut rx, "run.finished").await;
    let finished = events.last().unwrap().1["run"].clone();
    let spans = trace_of(app, token, &rid).await;
    (rid, finished, spans)
}

/// The gateway's answer to the last `TOOL` request of the runtime stand-in.
fn last_answer(seen: &common::grpc_runtime::Seen) -> (String, bool) {
    let answers = seen.tool_results.lock().unwrap();
    let (_, output, is_error) = answers.last().cloned().expect("the gateway answered");
    (output, is_error)
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn admins_register_counterparts_and_their_cards_are_fetched(pool: PgPool) {
    let app = a2a_app(pool).await;
    let (card_url, stub_seen) = a2a_stub(true).await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    let (member, _) = app.register("member@example.com").await;
    call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/members"),
        &owner,
        Some(json!({"email": "member@example.com", "role": "member"})),
    )
    .await;
    let (outsider, _) = app.register("outsider@example.com").await;

    // Registering fetches the card at once, makes the principal and issues its first token.
    let (status, registered) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/a2a-counterparts"),
        &owner,
        Some(
            json!({"name": "partner", "card_url": card_url, "auth_header": "Bearer remote-secret",
            "max_label": "internal"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{registered}");
    let counterpart = &registered["counterpart"];
    assert_eq!(counterpart["name"], "partner");
    assert_eq!(counterpart["host"], "127.0.0.1");
    assert_eq!(counterpart["has_auth_header"], true);
    assert_eq!(counterpart["last_check_ok"], true, "{counterpart}");
    assert_eq!(counterpart["card"]["name"], "Remote Stub");
    assert_eq!(counterpart["card"]["capabilities"]["streaming"], true);
    assert_eq!(
        counterpart["principal_id"],
        format!("agent://{wid}/a2a:partner")
    );
    assert!(
        registered["token"]["secret"]
            .as_str()
            .unwrap()
            .starts_with("nexc_agt_")
    );
    assert_eq!(
        stub_seen.lock().unwrap().auth.len(),
        0,
        "the card is fetched without a task"
    );
    let cid = counterpart["id"].as_str().unwrap().to_owned();
    // The principal is an external agent of the workspace, with the token on it.
    let (_, agent) = call(
        &app,
        Method::GET,
        &format!("/agents/{}", counterpart["agent_id"].as_str().unwrap()),
        &owner,
        None,
    )
    .await;
    assert_eq!(agent["agent"]["runtime"], "external");
    assert_eq!(agent["agent"]["name"], "a2a:partner");
    let (_, tokens) = call(
        &app,
        Method::GET,
        &format!(
            "/agents/{}/tokens",
            counterpart["agent_id"].as_str().unwrap()
        ),
        &owner,
        None,
    )
    .await;
    assert_eq!(tokens.as_array().unwrap().len(), 1);
    assert_eq!(tokens[0]["name"], "a2a");

    // Listed to members, not to outsiders; registered by admins only.
    let (status, list) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/a2a-counterparts"),
        &member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list[0]["id"], cid);
    assert!(list[0].get("auth_header").is_none());
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/a2a-counterparts"),
        &outsider,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/a2a-counterparts"),
        &member,
        Some(json!({"name": "other", "card_url": card_url})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, one) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/a2a-counterparts/{cid}"),
        &member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(one["name"], "partner");

    // The name is taken; the input is checked; a card that does not answer is still registered.
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/a2a-counterparts"),
        &owner,
        Some(json!({"name": "partner", "card_url": card_url})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/a2a-counterparts"),
        &owner,
        Some(
            json!({"name": "Bad Name", "card_url": "ftp://x", "allowed_agents": [Uuid::now_v7()]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["name"].is_array() && body["errors"]["card_url"].is_array());
    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/a2a-counterparts"),
        &owner,
        Some(
            json!({"name": "ghost", "card_url": "http://127.0.0.1:9/.well-known/agent.json",
            "allowed_agents": [Uuid::now_v7()]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["allowed_agents"].is_array());
    let (status, ghost) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/a2a-counterparts"),
        &owner,
        Some(json!({"name": "ghost", "card_url": "http://127.0.0.1:9/.well-known/agent.json"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{ghost}");
    assert_eq!(ghost["counterpart"]["last_check_ok"], false);
    assert_eq!(ghost["counterpart"]["card"], json!({}));
    let (status, check) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/a2a-counterparts/{cid}/check"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{check}");
    assert_eq!(check["ok"], true);
    assert!(check["detail"].as_str().unwrap().contains("streams"));

    // Audited, removed with its principal, and the token is refused from then on.
    let (_, audit) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/audit"),
        &owner,
        None,
    )
    .await;
    assert!(
        audit
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["action"] == "a2a_counterpart_added" && e["subject"] == "a2a:partner")
    );
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/workspaces/{wid}/a2a-counterparts/{cid}"),
        &member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/workspaces/{wid}/a2a-counterparts/{cid}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/agents/{}", counterpart["agent_id"].as_str().unwrap()),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the principal went with it");
    let analyst = exposed_analyst(&app, &owner, &["partner"]).await;
    let (status, _) = rpc(
        &app,
        analyst["principal_id"].as_str().unwrap(),
        Some(registered["token"]["secret"].as_str().unwrap()),
        "tasks/get",
        json!({"id": Uuid::now_v7().to_string()}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_private_counterpart_host_is_refused_unless_allowed(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/a2a-counterparts"),
        &owner,
        Some(json!({"name": "local", "card_url": "http://127.0.0.1:8099/.well-known/agent.json"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["errors"]["card_url"][0]
            .as_str()
            .unwrap()
            .contains("NEXC_A2A_ALLOW_PRIVATE_HOSTS")
    );
    // The workspace's own egress deny-list is honoured too.
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/policies"),
        &owner,
        Some(json!({"scope": "workspace", "scope_id": null,
            "document": {"egress": {"deny": ["*.evil.example"]}}})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/a2a-counterparts"),
        &owner,
        Some(
            json!({"name": "evil", "card_url": "https://a2a.evil.example/.well-known/agent.json"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["errors"]["card_url"][0]
            .as_str()
            .unwrap()
            .contains("egress.deny")
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_agent_calls_a_counterpart_through_the_gateway(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let app = grpc_app(pool, &url).await;
    let (card_url, stub_seen) = a2a_stub(true).await;
    let (poll_url, poll_seen) = a2a_stub(false).await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    let analyst = calling_analyst(&app, &owner, &["stub", "poller"], 1).await;
    let principal = analyst["principal_id"].as_str().unwrap();
    let (stub, _) = counterpart(&app, &owner, &wid, "stub", &card_url).await;
    counterpart(&app, &owner, &wid, "poller", &poll_url).await;

    // Streamed: the remote's events are read, its text comes back, its file is kept.
    let (rid, run, spans) = tool_run(
        &app,
        &owner,
        "analyst",
        "a2a:stub",
        json!({"message": "Summarise the quarter for the board"}),
    )
    .await;
    assert_eq!(run["status"], "succeeded", "{run}");
    let request = seen.requests.lock().unwrap().last().unwrap().clone();
    let offered: Vec<&str> = request["gateway_tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(
        offered.contains(&"a2a:stub") && offered.contains(&"a2a:poller"),
        "{offered:?}"
    );
    let (answer, is_error) = last_answer(&seen);
    assert!(!is_error, "{answer}");
    assert!(
        answer.contains("Remote says: Summarise the quarter for the board"),
        "{answer}"
    );
    assert!(
        answer.contains("kept as artifact a2a/stub/figures.csv"),
        "{answer}"
    );
    {
        let remote = stub_seen.lock().unwrap();
        assert_eq!(remote.methods, ["tasks/sendSubscribe"]);
        assert_eq!(remote.auth[0], "Bearer remote-secret");
        let params = &remote.params[0];
        assert_eq!(
            params["message"]["parts"][0]["text"],
            "Summarise the quarter for the board"
        );
        assert_eq!(params["metadata"]["nexc.hop"], 1);
        assert_eq!(params["metadata"]["nexc.chain"], json!([principal]));
        assert_eq!(params["metadata"]["nexc.principal"], principal);
        assert_eq!(
            params["id"]
                .as_str()
                .unwrap()
                .parse::<Uuid>()
                .unwrap()
                .get_version_num(),
            7
        );
    }
    let (_, artifacts) = call(
        &app,
        Method::GET,
        &format!("/runs/{rid}/artifacts"),
        &owner,
        None,
    )
    .await;
    assert!(
        artifacts
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["path"] == "a2a/stub/figures.csv" && a["mime"] == "text/csv"),
        "{artifacts}"
    );
    let tool = spans
        .iter()
        .find(|s| s["kind"] == "tool_call" && s["name"] == "a2a:stub")
        .expect("the tool call is on the trace");
    assert_eq!(tool["attributes"]["gateway"], true);
    assert_eq!(tool["attributes"]["a2a"], true);
    assert_eq!(tool["attributes"]["direction"], "outbound");
    assert_eq!(tool["attributes"]["counterpart"], stub["principal_id"]);
    assert_eq!(tool["attributes"]["hop"], 1);
    assert_eq!(tool["attributes"]["chain"], json!([principal]));
    assert_eq!(tool["attributes"]["remote_state"], "completed");
    assert_eq!(tool["attributes"]["streamed"], true);
    assert_eq!(tool["attributes"]["artifacts"], 1);
    assert_eq!(tool["attributes"]["egress_host"], "127.0.0.1");
    let remote_task = tool["attributes"]["remote_task_id"].as_str().unwrap();
    assert_eq!(remote_task, stub_seen.lock().unwrap().params[0]["id"]);
    let egress = spans
        .iter()
        .find(|s| s["kind"] == "egress" && s["parent_id"] == tool["id"])
        .expect("the egress span hangs under the tool call");
    assert_eq!(egress["name"], "127.0.0.1");
    assert_eq!(egress["attributes"]["transport"], "a2a");
    assert_eq!(egress["attributes"]["status"], 200);
    assert!(egress["attributes"]["bytes_in"].as_u64().unwrap() > 0);
    assert!(
        egress["attributes"]["url"]
            .as_str()
            .unwrap()
            .ends_with("/rpc")
    );

    // Not streamed: sent, then polled until it ended.
    let (_, run, spans) = tool_run(
        &app,
        &owner,
        "analyst",
        "a2a:poller",
        json!({"message": "Poll me", "session_id": "s-9"}),
    )
    .await;
    assert_eq!(run["status"], "succeeded", "{run}");
    let (answer, is_error) = last_answer(&seen);
    assert!(
        !is_error && answer.contains("Remote says: Poll me"),
        "{answer}"
    );
    {
        let remote = poll_seen.lock().unwrap();
        assert_eq!(remote.methods[0], "tasks/send");
        assert!(
            remote.methods[1..].iter().all(|m| m == "tasks/get"),
            "{:?}",
            remote.methods
        );
        assert_eq!(remote.params[0]["sessionId"], "s-9");
    }
    let tool = spans
        .iter()
        .find(|s| s["kind"] == "tool_call" && s["name"] == "a2a:poller")
        .unwrap();
    assert_eq!(tool["attributes"]["streamed"], false);
    assert_eq!(tool["attributes"]["remote_state"], "completed");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_gateway_refuses_counterparts_hosts_and_labels_the_policy_forbids(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let app = grpc_app(pool, &url).await;
    let (card_url, stub_seen) = a2a_stub(true).await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    counterpart(&app, &owner, &wid, "stub", &card_url).await;
    counterpart(&app, &owner, &wid, "other", &card_url).await;

    // The counterpart list: `other` is not among the agent's counterparts.
    let analyst = calling_analyst(&app, &owner, &["stub"], 1).await;
    let (_, agent) = call(
        &app,
        Method::PATCH,
        &format!("/agents/{}", analyst["id"].as_str().unwrap()),
        &owner,
        Some(json!({"capabilities": {
            "tools": ["read_file", "write_file", "make_docx", "finish", "a2a:stub", "a2a:other"],
            "egress_hosts": ["127.0.0.1"], "counterparts": ["stub"],
            "delegation": {"max_depth": 1, "max_fan_out": 2}}})),
    )
    .await;
    assert_eq!(agent["capabilities"]["counterparts"], json!(["stub"]));
    let (_, run, spans) = tool_run(
        &app,
        &owner,
        "analyst",
        "a2a:other",
        json!({"message": "hi"}),
    )
    .await;
    assert_eq!(run["status"], "succeeded");
    let (answer, is_error) = last_answer(&seen);
    assert!(is_error && answer.contains("counterparts"), "{answer}");
    let refusal = spans
        .iter()
        .find(|s| s["kind"] == "policy" && s["attributes"]["rule"] == "counterparts")
        .expect("a policy span names the rule");
    assert_eq!(refusal["attributes"]["decision"], "deny");
    assert_eq!(refusal["attributes"]["action"], "counterpart");
    assert_eq!(refusal["name"], "other");
    assert_eq!(
        stub_seen.lock().unwrap().methods.len(),
        0,
        "nothing reached the remote"
    );

    // The host: a counterpart on this machine is reached only when the allow-list names it.
    call(
        &app,
        Method::PATCH,
        &format!("/agents/{}", analyst["id"].as_str().unwrap()),
        &owner,
        Some(json!({"capabilities": {
            "tools": ["read_file", "write_file", "make_docx", "finish", "a2a:stub", "a2a:other"],
            "egress_hosts": ["api.partner.example"], "counterparts": ["stub"],
            "delegation": {"max_depth": 1, "max_fan_out": 2}}})),
    )
    .await;
    let (_, _, spans) = tool_run(
        &app,
        &owner,
        "analyst",
        "a2a:stub",
        json!({"message": "hi"}),
    )
    .await;
    let (answer, is_error) = last_answer(&seen);
    assert!(is_error && answer.contains("egress.hosts"), "{answer}");
    assert!(
        spans
            .iter()
            .any(|s| s["kind"] == "policy" && s["attributes"]["rule"] == "egress.hosts"),
    );
    assert_eq!(stub_seen.lock().unwrap().methods.len(), 0);

    // The labels: a node that read restricted data may not send a word of it out.
    tokio::fs::create_dir_all(app.state.settings.documents_dir())
        .await
        .unwrap();
    let r = app
        .send_bytes(
            &format!("/api/v1/workspaces/{wid}/documents?name=board.md"),
            &owner,
            b"# Board\n\nThe board's compensation for the quarter is in this document.\n".to_vec(),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let did = r.body["id"].as_str().unwrap().to_owned();
    nexc::engine::knowledge::work(&app.state).await.unwrap();
    let (status, _) = call(
        &app,
        Method::PATCH,
        &format!("/workspaces/{wid}/documents/{did}"),
        &owner,
        Some(json!({"label": "restricted"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    call(
        &app,
        Method::PATCH,
        &format!("/agents/{}", analyst["id"].as_str().unwrap()),
        &owner,
        Some(json!({"capabilities": {
            "tools": ["read_file", "write_file", "make_docx", "finish", "a2a:stub"],
            "data": {"labels": ["public", "internal", "confidential", "restricted"]},
            "egress_hosts": ["127.0.0.1"], "counterparts": ["stub"],
            "delegation": {"max_depth": 1, "max_fan_out": 2}}})),
    )
    .await;
    let gid = app
        .graph(&owner, "The board's compensation for the quarter")
        .await;
    app.node(
        &owner,
        &gid,
        json!({"title": "TOOL a2a:stub", "kind": "task", "executor": "agent", "agent_role": "analyst",
            "content": json!({"message": "What is the board's compensation for the quarter?"}).to_string()}),
    )
    .await;
    let mut rx = app.events(&gid);
    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/graphs/{gid}/runs"),
        &owner,
        Some(json!({"force": true})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let rid = body["id"].as_str().unwrap().to_owned();
    collect_until(&mut rx, "run.finished").await;
    let request = seen.requests.lock().unwrap().last().unwrap().clone();
    assert!(
        request["context"]["labels"]
            .as_array()
            .unwrap()
            .contains(&json!("restricted")),
        "{}",
        request["context"]["labels"]
    );
    let (answer, is_error) = last_answer(&seen);
    assert!(is_error && answer.contains("dlp.labels"), "{answer}");
    let spans = trace_of(&app, &owner, &rid).await;
    let dlp = spans
        .iter()
        .find(|s| s["kind"] == "policy" && s["attributes"]["rule"] == "dlp.labels")
        .expect("the DLP refusal is a policy span");
    assert_eq!(dlp["attributes"]["action"], "egress");
    assert_eq!(dlp["attributes"]["subject"], "127.0.0.1");
    assert_eq!(
        stub_seen.lock().unwrap().methods.len(),
        0,
        "the restricted message never left"
    );
    let (_, alerts) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/alerts"),
        &owner,
        None,
    )
    .await;
    assert!(
        alerts
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["kind"] == "dlp" && a["rule"] == "dlp.labels"),
        "{alerts}"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn delegation_depth_and_loops_are_bounded_across_hops(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let app = grpc_app(pool, &url).await;
    let (card_url, stub_seen) = a2a_stub(true).await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    // Exposed, may call `stub` back, and may go one hop deep.
    let analyst = calling_analyst(&app, &owner, &["stub"], 1).await;
    let principal = analyst["principal_id"].as_str().unwrap().to_owned();
    let (stub, secret) = counterpart(&app, &owner, &wid, "stub", &card_url).await;
    let stub_principal = stub["principal_id"].as_str().unwrap().to_owned();
    // The message's first line tells the runtime stand-in to call the counterpart back.
    let callback =
        |text: &str| text_message(&format!("TOOL a2a:stub\n{}", json!({"message": text})));

    // The stub sends a task; the Analyst calls the stub back as a tool: hop 1, the chain names
    // the Analyst, the egress span is on the inbound run's trace.
    let (status, reply) = rpc(
        &app,
        &principal,
        Some(&secret),
        "tasks/send",
        json!({"message": callback("call me back")}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let task = &reply["result"];
    assert_eq!(task["status"]["state"], "completed", "{task}");
    assert!(
        task["artifacts"][0]["parts"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Remote says: call me back"),
        "{task}"
    );
    {
        let remote = stub_seen.lock().unwrap();
        assert_eq!(remote.methods, ["tasks/sendSubscribe"]);
        assert_eq!(remote.params[0]["metadata"]["nexc.hop"], 1);
        assert_eq!(
            remote.params[0]["metadata"]["nexc.chain"],
            json!([principal])
        );
    }
    let run_id = task["metadata"]["nexc.run_id"].as_str().unwrap();
    let spans = trace_of(&app, &owner, run_id).await;
    assert_eq!(spans[0]["attributes"]["inbound"], true);
    assert_eq!(spans[0]["attributes"]["hop"], 0);
    let outbound = spans
        .iter()
        .find(|s| s["kind"] == "tool_call" && s["attributes"]["direction"] == "outbound")
        .expect("the callback is on the inbound run's trace");
    assert_eq!(outbound["name"], "a2a:stub");
    assert_eq!(outbound["attributes"]["hop"], 1);
    assert_eq!(outbound["attributes"]["chain"], json!([principal]));
    assert!(
        spans.iter().any(|s| s["kind"] == "egress"
            && s["parent_id"] == outbound["id"]
            && s["attributes"]["transport"] == "a2a"),
        "the egress span hangs under the callback"
    );

    // A loop: the task comes back to the Analyst with itself already in the chain.
    let (status, reply) = rpc(
        &app,
        &principal,
        Some(&secret),
        "tasks/send",
        json!({"message": text_message("again"),
            "metadata": {"nexc.hop": 1, "nexc.chain": [principal, stub_principal]}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let task = &reply["result"];
    assert_eq!(task["status"]["state"], "failed", "{task}");
    assert_eq!(task["metadata"]["nexc.refused_rule"], "delegation.loop");
    assert!(
        task["status"]["message"]["parts"][0]["text"]
            .as_str()
            .unwrap()
            .contains("loop")
    );
    let run_id = task["metadata"]["nexc.run_id"].as_str().unwrap();
    let spans = trace_of(&app, &owner, run_id).await;
    assert_eq!(spans.len(), 2, "{spans:?}");
    assert_eq!(spans[1]["kind"], "policy");
    assert_eq!(spans[1]["attributes"]["rule"], "delegation.loop");
    assert_eq!(spans[1]["attributes"]["decision"], "deny");
    assert_eq!(
        spans[1]["attributes"]["chain"],
        json!([principal, stub_principal])
    );
    assert_eq!(
        seen.requests.lock().unwrap().len(),
        1,
        "the agent never started"
    );

    // Too deep: a task that already passed two hops, for an agent allowed one.
    let (_, reply) = rpc(
        &app,
        &principal,
        Some(&secret),
        "tasks/send",
        json!({"message": text_message("deep"),
            "metadata": {"nexc.hop": 2, "nexc.chain": ["agent://other/X", "agent://other/Y"]}}),
    )
    .await;
    assert_eq!(reply["result"]["status"]["state"], "failed", "{reply}");
    assert_eq!(
        reply["result"]["metadata"]["nexc.refused_rule"],
        "delegation.max_depth"
    );
    let run_id = reply["result"]["metadata"]["nexc.run_id"].as_str().unwrap();
    let spans = trace_of(&app, &owner, run_id).await;
    assert_eq!(spans[1]["attributes"]["rule"], "delegation.max_depth");
    assert_eq!(spans[1]["attributes"]["hop"], 2);

    // One hop deep is admitted, but the callback would be the second: refused by the gateway
    // as a policy span, and nothing reaches the stub.
    let before = stub_seen.lock().unwrap().methods.len();
    let (_, reply) = rpc(
        &app,
        &principal,
        Some(&secret),
        "tasks/send",
        json!({"message": callback("one hop in"),
            "metadata": {"nexc.hop": 1, "nexc.chain": ["agent://other/X"]}}),
    )
    .await;
    let task = &reply["result"];
    assert_eq!(task["status"]["state"], "completed", "{task}");
    let run_id = task["metadata"]["nexc.run_id"].as_str().unwrap();
    let spans = trace_of(&app, &owner, run_id).await;
    let refusal = spans
        .iter()
        .find(|s| {
            s["kind"] == "policy"
                && s["attributes"]["rule"] == "delegation.max_depth"
                && s["attributes"]["action"] == "counterpart"
        })
        .expect("the gateway refused the second hop");
    assert_eq!(refusal["name"], "stub");
    assert_eq!(refusal["attributes"]["decision"], "deny");
    let (answer, is_error) = last_answer(&seen);
    assert!(
        is_error && answer.contains("delegation.max_depth"),
        "{answer}"
    );
    assert_eq!(stub_seen.lock().unwrap().methods.len(), before);
    assert_eq!(task["metadata"]["nexc.hop"], 1);

    // With two hops allowed, the callback goes out at hop 2 with the whole chain.
    call(
        &app,
        Method::PATCH,
        &format!("/agents/{}", analyst["id"].as_str().unwrap()),
        &owner,
        Some(json!({"capabilities": {
            "tools": ["read_file", "list_files", "write_file", "make_docx", "finish", "a2a:stub"],
            "egress_hosts": ["127.0.0.1"], "counterparts": ["stub"],
            "delegation": {"max_depth": 2, "max_fan_out": 2}}})),
    )
    .await;
    let (_, reply) = rpc(
        &app,
        &principal,
        Some(&secret),
        "tasks/send",
        json!({"message": callback("two hops allowed"),
            "metadata": {"nexc.hop": 1, "nexc.chain": ["agent://other/X"]}}),
    )
    .await;
    assert_eq!(reply["result"]["status"]["state"], "completed", "{reply}");
    {
        let remote = stub_seen.lock().unwrap();
        let last = remote.params.last().unwrap();
        assert_eq!(remote.methods.len(), before + 1);
        assert_eq!(last["metadata"]["nexc.hop"], 2);
        assert_eq!(
            last["metadata"]["nexc.chain"],
            json!(["agent://other/X", principal])
        );
    }
    let (_, tasks) = call(
        &app,
        Method::GET,
        &format!("/agents/{}/a2a/tasks", analyst["id"].as_str().unwrap()),
        &owner,
        None,
    )
    .await;
    let rules: Vec<Option<&str>> = tasks
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["refused_rule"].as_str())
        .collect();
    assert_eq!(
        rules,
        [
            None,
            None,
            Some("delegation.max_depth"),
            Some("delegation.loop"),
            None
        ]
    );
    assert_eq!(tasks[0]["hop"], 1);
    assert_eq!(tasks[0]["chain"], json!(["agent://other/X"]));
}
