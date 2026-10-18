//! The A2A adapter: an agent an admin exposes serves its card and takes
//! tasks over JSON-RPC from registered counterparts, each task a one-node
//! run of the agent with the caller and the firewall's decision on its
//! trace, its output and files answered as A2A artifacts; wrong, missing
//! and foreign tokens are refused as JSON-RPC errors with 401 and 403.
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode};
use base64::Engine as _;
use common::TestApp;
use common::grpc_runtime::fake_grpc_runtime;
use nexc::a2a::percent_encode;
use nexc::domain::capabilities::DataLabel;
use nexc::repo::a2a::NewCounterpart;
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

async fn workspace(app: &TestApp, token: &str) -> String {
    let (_, list) = call(app, Method::GET, "/workspaces", token, None).await;
    list[0]["id"].as_str().unwrap().to_owned()
}

async fn grpc_app(pool: PgPool, url: &str) -> TestApp {
    TestApp::new(
        pool,
        &[
            ("NEXC_RUNTIME_PROTOCOL", "grpc"),
            ("NEXC_RUNTIME_GRPC_URL", url),
            ("NEXC_PUBLIC_URL", "https://nexc.example"),
        ],
    )
    .await
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

/// Registers a counterpart with its principal and a token (the repo's way;
/// the registration route is the next step's). Returns the token.
async fn counterpart(app: &TestApp, token: &str, wid: &str, name: &str) -> (Value, String) {
    let (status, agent) = call(
        app,
        Method::POST,
        "/agents",
        token,
        Some(json!({
            "name": format!("a2a:{name}"), "role": "counterpart", "runtime": "external",
            "purpose": format!("A2A counterpart {name}."),
            "capabilities": {"tools": [], "egress_hosts": [], "counterparts": [],
                "delegation": {"max_depth": 0, "max_fan_out": 0}},
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{agent}");
    let agent_id: Uuid = agent["id"].as_str().unwrap().parse().unwrap();
    let (status, issued) = call(
        app,
        Method::POST,
        &format!("/agents/{agent_id}/tokens"),
        token,
        Some(json!({"name": "a2a"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{issued}");
    let (_, me) = call(app, Method::GET, "/auth/me", token, None).await;
    nexc::repo::a2a::insert_counterpart(
        &app.state.db,
        Uuid::now_v7(),
        &NewCounterpart {
            workspace_id: wid.parse().unwrap(),
            name,
            card_url: "https://remote.example/.well-known/agent.json",
            auth_header: None,
            allowed_agents: &[],
            max_label: DataLabel::Internal,
            agent_id,
            created_by: me["id"].as_str().unwrap().parse().unwrap(),
        },
    )
    .await
    .unwrap();
    (agent, issued["secret"].as_str().unwrap().to_owned())
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
    let app = TestApp::new(pool, &[("NEXC_PUBLIC_URL", "https://nexc.example")]).await;
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
    assert_eq!(card["name"], "Analyst");
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
    let r = app
        .request(
            Method::GET,
            &format!("/.well-known/agent.json?agent={encoded}"),
            None,
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["name"], "Analyst");
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
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    let analyst = exposed_analyst(&app, &owner, &["stub"]).await;
    let principal = analyst["principal_id"].as_str().unwrap();
    let (stub, secret) = counterpart(&app, &owner, &wid, "stub").await;

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
    assert_eq!(graph["name"], "A2A: Analyst");
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
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    let analyst = exposed_analyst(&app, &owner, &["stub"]).await;
    let principal = analyst["principal_id"].as_str().unwrap();
    let (_, secret) = counterpart(&app, &owner, &wid, "stub").await;

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
    let app = TestApp::new(pool, &[]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    let analyst = exposed_analyst(&app, &owner, &["stub"]).await;
    let principal = analyst["principal_id"].as_str().unwrap();
    let (stub, secret) = counterpart(&app, &owner, &wid, "stub").await;
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
    let (_, foreign) = counterpart(&app, &other, &other_wid, "stub").await;
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
    let (_, fresh) = counterpart(&app, &owner, &wid, "fresh").await;
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
