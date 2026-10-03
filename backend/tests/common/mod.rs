//! Shared harness for integration tests: settings, a fake LLM provider and
//! a request helper running the real router with `tower::ServiceExt::oneshot`.
#![allow(dead_code)]
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use base64::Engine as _;
use http_body_util::BodyExt;
use nexc::app::{AppState, http_client, router};
use nexc::config::Settings;
use nexc::domain::memory::MEMORY_SCHEMA_NAME;
use nexc::domain::plan::{PLAN_SCHEMA_NAME, PlanContext};
use nexc::domain::prompt::parse_node_prompt;
use nexc::llm::{LlmError, LlmEvent, LlmProvider, LlmRequest, LlmStream, StopReason, Usage};
use nexc::realtime::hub::SseFrame;
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub const PASSWORD: &str = "correct horse battery staple";

/// Test settings; `overrides` replace or add variables.
pub fn settings(overrides: &[(&str, &str)]) -> Settings {
    let data_dir = std::env::temp_dir().join(format!("nexc-test-{}", Uuid::now_v7()));
    let mut vars: HashMap<String, String> = [
        ("NEXC_DATABASE_URL", "postgres://unused@localhost/unused"),
        (
            "NEXC_JWT_SECRET",
            "test-jwt-secret-0123456789-abcdefghijklmnop",
        ),
        (
            "NEXC_MASTER_KEY",
            &base64::engine::general_purpose::STANDARD.encode([7u8; 32]),
        ),
        (
            "NEXC_RUNTIME_TOKEN",
            "test-runtime-token-0123456789-abcdefghij",
        ),
        ("NEXC_DATA_DIR", data_dir.to_str().unwrap()),
        ("NEXC_MAX_ATTEMPTS", "2"),
        ("ANTHROPIC_API_KEY", "sk-test-not-a-real-key"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    for (k, v) in overrides {
        if v.is_empty() {
            vars.remove(*k);
        } else {
            vars.insert((*k).to_owned(), (*v).to_owned());
        }
    }
    Settings::from_lookup(|k| vars.get(k).cloned()).expect("valid test settings")
}

/// Deterministic fake LLM. Plans propose `Outline -> Draft` for the first
/// node; node outputs echo the task title. Titles containing `FAIL` fail
/// permanently, titles containing `FLAKY` fail once with a retryable error.
#[derive(Default)]
pub struct FakeLlm {
    pub calls: AtomicUsize,
    flaky_failures: AtomicUsize,
}

impl FakeLlm {
    fn events(text: String, usage: Usage) -> LlmStream {
        let mut events: Vec<Result<LlmEvent, LlmError>> = vec![Ok(LlmEvent::Usage(Usage {
            output_tokens: 0,
            ..usage
        }))];
        let chars: Vec<char> = text.chars().collect();
        events.extend(
            chars
                .chunks(7)
                .map(|c| Ok(LlmEvent::Text(c.iter().collect()))),
        );
        events.push(Ok(LlmEvent::Usage(usage)));
        events.push(Ok(LlmEvent::Done(StopReason::EndTurn)));
        Box::pin(futures::stream::iter(events))
    }

    fn plan(prompt: &str) -> String {
        let ctx = PlanContext::extract(prompt).expect("planner embeds its context");
        let first = ctx.nodes.first().map(|n| n.id);
        json!({
            "summary": "Split the work into an outline and a draft.",
            "nodes": [
                {"ref": "outline", "existing_id": first, "title": "Outline", "content": "Make an outline.",
                 "kind": "task", "agent_role": "planner", "executor": "llm", "tags": ["plan"]},
                {"ref": "draft", "existing_id": null, "title": "Draft", "content": "Write the draft.",
                 "kind": "document", "agent_role": "writer", "executor": "llm", "tags": []},
                {"ref": "final", "existing_id": "not-a-uuid", "title": "Final", "content": "Polish.",
                 "kind": "output", "agent_role": "writer", "executor": "llm", "tags": []}
            ],
            "edges": [
                {"source_ref": "outline", "target_ref": "draft"},
                {"source_ref": "draft", "target_ref": "final"},
                {"source_ref": "final", "target_ref": "outline"}
            ]
        })
        .to_string()
    }
}

impl LlmProvider for FakeLlm {
    fn stream(&self, req: LlmRequest) -> LlmStream {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let prompt = req
            .messages
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_default();
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 20,
        };
        match req.json_schema.as_ref().map(|s| s.name) {
            Some(PLAN_SCHEMA_NAME) => return Self::events(Self::plan(&prompt), usage),
            Some(MEMORY_SCHEMA_NAME) => {
                let text = json!({"memories": [{"kind": "fact", "content": "Reports use APA style.", "importance": 0.7}]});
                return Self::events(text.to_string(), usage);
            }
            _ => {}
        }
        let (title, upstream) = parse_node_prompt(&prompt).unwrap_or_default();
        if title.contains("FAIL") {
            return Box::pin(futures::stream::iter([Err(LlmError::from_status(
                400,
                "bad request".into(),
            ))]));
        }
        if title.contains("FLAKY") && self.flaky_failures.fetch_add(1, Ordering::SeqCst) == 0 {
            return Box::pin(futures::stream::iter([Err(LlmError::from_status(
                529,
                "overloaded".into(),
            ))]));
        }
        Self::events(
            format!("Result of {title} (after: {})", upstream.join(", ")),
            usage,
        )
    }
}

/// A running application on an isolated test database.
pub struct TestApp {
    pub state: AppState,
    pub router: Router,
    pub fake: Arc<FakeLlm>,
    shutdown: CancellationToken,
}

impl Drop for TestApp {
    fn drop(&mut self) {
        self.shutdown.cancel();
        let _ = std::fs::remove_dir_all(&self.state.settings.data_dir);
    }
}

/// HTTP response parts.
pub struct Resp {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Value,
}

impl TestApp {
    /// App with the fake LLM and the run dispatcher started.
    pub async fn new(pool: PgPool, overrides: &[(&str, &str)]) -> Self {
        let fake = Arc::new(FakeLlm::default());
        let state = AppState::with_provider(
            settings(overrides),
            pool,
            http_client().unwrap(),
            fake.clone(),
        )
        .unwrap();
        Self::start(state, fake)
    }

    /// App using the production provider router (for demo mode).
    pub async fn with_real_providers(pool: PgPool, overrides: &[(&str, &str)]) -> Self {
        let state = AppState::new(settings(overrides), pool).unwrap();
        Self::start(state, Arc::new(FakeLlm::default()))
    }

    fn start(state: AppState, fake: Arc<FakeLlm>) -> Self {
        let shutdown = CancellationToken::new();
        tokio::spawn(nexc::engine::scheduler::dispatcher(
            state.clone(),
            shutdown.clone(),
        ));
        let router = router::build(state.clone());
        TestApp {
            state,
            router,
            fake,
            shutdown,
        }
    }

    /// Sends a request; `body` is JSON when given.
    pub async fn request(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> Resp {
        self.request_with(method, path, token, body, &[]).await
    }

    /// Sends a request with extra headers.
    pub async fn request_with(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> Resp {
        let mut builder = Request::builder().method(method).uri(path);
        if let Some(t) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        for (k, v) in headers {
            builder = builder.header(*k, *v);
        }
        let req = match body {
            Some(json) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap();
        let resp = tower::ServiceExt::oneshot(self.router.clone(), req)
            .await
            .unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()));
        Resp {
            status,
            headers,
            body,
        }
    }

    /// Registers a user; returns `(access token, refresh cookie value)`.
    pub async fn register(&self, email: &str) -> (String, String) {
        let r = self
            .request(
                Method::POST,
                "/api/v1/auth/register",
                None,
                Some(json!({"email": email, "password": PASSWORD, "name": "Test"})),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
        (
            r.body["access_token"].as_str().unwrap().to_owned(),
            refresh_cookie(&r.headers).unwrap(),
        )
    }

    /// Creates a graph; returns its id.
    pub async fn graph(&self, token: &str, goal: &str) -> String {
        let r = self
            .request(
                Method::POST,
                "/api/v1/graphs",
                Some(token),
                Some(json!({"name": "G", "goal": goal})),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
        r.body["id"].as_str().unwrap().to_owned()
    }

    /// Adds a node; returns its id.
    pub async fn node(&self, token: &str, gid: &str, body: Value) -> String {
        let r = self
            .request(
                Method::POST,
                &format!("/api/v1/graphs/{gid}/nodes"),
                Some(token),
                Some(body),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
        r.body["id"].as_str().unwrap().to_owned()
    }

    /// Adds an edge and returns the response.
    pub async fn edge(&self, token: &str, gid: &str, source: &str, target: &str) -> Resp {
        let body = json!({"source": source, "target": target});
        self.request(
            Method::POST,
            &format!("/api/v1/graphs/{gid}/edges"),
            Some(token),
            Some(body),
        )
        .await
    }

    /// Subscribes to a graph's SSE events (before triggering work).
    pub fn events(&self, gid: &str) -> broadcast::Receiver<SseFrame> {
        self.state.hub.subscribe_sse(gid.parse().unwrap())
    }
}

/// Extracts the `nexc_refresh` value from `Set-Cookie`.
pub fn refresh_cookie(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::SET_COOKIE)?.to_str().ok()?;
    let value = raw.strip_prefix("nexc_refresh=")?.split(';').next()?;
    (!value.is_empty()).then(|| value.to_owned())
}

/// Collects events until one named `last` arrives (30 s limit).
pub async fn collect_until(
    rx: &mut broadcast::Receiver<SseFrame>,
    last: &str,
) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let frame = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "timed out waiting for {last}; got {:?}",
                    out.iter().map(|(e, _)| e).collect::<Vec<_>>()
                )
            })
            .expect("hub open");
        let data: Value = serde_json::from_str(&frame.data).unwrap();
        let done = &*frame.event == last;
        out.push((frame.event.to_string(), data));
        if done {
            return out;
        }
    }
}

/// Bearer token the test settings give the agent runtime.
pub const RUNTIME_TOKEN: &str = "test-runtime-token-0123456789-abcdefghij";

/// Starts an in-process stand-in for the Python agent runtime that speaks the
/// NDJSON protocol of CONTRACT §8: it checks the bearer token, streams a
/// delta, an incremental `tokens` event, one small artifact and a `result`.
/// Returns its base URL for `NEXC_RUNTIME_URL`.
pub async fn fake_runtime() -> String {
    use axum::routing::{get, post};

    async fn execute(
        headers: HeaderMap,
        axum::Json(req): axum::Json<Value>,
    ) -> axum::response::Response {
        let authorized = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            == Some(&format!("Bearer {RUNTIME_TOKEN}"));
        if !authorized {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        let title = req["task"]["title"].as_str().unwrap_or("node");
        let lines = [
            json!({"type": "delta", "text": format!("[runtime] {title}")}),
            json!({"type": "tokens", "input": 10, "output": 5}),
            json!({
                "type": "artifact",
                "path": "deliverable.md",
                "mime": "text/markdown",
                "content_b64": base64::engine::general_purpose::STANDARD.encode(format!("# {title}\n")),
            }),
            json!({"type": "result", "output": format!("[runtime] {title}"), "tokens_in": 10, "tokens_out": 5}),
        ];
        let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
        ([(header::CONTENT_TYPE, "application/x-ndjson")], body).into_response()
    }

    use axum::response::IntoResponse;
    let app = Router::new()
        .route(
            "/healthz",
            get(|| async { axum::Json(json!({"status": "ok"})) }),
        )
        .route("/v1/execute", post(execute));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}
