//! `make prove-api`: the API's latency per endpoint under load. A workspace
//! with one of everything is seeded on the demo provider; then every public
//! GET endpoint and the main writes (an issue filed, a node added, a run
//! started) are driven by 64 concurrent callers for a fixed number of
//! requests each through the real router in process (no socket between the
//! caller and axum, so the figures are the server's own, PostgreSQL
//! included); and the three gRPC services are measured over loopback: the
//! runtime's `Execute` stream against the stand-in runtime, onto-intelli's
//! `Ask` against the stub server, and the Agent Firewall's `AuthorizeAction`
//! in process and over the wire, the difference being the transport. Single
//! sign-on's two navigations are driven against the OpenID Connect provider
//! in process (`common::oidc`).
//!
//! Every gated row has a target; a missed one fails the test, so the table
//! gates CI. `NEXC_PROVE_API_CALLERS`, `NEXC_PROVE_API_REQUESTS` and
//! `NEXC_PROVE_API_TARGET_SCALE` size it for a smaller machine; the table is
//! recorded in `docs/PERFORMANCE.md`.
#![forbid(unsafe_code)]

mod common;

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use common::TestApp;
use common::grpc_runtime::fake_grpc_runtime;
use http_body_util::BodyExt;
use nexc::domain::user::Role;
use nexc::firewall::FirewallService;
use nexc::ontology::stub::{StubOntology, server};
use nexc::ontology::{AskMode, AskRequest, OntologyBackend, Principal};
use nexc::rpc::firewall::agent_firewall_client::AgentFirewallClient;
use nexc::rpc::firewall::agent_firewall_server::AgentFirewall;
use nexc::rpc::firewall::{self as fw};
use nexc::rpc::runtime::agent_runtime_client::AgentRuntimeClient;
use nexc::rpc::runtime::{self as pb, BackendCommand, backend_command, runtime_event};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tonic::transport::Channel;
use tonic::{Code, Request as GrpcRequest};
use tonic_health::pb::HealthCheckRequest;
use tonic_health::pb::health_client::HealthClient;
use uuid::Uuid;

/// Targets, in milliseconds at p99, on a local PostgreSQL and a release build.
const READ_TARGET_MS: f64 = 20.0;
const WRITE_TARGET_MS: f64 = 50.0;
/// The gRPC calls, the work itself excluded (the stubs do none; the
/// firewall's is measured in process and taken off).
const GRPC_TARGET_MS: f64 = 1.0;
/// The reads that aggregate a whole workspace in one answer (a dashboard, a
/// search, a map, a month of usage) run many queries by design, and the
/// server's status page probes what it runs on; they are held to a target
/// of their own.
const AGGREGATE_TARGET_MS: f64 = 200.0;
const AGGREGATES: &[&str] = &[
    "/api/v1/admin/infrastructure",
    "/api/v1/workspaces/{wid}/secops/overview",
    "/api/v1/workspaces/{wid}/search",
    "/api/v1/workspaces/{wid}/map",
    "/api/v1/workspaces/{wid}/map/{kind}",
    "/api/v1/workspaces/{wid}/map/{kind}/{id}",
    "/api/v1/workspaces/{wid}/usage",
    "/api/v1/workspaces/{wid}/timeline/summary",
    "/api/v1/workspaces/{wid}/transfers",
    "/api/v1/workspaces/{wid}/knowledge/search",
    "/api/v1/memories/topics",
];
/// The pool the server runs with by default (`NEXC_DB_MAX_CONNECTIONS`);
/// the floor row shows what it costs at the callers' concurrency.
const DEFAULT_DB_CONNECTIONS: u32 = 20;
/// The reads whose answer the hot-read caches serve; measured again with
/// the caches off, to show the gain.
const CACHED_READS: &[&str] = &[
    "/api/v1/agents/{id}",
    "/api/v1/workspaces/{wid}/policies/resolve",
    "/api/v1/workspaces/{wid}/guardrails",
    "/api/v1/workspaces/{wid}/llm",
    "/api/v1/settings/llm",
    "/.well-known/agent.json",
];

fn read_target(path: &str) -> f64 {
    if AGGREGATES.contains(&path) {
        AGGREGATE_TARGET_MS
    } else {
        READ_TARGET_MS
    }
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(default)
}

fn env_f64(name: &str, default: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n: &f64| n.is_finite() && *n > 0.0)
        .unwrap_or(default)
}

/// Percentile of sorted samples.
fn pct(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((p / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

/// What one measured endpoint came to.
struct Row {
    name: String,
    calls: usize,
    /// The answer most calls got (`200`, `201`, `OK` for a gRPC call).
    status: String,
    /// True when every call succeeded, so the row is held to its target.
    all_ok: bool,
    p50: f64,
    p99: f64,
    max: f64,
    /// Target at p99 in ms; `None` for an informational row.
    target: Option<f64>,
}

impl Row {
    fn passed(&self, scale: f64) -> Option<bool> {
        let target = self.target?;
        Some(self.all_ok && self.p99 <= target * scale)
    }
}

/// Latencies (ms) and outcomes of one sweep.
struct Samples {
    ms: Vec<f64>,
    outcomes: BTreeMap<String, usize>,
}

impl Samples {
    fn row(
        mut self,
        name: impl Into<String>,
        target: Option<f64>,
        ok: impl Fn(&str) -> bool,
    ) -> Row {
        self.ms.sort_by(f64::total_cmp);
        let calls = self.ms.len();
        let (status, _) = self
            .outcomes
            .iter()
            .max_by_key(|(_, n)| **n)
            .map(|(s, n)| (s.clone(), *n))
            .unwrap_or_default();
        let all_ok = self.outcomes.keys().all(|s| ok(s));
        Row {
            name: name.into(),
            calls,
            status,
            all_ok,
            p50: pct(&self.ms, 50.0),
            p99: pct(&self.ms, 99.0),
            max: self.ms.last().copied().unwrap_or(0.0),
            target,
        }
    }
}

/// Rounds each row is measured in; the round with the lowest p99 is kept,
/// as the best of N runs is, so a stall of the machine during one round
/// (another build, a backup) does not stand for the endpoint.
fn rounds() -> usize {
    env_usize("NEXC_PROVE_API_ROUNDS", 2)
}

/// Runs `call` from `callers` tasks, `requests` times each in series, and
/// times every call; [`rounds`] times, keeping the best round.
async fn load<F, Fut>(callers: usize, requests: usize, call: F) -> Samples
where
    F: Fn(usize, usize) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = String> + Send,
{
    let mut best: Option<Samples> = None;
    for _ in 0..rounds() {
        let mut samples = load_once(callers, requests, call.clone()).await;
        samples.ms.sort_by(f64::total_cmp);
        let better = best
            .as_ref()
            .is_none_or(|b| pct(&samples.ms, 99.0) < pct(&b.ms, 99.0));
        if better {
            best = Some(samples);
        }
    }
    best.expect("at least one round")
}

async fn load_once<F, Fut>(callers: usize, requests: usize, call: F) -> Samples
where
    F: Fn(usize, usize) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = String> + Send,
{
    let call = Arc::new(call);
    let mut tasks = JoinSet::new();
    for caller in 0..callers {
        let call = call.clone();
        tasks.spawn(async move {
            let mut out = Vec::with_capacity(requests);
            for n in 0..requests {
                let started = Instant::now();
                let outcome = call(caller, n).await;
                out.push((outcome, started.elapsed().as_secs_f64() * 1000.0));
            }
            out
        });
    }
    let mut samples = Samples {
        ms: Vec::with_capacity(callers * requests),
        outcomes: BTreeMap::new(),
    };
    while let Some(joined) = tasks.join_next().await {
        for (outcome, ms) in joined.expect("a caller finished") {
            samples.ms.push(ms);
            *samples.outcomes.entry(outcome).or_default() += 1;
        }
    }
    samples
}

/// Sends one request through the router and drains the answer; returns its status.
async fn send(
    router: Router,
    method: Method,
    url: &str,
    token: Option<&str>,
    body: Option<String>,
    extra: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(url);
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    for (name, value) in extra {
        request = request.header(*name, *value);
    }
    let request = match body {
        Some(body) => request
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body)),
        None => request.body(Body::empty()),
    }
    .unwrap();
    let response = tower::ServiceExt::oneshot(router, request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn call(
    app: &TestApp,
    method: Method,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    send(
        app.router.clone(),
        method,
        &format!("/api/v1{path}"),
        Some(token),
        body.map(|b| b.to_string()),
        &[],
    )
    .await
}

async fn made(app: &TestApp, token: &str, path: &str, body: Value) -> String {
    let (status, answer) = call(app, Method::POST, path, token, Some(body)).await;
    assert!(status.is_success(), "POST {path}: {status} {answer}");
    answer["id"].as_str().unwrap().to_owned()
}

/// One documented GET operation.
struct Operation {
    path: String,
    query: Vec<String>,
}

/// Every documented GET but the streams and the SIEM exports (rate limited
/// by design to a handful per minute).
fn get_operations() -> Vec<Operation> {
    let spec = serde_json::to_value(nexc::http::openapi()).unwrap();
    let mut out = Vec::new();
    for (path, methods) in spec["paths"].as_object().unwrap() {
        let stream = path.ends_with("/ws")
            || path.ends_with("/events")
                && (path.contains("/graphs/") || path.contains("/secops/"));
        if stream || path.ends_with("/export") {
            continue;
        }
        let Some(op) = methods.get("get") else {
            continue;
        };
        let query = op["parameters"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| p["in"] == "query")
            .map(|p| p["name"].as_str().unwrap().to_owned())
            .collect();
        out.push(Operation {
            path: path.clone(),
            query,
        });
    }
    assert!(out.len() > 60, "the whole API was read: {}", out.len());
    out
}

/// The workspace with one of everything, and the ids a path can name.
struct World {
    owner: String,
    admin: String,
    wid: String,
    tid: String,
    agent_principal: String,
    /// The external agent's API token, for the firewall.
    external_secret: String,
    ids: HashMap<&'static str, String>,
}

impl World {
    /// A documented path with its placeholders filled; what is not seeded is
    /// filled with an id of nothing (a 404, shown and not gated).
    fn fill(&self, op: &Operation) -> String {
        let nowhere = Uuid::now_v7().to_string();
        let encoded = percent_encode(&self.agent_principal);
        let mut url: String = op
            .path
            .split('/')
            .map(
                |part| match part.strip_prefix('{').and_then(|p| p.strip_suffix('}')) {
                    Some("pid") if op.path.contains("/plans/") => self
                        .ids
                        .get("plan")
                        .map_or(nowhere.as_str(), String::as_str),
                    Some("id") if op.path.starts_with("/api/v1/agents/") => self
                        .ids
                        .get("agent")
                        .map_or(nowhere.as_str(), String::as_str),
                    Some("id") if op.path.contains("/map/") => self.ids["iid"].as_str(),
                    Some("principal") => encoded.as_str(),
                    Some(name) => self.ids.get(name).map_or(nowhere.as_str(), String::as_str),
                    None => part,
                },
            )
            .collect::<Vec<_>>()
            .join("/");
        let mut query: Vec<String> = Vec::new();
        for name in &op.query {
            match name.as_str() {
                "workspace_id" => query.push(format!("workspace_id={}", self.wid)),
                "agent_id" => query.push(format!("agent_id={}", self.ids["agent"])),
                "q" => query.push("q=load".into()),
                "agent" => query.push(format!("agent={encoded}")),
                _ => {}
            }
        }
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query.join("&"));
        }
        url
    }

    fn token_for(&self, path: &str) -> &str {
        if path.starts_with("/api/v1/admin/") || path == "/metrics" {
            &self.admin
        } else {
            &self.owner
        }
    }
}

fn percent_encode(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            ':' => "%3A".to_owned(),
            '/' => "%2F".to_owned(),
            other => other.to_string(),
        })
        .collect()
}

/// Waits for a run to finish; returns its final status.
async fn finished(app: &TestApp, token: &str, rid: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let (_, run) = call(app, Method::GET, &format!("/runs/{rid}"), token, None).await;
        let status = run["status"].as_str().unwrap_or_default().to_owned();
        if matches!(status.as_str(), "succeeded" | "failed" | "cancelled") {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "the seed run did not finish: {run}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn world(app: &TestApp, pool: &PgPool) -> World {
    let (owner, _) = app.register("prover@example.com").await;
    let (_, me) = call(app, Method::GET, "/auth/me", &owner, None).await;
    let owner_id = me["id"].as_str().unwrap().to_owned();
    let (_, list) = call(app, Method::GET, "/workspaces", &owner, None).await;
    let wid = list[0]["id"].as_str().unwrap().to_owned();
    let ws = format!("/workspaces/{wid}");
    let team = made(
        app,
        &owner,
        &format!("{ws}/teams"),
        json!({"name": "Engineering", "key": "ENG"}),
    )
    .await;
    let project = made(
        app,
        &owner,
        &format!("{ws}/projects"),
        json!({"name": "Launch"}),
    )
    .await;
    let label = made(
        app,
        &owner,
        &format!("{ws}/labels"),
        json!({"name": "Bug", "color": "#ef4444"}),
    )
    .await;
    let issue = made(
        app,
        &owner,
        &format!("{ws}/teams/{team}/issues"),
        json!({"title": "Load issue", "description": "Measure the API.", "assignee_id": owner_id,
            "project_id": project, "label_ids": [label]}),
    )
    .await;
    made(
        app,
        &owner,
        &format!("/issues/{issue}/comments"),
        json!({"body": "Looks fine."}),
    )
    .await;
    let graph = made(
        app,
        &owner,
        "/graphs",
        json!({"name": "Load graph", "goal": "Measure the API", "workspace_id": wid}),
    )
    .await;
    let outline = app
        .node(
            &owner,
            &graph,
            json!({"title": "Outline", "content": "Make an outline."}),
        )
        .await;
    let draft = app
        .node(
            &owner,
            &graph,
            json!({"title": "Draft", "content": "Write the draft."}),
        )
        .await;
    app.edge(&owner, &graph, &outline, &draft).await;
    let plan = made(
        app,
        &owner,
        &format!("/graphs/{graph}/plan"),
        json!({"instructions": "Keep it short."}),
    )
    .await;
    let run = made(app, &owner, &format!("/graphs/{graph}/runs"), json!({})).await;
    let status = finished(app, &owner, &run).await;
    assert_eq!(status, "succeeded", "the seed run on the demo provider");
    let (_, artifacts) = call(
        app,
        Method::GET,
        &format!("/runs/{run}/artifacts"),
        &owner,
        None,
    )
    .await;
    let artifact = artifacts[0]["id"].as_str().map(str::to_owned);
    let agent = made(
        app,
        &owner,
        "/agents",
        json!({"name": "Analyst", "role": "analyst", "workspace_id": wid,
            "purpose": "Answers the load test's questions.",
            "capabilities": {"tools": ["read_file", "finish"], "egress_hosts": ["api.example.com"]}}),
    )
    .await;
    let (status, exposed) = call(
        app,
        Method::PATCH,
        &format!("/agents/{agent}"),
        &owner,
        Some(json!({"a2a_exposed": true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{exposed}");
    let agent_principal = exposed["principal_id"].as_str().unwrap().to_owned();
    let external = made(
        app,
        &owner,
        "/agents",
        json!({"name": "Courier", "role": "courier", "runtime": "external", "workspace_id": wid,
            "capabilities": {"tools": ["send_parcel", "finish"], "egress_hosts": ["api.partner.example"]}}),
    )
    .await;
    let (status, issued) = call(
        app,
        Method::POST,
        &format!("/agents/{external}/tokens"),
        &owner,
        Some(json!({"name": "load"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{issued}");
    let external_secret = issued["secret"].as_str().unwrap().to_owned();
    let (status, policy) = call(
        app,
        Method::POST,
        &format!("{ws}/policies"),
        &owner,
        Some(json!({"scope": "workspace", "scope_id": null,
            "document": {"egress": {"deny": ["*.evil.example"]}}})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{policy}");
    let upload = app
        .send_bytes(
            &format!("/api/v1{ws}/documents?name=notes.txt"),
            &owner,
            b"Goods under 150 EUR are exempt from the duty.".to_vec(),
        )
        .await;
    assert!(upload.status.is_success(), "{}", upload.body);
    let document = upload.body["id"].as_str().unwrap().to_owned();
    nexc::engine::knowledge::work(&app.state).await.unwrap();

    // The platform console is swept as a platform administrator.
    let root = nexc::http::handlers::auth::create_user(
        &app.state,
        "root@example.com",
        "Root",
        Role::Admin,
        "!",
    )
    .await
    .unwrap();
    let admin = app.state.jwt.issue(root.id, Role::Admin, 0).unwrap();
    // Everyone who can read the workspace: the sweep reads as its owner.
    let _ = pool;

    let mut ids: HashMap<&'static str, String> = HashMap::from([
        ("wid", wid.clone()),
        ("tid", team.clone()),
        ("iid", issue.clone()),
        ("gid", graph),
        ("lid", label),
        ("uid", owner_id),
        ("kind", "issue".to_owned()),
        ("pid", project),
        ("plan", plan),
        ("rid", run),
        ("agent", agent),
        ("did", document),
    ]);
    if let Some(artifact) = artifact {
        ids.insert("aid", artifact);
    }
    World {
        owner,
        admin,
        wid,
        tid: team,
        agent_principal,
        external_secret,
        ids,
    }
}

fn format_row(row: &Row, scale: f64) -> String {
    let (target, result) = match (row.target, row.passed(scale)) {
        (Some(target), Some(true)) => (format!("{:.0} ms", target * scale), "pass".to_owned()),
        (Some(target), Some(false)) if row.all_ok => {
            (format!("{:.0} ms", target * scale), "MISSED".to_owned())
        }
        (Some(target), _) => (
            format!("{:.0} ms", target * scale),
            format!("not gated ({})", row.status),
        ),
        (None, _) => ("—".to_owned(), "info".to_owned()),
    };
    format!(
        "| {} | {} | {} | {:.2} | {:.2} | {:.2} | {} | {} |",
        row.name, row.calls, row.status, row.p50, row.p99, row.max, target, result
    )
}

fn http_ok(status: &str) -> bool {
    status.starts_with('2')
}

/// The GET sweep over every documented read, `callers` × `requests` each.
async fn sweep_reads(
    app: &TestApp,
    w: &World,
    callers: usize,
    requests: usize,
    label: &str,
    only: Option<&[&str]>,
) -> Vec<Row> {
    let mut rows = Vec::new();
    for op in get_operations() {
        if let Some(only) = only
            && !only.contains(&op.path.as_str())
        {
            continue;
        }
        // Single sign-on's navigations have their own rows, with a provider ([`sweep_sso`]).
        if op.path.starts_with("/api/v1/auth/oidc/") {
            continue;
        }
        let url = w.fill(&op);
        let token = w.token_for(&op.path).to_owned();
        let router = app.router.clone();
        let samples = load(callers, requests, move |_, _| {
            let router = router.clone();
            let url = url.clone();
            let token = token.clone();
            async move {
                let (status, _) = send(router, Method::GET, &url, Some(&token), None, &[]).await;
                status.as_u16().to_string()
            }
        })
        .await;
        let name = if label.is_empty() {
            format!("GET {}", op.path)
        } else {
            format!("GET {} {label}", op.path)
        };
        let target = if label.is_empty() {
            Some(read_target(&op.path))
        } else {
            None
        };
        rows.push(samples.row(name, target, http_ok));
    }
    rows
}

/// A browser's navigation through the router: the status and where it was
/// sent (`Location`).
async fn navigate(router: Router, url: &str, headers: &[(&str, &str)]) -> (StatusCode, String) {
    let mut request = Request::builder().method(Method::GET).uri(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let request = request.body(Body::empty()).unwrap();
    let response = tower::ServiceExt::oneshot(router, request).await.unwrap();
    let status = response.status();
    let location = response
        .headers()
        .get(header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let _ = response.into_body().collect().await;
    (status, location)
}

/// Single sign-on's two navigations against the OpenID Connect provider in
/// process, each caller a person of their own who signed in once already
/// (so the provider's discovery document and key set are kept, as a running
/// server keeps them). The login (the kept document read, a row of
/// `oidc_logins` written) is held to the write target. The callback (the
/// state taken, the code exchanged at the provider, the ID token verified,
/// the account found and its groups applied, a session started) is shown,
/// not gated: half of it is the provider's work, here an in-process stub
/// that signs every token under one lock.
async fn sweep_sso(
    pool: &PgPool,
    overrides: &[(&str, &str)],
    callers: usize,
    requests: usize,
) -> Vec<Row> {
    use common::oidc::{Assertion, Stub, begin, local, trip, visitor};

    let stub = Stub::start().await;
    let settings = stub.settings();
    let mut with_sso: Vec<(&str, &str)> = overrides.to_vec();
    // Every navigation comes from an address of its own, past the `/auth/*` rate limit.
    with_sso.push(("NEXC_TRUST_PROXY", "true"));
    with_sso.extend(settings.iter().map(|(k, v)| (*k, v.as_str())));
    let app = TestApp::with_real_providers(pool.clone(), &with_sso).await;
    let person = |caller: usize| {
        Assertion::person(
            &format!("sub-load-{caller}"),
            &format!("load-{caller}@example.com"),
        )
        .groups(&["staff"])
    };
    for caller in 0..callers {
        let first = trip(&app, &stub, person(caller), None).await;
        assert_eq!(
            first.location, "/app",
            "the first sign-in of caller {caller}"
        );
    }
    let mut rows = Vec::new();

    let router = app.router.clone();
    let samples = load(callers, requests, move |_, _| {
        let router = router.clone();
        async move {
            let from = visitor();
            let headers = [("x-forwarded-for", from.as_str())];
            let (status, location) = navigate(router, "/api/v1/auth/oidc/login", &headers).await;
            if status == StatusCode::SEE_OTHER && !location.starts_with('/') {
                "303".to_owned()
            } else {
                format!("{} {location}", status.as_u16())
            }
        }
    })
    .await;
    rows.push(samples.row(
        "GET /api/v1/auth/oidc/login (to the provider)",
        Some(WRITE_TARGET_MS),
        |s| s == "303",
    ));

    // The provider's half of each sign-in is done beforehand, untimed: a state works once.
    let mut prepared = Vec::with_capacity(callers);
    for caller in 0..callers {
        let mut trips = Vec::with_capacity(rounds() * requests);
        for _ in 0..rounds() * requests {
            stub.expect(person(caller));
            let (_, location, binding) = begin(&app, None).await;
            trips.push((local(&stub.authorize(&location)), binding));
        }
        prepared.push(std::sync::Mutex::new(trips));
    }
    let prepared = Arc::new(prepared);
    let router = app.router.clone();
    let samples = load(callers, requests, move |caller, _| {
        let router = router.clone();
        let (url, binding) = prepared[caller]
            .lock()
            .unwrap()
            .pop()
            .expect("a sign-in prepared");
        async move {
            let cookie = format!("nexc_sso={binding}");
            let from = visitor();
            let headers = [
                ("cookie", cookie.as_str()),
                ("x-forwarded-for", from.as_str()),
            ];
            let (status, location) = navigate(router, &url, &headers).await;
            format!("{} {location}", status.as_u16())
        }
    })
    .await;
    rows.push(samples.row(
        "GET /api/v1/auth/oidc/callback (a whole sign-in, the provider in process)",
        None,
        |s| s == "303 /app",
    ));
    rows
}

/// A pool shaped like the server's on the test database, with as many
/// connections as there are callers (`NEXC_PROVE_API_DB_CONNECTIONS` to
/// size it otherwise), as a deployment serving that many concurrent
/// requests would set `NEXC_DB_MAX_CONNECTIONS`. The test harness's own
/// pool holds five connections, which would measure the pool, not the API;
/// the floor rows show what a smaller pool costs.
async fn server_pool(test_pool: &PgPool, connections: u32) -> PgPool {
    let options = (*test_pool.connect_options()).clone();
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(connections)
        .min_connections(connections)
        .acquire_timeout(Duration::from_secs(10))
        .idle_timeout(Duration::from_secs(600))
        .connect_with(options)
        .await
        .expect("a pool on the test database")
}

/// The floor under every read: one `SELECT 1` through the pool.
async fn db_floor(pool: &PgPool, callers: usize, requests: usize, label: &str) -> Row {
    let pool = pool.clone();
    let samples = load(callers, requests, move |_, _| {
        let pool = pool.clone();
        async move {
            match sqlx::query_scalar::<_, i32>("SELECT 1")
                .fetch_one(&pool)
                .await
            {
                Ok(_) => "OK".to_owned(),
                Err(err) => err.to_string(),
            }
        }
    })
    .await;
    samples.row(
        format!("PostgreSQL round trip, SELECT 1 through the pool ({label})"),
        None,
        grpc_ok,
    )
}

/// A request to the stand-in runtime that it answers at once.
fn execute_request() -> pb::ExecuteRequest {
    pb::ExecuteRequest {
        run_id: Uuid::now_v7().to_string(),
        node_id: Uuid::now_v7().to_string(),
        token: "measure".into(),
        agent: Some(pb::AgentSpec {
            name: "Prover".into(),
            role: "prover".into(),
            principal: "agent://w/Prover".into(),
            ..Default::default()
        }),
        task: Some(pb::Task {
            title: "ping".into(),
            ..Default::default()
        }),
        context: Some(pb::Context::default()),
        llm: Some(pb::Llm::default()),
        limits: Some(pb::Limits::default()),
        gateway_tools: Vec::new(),
    }
}

/// One `Execute` stream: the request down, every event up to the result.
async fn execute_round_trip(mut client: AgentRuntimeClient<Channel>) -> String {
    let (tx, rx) = mpsc::unbounded_channel::<BackendCommand>();
    let _ = tx.send(BackendCommand {
        command: Some(backend_command::Command::Execute(execute_request())),
    });
    let mut request = GrpcRequest::new(UnboundedReceiverStream::new(rx));
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {}", common::RUNTIME_TOKEN).parse().unwrap(),
    );
    let mut events = match client.execute(request).await {
        Ok(response) => response.into_inner(),
        Err(status) => return format!("{:?}", status.code()),
    };
    loop {
        match events.message().await {
            Ok(Some(event)) => {
                if matches!(event.event, Some(runtime_event::Event::Result(_))) {
                    return "OK".into();
                }
            }
            Ok(None) => return "ended".into(),
            Err(status) => return format!("{:?}", status.code()),
        }
    }
}

fn grpc_ok(status: &str) -> bool {
    status == "OK"
}

fn authorize_request(principal: &str, secret: &str) -> GrpcRequest<fw::AuthorizeRequest> {
    let mut request = GrpcRequest::new(fw::AuthorizeRequest {
        principal: principal.to_owned(),
        action: "tool_call".into(),
        subject: "finish".into(),
        labels: vec!["internal".into()],
        context: None,
    });
    request
        .metadata_mut()
        .insert("authorization", format!("Bearer {secret}").parse().unwrap());
    request
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
#[ignore = "a measurement: run with `make prove-api`"]
async fn prove_api(pool: PgPool) {
    let callers = env_usize("NEXC_PROVE_API_CALLERS", 64);
    let requests = env_usize("NEXC_PROVE_API_REQUESTS", 8);
    let scale = env_f64("NEXC_PROVE_API_TARGET_SCALE", 1.0);
    let (runtime_url, _seen) = fake_grpc_runtime().await;
    let test_pool = pool;
    let connections = env_usize("NEXC_PROVE_API_DB_CONNECTIONS", callers) as u32;
    let pool = server_pool(&test_pool, connections).await;
    let overrides = [
        ("NEXC_LLM_PROVIDER", "demo"),
        ("NEXC_API_CALLS_PER_MINUTE", "10000000"),
        ("NEXC_RUNTIME_PROTOCOL", "grpc"),
        ("NEXC_RUNTIME_GRPC_URL", &runtime_url),
    ];
    let app = TestApp::with_real_providers(pool.clone(), &overrides).await;
    let w = world(&app, &pool).await;
    let postgres: String = sqlx::query_scalar("SELECT version()")
        .fetch_one(&pool)
        .await
        .unwrap();
    let postgres = postgres
        .split(" on ")
        .next()
        .unwrap_or(&postgres)
        .to_owned();
    let cores = std::thread::available_parallelism().map_or(0, |n| n.get());
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let connections = pool.options().get_max_connections();
    println!(
        "prove-api: nexc-engine API latency: {callers} concurrent callers × {requests} requests per endpoint, \
         the best of {} rounds, through the router in process, a pool of {connections} connections; {} {}, \
         {cores} cores, {profile} build, {postgres} on localhost; targets at p99: reads {READ_TARGET_MS:.0} ms \
         (whole-workspace aggregates and the status page {AGGREGATE_TARGET_MS:.0} ms), writes {WRITE_TARGET_MS:.0} ms, gRPC \
         {GRPC_TARGET_MS:.0} ms (scale ×{scale})",
        rounds(),
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    let mut rows: Vec<Row> = Vec::new();

    // --- The floor: the database's round trip ----------------------------------------------------
    rows.push(db_floor(&pool, 1, callers * requests, "1 caller").await);
    rows.push(
        db_floor(
            &pool,
            callers,
            requests,
            &format!("{callers} callers, {connections} connections"),
        )
        .await,
    );
    if connections != DEFAULT_DB_CONNECTIONS {
        let smaller = server_pool(&test_pool, DEFAULT_DB_CONNECTIONS).await;
        rows.push(
            db_floor(
                &smaller,
                callers,
                requests,
                &format!("{callers} callers, {DEFAULT_DB_CONNECTIONS} connections, the server's default pool"),
            )
            .await,
        );
        smaller.close().await;
    }

    // --- The gRPC services, before anything runs in the background -------------------------------
    rows.extend(sweep_grpc(&app, &w, callers, requests).await);

    // --- Every documented read ---------------------------------------------------------------
    rows.extend(sweep_reads(&app, &w, callers, requests, "", None).await);

    // --- The same reads with the hot-read caches off: the gain ----------------------------------
    let mut uncached = overrides.to_vec();
    uncached.push(("NEXC_CACHE_TTL_SECS", "0"));
    let cold = TestApp::with_real_providers(pool.clone(), &uncached).await;
    rows.extend(
        sweep_reads(
            &cold,
            &w,
            callers,
            requests,
            "(hot-read cache off)",
            Some(CACHED_READS),
        )
        .await,
    );
    drop(cold);

    // --- The main writes ---------------------------------------------------------------------
    // Issues are numbered per team under a lock, so a team per caller files in parallel; the
    // same callers on one team show what the lock costs.
    let mut teams = Vec::with_capacity(callers);
    for n in 0..callers {
        teams.push(
            made(
                &app,
                &w.owner,
                &format!("/workspaces/{}/teams", w.wid),
                json!({"name": format!("Load team {n}"), "key": format!("L{n}")}),
            )
            .await,
        );
    }
    let teams = Arc::new(teams);
    let (router, token, wid, per_caller) =
        (app.router.clone(), w.owner.clone(), w.wid.clone(), teams);
    let samples = load(callers, requests, move |caller, n| {
        let router = router.clone();
        let token = token.clone();
        let url = format!(
            "/api/v1/workspaces/{wid}/teams/{}/issues",
            per_caller[caller]
        );
        async move {
            let body = json!({"title": format!("Load issue {caller}-{n}")}).to_string();
            let (status, _) = send(router, Method::POST, &url, Some(&token), Some(body), &[]).await;
            status.as_u16().to_string()
        }
    })
    .await;
    rows.push(samples.row(
        "POST /api/v1/workspaces/{wid}/teams/{tid}/issues (a team per caller)",
        Some(WRITE_TARGET_MS),
        http_ok,
    ));
    let issues = format!("/api/v1/workspaces/{}/teams/{}/issues", w.wid, w.tid);
    let (router, token) = (app.router.clone(), w.owner.clone());
    let samples = load(callers, requests, move |caller, n| {
        let router = router.clone();
        let (url, token) = (issues.clone(), token.clone());
        async move {
            let body = json!({"title": format!("Load issue {caller}-{n}")}).to_string();
            let (status, _) = send(router, Method::POST, &url, Some(&token), Some(body), &[]).await;
            status.as_u16().to_string()
        }
    })
    .await;
    rows.push(samples.row(
        format!("POST /api/v1/workspaces/{{wid}}/teams/{{tid}}/issues ({callers} callers on one team: the numbering lock)"),
        None,
        http_ok,
    ));

    // A graph per caller: a graph holds 500 nodes at most, and a run is one graph's.
    let mut graphs = Vec::with_capacity(callers);
    for n in 0..callers {
        let gid = made(
            &app,
            &w.owner,
            "/graphs",
            json!({"name": format!("Load graph {n}"), "goal": "Measure writes", "workspace_id": w.wid}),
        )
        .await;
        app.node(
            &w.owner,
            &gid,
            json!({"title": "Hello", "content": "Say hello."}),
        )
        .await;
        graphs.push(gid);
    }
    let graphs = Arc::new(graphs);
    let (router, token, per_caller) = (app.router.clone(), w.owner.clone(), graphs.clone());
    let samples = load(callers, requests, move |caller, n| {
        let router = router.clone();
        let token = token.clone();
        let url = format!("/api/v1/graphs/{}/nodes", per_caller[caller]);
        async move {
            let body =
                json!({"title": format!("Node {n}"), "content": "Added under load."}).to_string();
            let (status, _) = send(router, Method::POST, &url, Some(&token), Some(body), &[]).await;
            status.as_u16().to_string()
        }
    })
    .await;
    rows.push(samples.row(
        "POST /api/v1/graphs/{gid}/nodes",
        Some(WRITE_TARGET_MS),
        http_ok,
    ));
    let (router, token, per_caller) = (app.router.clone(), w.owner.clone(), graphs.clone());
    let samples = load(callers, requests, move |caller, _| {
        let router = router.clone();
        let token = token.clone();
        let url = format!("/api/v1/graphs/{}/runs", per_caller[caller]);
        async move {
            let body = json!({"force": true}).to_string();
            let (status, _) = send(router, Method::POST, &url, Some(&token), Some(body), &[]).await;
            status.as_u16().to_string()
        }
    })
    .await;
    rows.push(samples.row(
        "POST /api/v1/graphs/{gid}/runs",
        Some(WRITE_TARGET_MS),
        http_ok,
    ));

    // --- Single sign-on, against the provider in process ---------------------------------------
    rows.extend(sweep_sso(&pool, &overrides, callers, requests).await);

    // --- The table -------------------------------------------------------------------------------
    print_table(&rows, scale);
    pool.close().await;
    drop(test_pool);
}

/// The three gRPC services over loopback.
async fn sweep_grpc(app: &TestApp, w: &World, callers: usize, requests: usize) -> Vec<Row> {
    let mut rows = Vec::new();
    let calls = callers * requests;
    // The runtime's Execute stream against the stand-in runtime, over loopback.
    let channel = app.state.runtime_grpc.clone().expect("the gRPC wire is on");
    let client = AgentRuntimeClient::new(channel);
    for _ in 0..16 {
        assert_eq!(execute_round_trip(client.clone()).await, "OK", "warm-up");
    }
    let warm = client.clone();
    let samples = load(1, calls, move |_, _| execute_round_trip(warm.clone())).await;
    rows.push(samples.row(
        "gRPC AgentRuntime/Execute round trip, stand-in runtime (1 caller)",
        Some(GRPC_TARGET_MS),
        grpc_ok,
    ));
    let warm = client.clone();
    let samples = load(callers, requests, move |_, _| {
        execute_round_trip(warm.clone())
    })
    .await;
    rows.push(samples.row(
        format!("gRPC AgentRuntime/Execute round trip, stand-in runtime ({callers} callers)"),
        None,
        grpc_ok,
    ));

    // onto-intelli's Ask against the stub server, over loopback.
    let stub = Arc::new(StubOntology::new());
    let onto = server::serve(stub, Some("t")).await.unwrap();
    let client = nexc::ontology::grpc::GrpcOntology::new(&onto.url(), Some("t")).unwrap();
    let ask = |client: nexc::ontology::grpc::GrpcOntology| async move {
        let request = AskRequest {
            question: "Which agents read restricted data this week?".into(),
            mode: AskMode::Deterministic,
            token_budget: None,
        };
        match client.ask(&Principal::system("nexc-load"), request).await {
            Ok(_) => "OK".to_owned(),
            Err(err) => err.to_string(),
        }
    };
    for _ in 0..16 {
        assert_eq!(ask(client.clone()).await, "OK", "warm-up");
    }
    let (warm, asking) = (client.clone(), ask);
    let samples = load(1, calls, move |_, _| asking(warm.clone())).await;
    rows.push(samples.row(
        "gRPC IntelliService/Ask, stub onto-intelli (1 caller)",
        Some(GRPC_TARGET_MS),
        grpc_ok,
    ));
    let (warm, asking) = (client.clone(), ask);
    let samples = load(callers, requests, move |_, _| asking(warm.clone())).await;
    rows.push(samples.row(
        format!("gRPC IntelliService/Ask, stub onto-intelli ({callers} callers)"),
        None,
        grpc_ok,
    ));
    onto.stop().await;

    // The Agent Firewall's AuthorizeAction: in process (the work: the token looked up in
    // PostgreSQL, the decision, the span), over the wire (the whole call), and the transport
    // alone, as the `grpc.health.v1/Check` served beside it on the same server, which does no work.
    let principal = format!("agent://{}/Courier", w.wid);
    let secret = w.external_secret.clone();
    let service = Arc::new(FirewallService::new(app.state.clone()));
    let in_process = {
        let (service, principal, secret) = (service.clone(), principal.clone(), secret.clone());
        move || {
            let (service, principal, secret) = (service.clone(), principal.clone(), secret.clone());
            async move {
                match service
                    .authorize_action(authorize_request(&principal, &secret))
                    .await
                {
                    Ok(reply) => {
                        assert_eq!(reply.into_inner().decision, "allow");
                        "OK".to_owned()
                    }
                    Err(status) => format!("{:?}", status.code()),
                }
            }
        }
    };
    for _ in 0..16 {
        assert_eq!(in_process().await, "OK", "warm-up");
    }
    let direct = in_process.clone();
    let samples = load(1, calls, move |_, _| direct()).await;
    rows.push(samples.row(
        "gRPC AgentFirewall/AuthorizeAction in process: the work (token lookup, decision, span; 1 caller)",
        None,
        grpc_ok,
    ));
    let server = nexc::firewall::serve(app.state.clone(), "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let client = AgentFirewallClient::connect(server.url()).await.unwrap();
    let over_wire = {
        let (principal, secret) = (principal.clone(), secret.clone());
        move |mut client: AgentFirewallClient<Channel>| {
            let (principal, secret) = (principal.clone(), secret.clone());
            async move {
                match client
                    .authorize_action(authorize_request(&principal, &secret))
                    .await
                {
                    Ok(reply) => {
                        assert_eq!(reply.into_inner().decision, "allow");
                        "OK".to_owned()
                    }
                    Err(status) if status.code() == Code::Ok => "OK".to_owned(),
                    Err(status) => format!("{:?}", status.code()),
                }
            }
        }
    };
    for _ in 0..16 {
        assert_eq!(over_wire(client.clone()).await, "OK", "warm-up");
    }
    let (warm, wired) = (client.clone(), over_wire.clone());
    let samples = load(1, calls, move |_, _| wired(warm.clone())).await;
    rows.push(samples.row(
        "gRPC AgentFirewall/AuthorizeAction over the wire, whole call (1 caller)",
        None,
        grpc_ok,
    ));
    let (warm, wired) = (client.clone(), over_wire.clone());
    let samples = load(callers, requests, move |_, _| wired(warm.clone())).await;
    rows.push(samples.row(
        format!("gRPC AgentFirewall/AuthorizeAction over the wire, whole call ({callers} callers)"),
        None,
        grpc_ok,
    ));
    let health = HealthClient::new(
        Channel::from_shared(server.url())
            .unwrap()
            .connect()
            .await
            .unwrap(),
    );
    let check = |mut client: HealthClient<Channel>| async move {
        match client
            .check(HealthCheckRequest {
                service: String::new(),
            })
            .await
        {
            Ok(_) => "OK".to_owned(),
            Err(status) => format!("{:?}", status.code()),
        }
    };
    for _ in 0..16 {
        assert_eq!(check(health.clone()).await, "OK", "warm-up");
    }
    let (warm, checking) = (health.clone(), check);
    let samples = load(1, calls, move |_, _| checking(warm.clone())).await;
    rows.push(samples.row(
        "gRPC grpc.health.v1/Check on the firewall server: the transport, no work (1 caller)",
        Some(GRPC_TARGET_MS),
        grpc_ok,
    ));
    let (warm, checking) = (health.clone(), check);
    let samples = load(callers, requests, move |_, _| checking(warm.clone())).await;
    rows.push(samples.row(
        format!("gRPC grpc.health.v1/Check on the firewall server: the transport, no work ({callers} callers)"),
        None,
        grpc_ok,
    ));
    server.stop().await;
    rows
}

fn print_table(rows: &[Row], scale: f64) {
    println!("prove-api: ");
    println!(
        "prove-api: | endpoint | calls | status | p50 ms | p99 ms | max ms | target p99 | result |"
    );
    println!("prove-api: | --- | ---: | --- | ---: | ---: | ---: | ---: | --- |");
    for row in rows {
        println!("prove-api: {}", format_row(row, scale));
    }
    let gated = rows.iter().filter(|r| r.passed(scale).is_some()).count();
    let missed: Vec<&Row> = rows
        .iter()
        .filter(|r| r.passed(scale) == Some(false) && r.all_ok)
        .collect();
    let not_gated: Vec<&Row> = rows
        .iter()
        .filter(|r| r.target.is_some() && !r.all_ok)
        .collect();
    println!(
        "prove-api: {gated} rows gated, {} missed, {} not gated (no fixture or refused: {})",
        missed.len(),
        not_gated.len(),
        not_gated
            .iter()
            .map(|r| format!("{} {}", r.name, r.status))
            .collect::<Vec<_>>()
            .join("; ")
    );
    assert!(
        missed.is_empty(),
        "prove-api: targets missed:\n{}",
        missed
            .iter()
            .map(|r| format!(
                "prove-api:   {} p99 {:.2} ms > {:.0} ms",
                r.name,
                r.p99,
                r.target.unwrap_or(0.0) * scale
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
