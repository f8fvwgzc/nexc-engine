//! Alert routing: a workspace registers destinations (a signed JSON
//! webhook, a Slack incoming webhook); alerts raised and approvals
//! requested or decided go into an outbox and a worker delivers them,
//! signed, retried with backoff, logged; a destination that keeps failing
//! is switched off with an alert; and a SIEM that would rather pull reads
//! alerts and audit entries over a window as NDJSON or JSON.
#![forbid(unsafe_code)]

mod common;

use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use chrono::Utc;
use common::TestApp;
use hmac::{Hmac, KeyInit, Mac};
use nexc::domain::alert::AlertKind;
use nexc::domain::alert_routing::DeliveryEvent;
use nexc::domain::policy::PolicyVersions;
use nexc::domain::policy_document::Severity;
use nexc::engine::defend::{About, Finding};
use nexc::repo;
use nexc::repo::approvals::ApprovalRequest;
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::PgPool;
use uuid::Uuid;

/// One request the receiver saw.
#[derive(Debug, Clone)]
struct Received {
    path: String,
    signature: String,
    timestamp: String,
    content_type: String,
    body: Vec<u8>,
}

impl Received {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("a JSON body")
    }
}

#[derive(Clone)]
struct Receiver {
    seen: Arc<Mutex<Vec<Received>>>,
    /// The status answered.
    answer: Arc<AtomicU16>,
}

async fn receive(
    State(receiver): State<Receiver>,
    axum::extract::Path(path): axum::extract::Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned()
    };
    receiver.seen.lock().unwrap().push(Received {
        path,
        signature: header("x-nexc-signature"),
        timestamp: header("x-nexc-timestamp"),
        content_type: header("content-type"),
        body: body.to_vec(),
    });
    let status = StatusCode::from_u16(receiver.answer.load(Ordering::SeqCst)).unwrap();
    (status, if status.is_success() { "ok" } else { "nope" }).into_response()
}

/// Starts the receiver; returns its base URL.
async fn receiver() -> (String, Receiver) {
    let receiver = Receiver {
        seen: Arc::default(),
        answer: Arc::new(AtomicU16::new(200)),
    };
    let app = Router::new()
        .route("/{path}", post(receive))
        .with_state(receiver.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, receiver)
}

fn verify(secret: &str, body: &[u8], signature: &str) -> bool {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    let expected = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
    expected == signature
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

async fn user_id(app: &TestApp, token: &str) -> Uuid {
    let (_, me) = call(app, Method::GET, "/auth/me", token, None).await;
    me["id"].as_str().unwrap().parse().unwrap()
}

async fn add_member(app: &TestApp, owner: &str, wid: &str, email: &str, role: &str) {
    let (status, body) = call(
        app,
        Method::POST,
        &format!("/workspaces/{wid}/members"),
        owner,
        Some(json!({"email": email, "role": role})),
    )
    .await;
    assert!(status.is_success(), "{body}");
}

/// Registers a destination; returns the body of the 201 (destination and secret).
async fn destination(app: &TestApp, token: &str, wid: &str, input: Value) -> Value {
    let (status, body) = call(
        app,
        Method::POST,
        &format!("/workspaces/{wid}/alert-destinations"),
        token,
        Some(input),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body
}

/// Raises an alert the way the engine does; returns it.
async fn raise(app: &TestApp, wid: &str, actor: Uuid, severity: Severity, title: &str) -> Value {
    let workspace_id: Uuid = wid.parse().unwrap();
    let principal = format!("agent://{wid}/Scribe");
    let alert = nexc::engine::defend::raise(
        &app.state,
        &About {
            workspace_id,
            agent_id: Some(Uuid::now_v7()),
            principal: &principal,
            run_id: None,
            node_id: None,
            graph_id: None,
            actor: Some(actor),
        },
        &Finding {
            span_id: None,
            kind: AlertKind::Dlp,
            severity,
            rule: "dlp.secrets",
            title,
            detail: "an API key in the draft",
        },
    )
    .await
    .expect("the alert is recorded");
    serde_json::to_value(alert).unwrap()
}

async fn pending(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM alert_deliveries WHERE status = 'pending'")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Makes every pending delivery due now (the test does not wait for the backoff).
async fn hurry(pool: &PgPool) {
    sqlx::query("UPDATE alert_deliveries SET next_attempt_at = now() WHERE status = 'pending'")
        .execute(pool)
        .await
        .unwrap();
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn destinations_are_registered_by_admins_and_checked(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    let (member, _) = app.register("member@example.com").await;
    add_member(&app, &owner, &wid, "member@example.com", "member").await;
    let (outsider, _) = app.register("outsider@example.com").await;
    let base = format!("/workspaces/{wid}/alert-destinations");

    // A host on this machine or a private network is refused, like the gateway refuses it.
    for private in [
        "http://127.0.0.1:9/hook",
        "http://localhost:9/hook",
        "http://10.1.2.3/hook",
        "http://169.254.169.254/latest",
    ] {
        let (status, body) = call(
            &app,
            Method::POST,
            &base,
            &owner,
            Some(json!({"name": "siem", "kind": "webhook", "url": private})),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{private}: {body}"
        );
        assert!(body["errors"]["url"].is_array(), "{body}");
    }
    // So is anything that is not an http(s) URL, or a blank name.
    for (name, url) in [
        ("siem", "ftp://siem.example.com/x"),
        ("siem", "not a url"),
        ("siem", "https://user:pw@siem.example.com/x"),
        ("   ", "https://siem.example.com/x"),
    ] {
        let (status, body) = call(
            &app,
            Method::POST,
            &base,
            &owner,
            Some(json!({"name": name, "kind": "webhook", "url": url})),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{name} {url}: {body}"
        );
    }
    let (status, body) = call(
        &app,
        Method::POST,
        &base,
        &owner,
        Some(json!({"name": "siem", "kind": "nope", "url": "https://siem.example.com/x"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // Registered: the secret is in the answer once; the URL never comes back, only its host.
    let issued = destination(
        &app,
        &owner,
        &wid,
        json!({"name": "SIEM", "kind": "webhook", "url": "https://SIEM.example.com/hooks/nexc?token=abc",
            "kinds": ["dlp", "injection", "dlp"], "min_severity": "medium"}),
    )
    .await;
    let secret = issued["secret"].as_str().unwrap();
    assert!(
        secret.starts_with("nexc_whs_") && secret.len() > 40,
        "{secret}"
    );
    let made = &issued["destination"];
    assert_eq!(made["host"], "siem.example.com");
    assert!(
        made.get("url").is_none() && made.get("secret").is_none(),
        "{made}"
    );
    assert_eq!(
        made["kinds"],
        json!(["dlp", "injection"]),
        "sorted, deduplicated"
    );
    assert_eq!(made["min_severity"], "medium");
    assert_eq!(made["enabled"], true);
    assert_eq!(made["include_approvals"], false);
    assert_eq!(made["failures"], 0);
    let id = made["id"].as_str().unwrap().to_owned();
    assert!(
        !issued.to_string().contains("token=abc"),
        "the URL is not shown"
    );

    // The same name twice is a conflict; a second one with another name is fine.
    let (status, _) = call(
        &app,
        Method::POST,
        &base,
        &owner,
        Some(json!({"name": "SIEM", "kind": "slack", "url": "https://hooks.slack.com/services/T/B/x"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let slack = destination(
        &app,
        &owner,
        &wid,
        json!({"name": "ops", "kind": "slack", "url": "https://hooks.slack.com/services/T/B/x",
            "include_approvals": true, "enabled": false}),
    )
    .await;
    assert_eq!(slack["destination"]["enabled"], false);
    assert_ne!(slack["secret"], issued["secret"]);

    // Listed and read by admins; members get 403; outsiders 404 everywhere.
    let (status, list) = call(&app, Method::GET, &base, &owner, None).await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["ops", "SIEM"]);
    let (status, one) = call(&app, Method::GET, &format!("{base}/{id}"), &owner, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(one["name"], "SIEM");
    for (method, path, body) in [
        (Method::GET, base.clone(), None),
        (
            Method::POST,
            base.clone(),
            Some(json!({"name": "x", "kind": "webhook", "url": "https://x.example.com/"})),
        ),
        (Method::GET, format!("{base}/{id}"), None),
        (
            Method::PATCH,
            format!("{base}/{id}"),
            Some(json!({"enabled": false})),
        ),
        (Method::POST, format!("{base}/{id}/test"), None),
        (Method::GET, format!("{base}/{id}/deliveries"), None),
        (Method::DELETE, format!("{base}/{id}"), None),
    ] {
        let (status, _) = call(&app, method.clone(), &path, &member, body.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "member {method} {path}");
        let (status, _) = call(&app, method.clone(), &path, &outsider, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "outsider {method} {path}");
    }

    // Changed: filters, the switch, the URL (checked again), the name (unique).
    let (status, patched) = call(
        &app,
        Method::PATCH,
        &format!("{base}/{id}"),
        &owner,
        Some(json!({"kinds": [], "min_severity": "critical", "include_approvals": true, "enabled": false})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["kinds"], json!([]));
    assert_eq!(patched["min_severity"], "critical");
    assert_eq!(patched["include_approvals"], true);
    assert_eq!(patched["enabled"], false);
    let (status, body) = call(
        &app,
        Method::PATCH,
        &format!("{base}/{id}"),
        &owner,
        Some(json!({"url": "http://192.168.1.1/hook"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, _) = call(
        &app,
        Method::PATCH,
        &format!("{base}/{id}"),
        &owner,
        Some(json!({"name": "ops"})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a name another destination has"
    );
    let (status, body) = call(
        &app,
        Method::PATCH,
        &format!("{base}/{id}"),
        &owner,
        Some(json!({"url": "https://siem2.example.com/x", "name": "SIEM two"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["host"], "siem2.example.com");
    assert_eq!(body["name"], "SIEM two");
    let (status, body) = call(
        &app,
        Method::PATCH,
        &format!("{base}/{id}"),
        &owner,
        Some(json!({"colour": "red"})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "unknown fields: {body}"
    );
    let (status, _) = call(
        &app,
        Method::PATCH,
        &format!("{base}/{id}"),
        &owner,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "an empty patch changes nothing");

    // Removed: gone, and the audit log says it all.
    let (status, _) = call(&app, Method::DELETE, &format!("{base}/{id}"), &owner, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(&app, Method::GET, &format!("{base}/{id}"), &owner, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&app, Method::DELETE, &format!("{base}/{id}"), &owner, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, audit) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/audit"),
        &owner,
        None,
    )
    .await;
    let entries: Vec<(String, String)> = audit
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["action"] == "alert_destination_changed")
        .map(|e| {
            (
                e["subject"].as_str().unwrap().to_owned(),
                e["detail"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert!(
        entries.iter().any(|(s, d)| s == "alerts:SIEM" && d.starts_with("added: webhook to siem.example.com")),
        "{entries:?}"
    );
    assert!(
        entries.iter().any(|(s, d)| s == "alerts:SIEM two"
            && d.contains("renamed from SIEM")
            && d.contains("siem2.example.com")),
        "{entries:?}"
    );
    assert!(
        entries
            .iter()
            .any(|(s, d)| s == "alerts:SIEM two" && d.starts_with("removed:")),
        "{entries:?}"
    );
    assert!(
        entries.iter().any(|(_, d)| d.contains("disabled")),
        "{entries:?}"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn deliveries_are_signed_retried_and_logged(pool: PgPool) {
    let (base_url, received) = receiver().await;
    let app = TestApp::new(
        pool.clone(),
        &[
            ("NEXC_ALERT_ALLOW_PRIVATE_HOSTS", "true"),
            ("NEXC_PUBLIC_URL", "https://nexc.example.com/"),
        ],
    )
    .await;
    let (owner, _) = app.register("owner@example.com").await;
    let owner_id = user_id(&app, &owner).await;
    let wid = workspace(&app, &owner).await;
    let base = format!("/workspaces/{wid}/alert-destinations");
    let hook = destination(
        &app,
        &owner,
        &wid,
        json!({"name": "SIEM", "kind": "webhook", "url": format!("{base_url}/hook"), "min_severity": "medium"}),
    )
    .await;
    let secret = hook["secret"].as_str().unwrap().to_owned();
    let hook_id = hook["destination"]["id"].as_str().unwrap().to_owned();
    let slack = destination(
        &app,
        &owner,
        &wid,
        json!({"name": "ops", "kind": "slack", "url": format!("{base_url}/slack"), "include_approvals": true}),
    )
    .await;
    let slack_id = slack["destination"]["id"].as_str().unwrap().to_owned();
    // A destination that wants nothing of this: off, or above the severity.
    destination(
        &app,
        &owner,
        &wid,
        json!({"name": "off", "kind": "webhook", "url": format!("{base_url}/off"), "enabled": false}),
    )
    .await;
    destination(
        &app,
        &owner,
        &wid,
        json!({"name": "critical only", "kind": "webhook", "url": format!("{base_url}/critical"),
            "min_severity": "critical"}),
    )
    .await;

    // An alert raised goes into the outbox of the two destinations that want it.
    let alert = raise(
        &app,
        &wid,
        owner_id,
        Severity::High,
        "an API key in draft.md",
    )
    .await;
    let alert_id = alert["id"].as_str().unwrap().to_owned();
    assert_eq!(pending(&pool).await, 2);
    let progress = nexc::alerting::pass(&app.state).await.unwrap();
    assert_eq!(
        (progress.delivered, progress.retried, progress.failed),
        (2, 0, 0)
    );
    assert_eq!(pending(&pool).await, 0);
    let seen = received.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 2, "{seen:?}");
    let webhook = seen.iter().find(|r| r.path == "hook").expect("the webhook");
    let slack_msg = seen
        .iter()
        .find(|r| r.path == "slack")
        .expect("the Slack message");

    // The webhook: signed over the body as sent, timestamped, the envelope as documented.
    assert!(
        verify(&secret, &webhook.body, &webhook.signature),
        "{}",
        webhook.signature
    );
    assert!(!verify("nexc_whs_other", &webhook.body, &webhook.signature));
    let at: i64 = webhook.timestamp.parse().unwrap();
    assert!((Utc::now().timestamp() - at).abs() < 60, "{at}");
    assert!(webhook.content_type.starts_with("application/json"));
    let body = webhook.json();
    assert_eq!(body["event"], "alert.raised");
    assert_eq!(body["alert"]["id"], alert_id);
    assert_eq!(body["alert"]["title"], "an API key in draft.md");
    assert_eq!(body["alert"]["severity"], "high");
    assert_eq!(body["workspace"]["id"], wid);
    assert!(body["workspace"]["name"].is_string() && body["workspace"]["slug"].is_string());
    assert_eq!(body["agent"]["name"], "Scribe");
    assert_eq!(body["agent"]["principal"], format!("agent://{wid}/Scribe"));
    assert_eq!(
        body["links"]["console"],
        "https://nexc.example.com/app/secops"
    );
    assert!(
        body["links"]["agent"]
            .as_str()
            .unwrap()
            .starts_with("https://nexc.example.com/app/agents?agent=")
    );
    assert_eq!(body["timestamp"], alert["created_at"]);
    let delivery_id = body["id"].as_str().unwrap().to_owned();
    assert!(delivery_id.parse::<Uuid>().is_ok());

    // Slack: a Block Kit message in a coloured attachment, with link buttons.
    let message = slack_msg.json();
    assert_eq!(message["attachments"][0]["color"], "#ea580c");
    let blocks = message["attachments"][0]["blocks"].as_array().unwrap();
    assert_eq!(blocks[0]["type"], "header");
    assert!(
        blocks[0]["text"]["text"]
            .as_str()
            .unwrap()
            .contains("an API key in draft.md")
    );
    let fields = blocks[1]["fields"].as_array().unwrap();
    let texts: Vec<&str> = fields.iter().map(|f| f["text"].as_str().unwrap()).collect();
    assert!(
        texts.iter().any(|t| t.contains("Scribe"))
            && texts.iter().any(|t| t.contains("dlp.secrets")),
        "{texts:?}"
    );
    let actions = blocks.last().unwrap();
    assert_eq!(actions["type"], "actions");
    assert!(
        actions["elements"]
            .as_array()
            .unwrap()
            .iter()
            .all(|b| b["url"].is_string() && b.get("action_id").is_none())
    );
    assert!(
        verify(
            slack["secret"].as_str().unwrap(),
            &slack_msg.body,
            &slack_msg.signature
        ),
        "Slack deliveries are signed too"
    );

    // The alert knows how many destinations got it; each log says what happened.
    let (_, alerts) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/alerts"),
        &owner,
        None,
    )
    .await;
    let listed = alerts
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == alert_id)
        .unwrap();
    assert_eq!(listed["deliveries"], 2);
    let (status, log) = call(
        &app,
        Method::GET,
        &format!("{base}/{hook_id}/deliveries"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{log}");
    assert_eq!(log.as_array().unwrap().len(), 1);
    assert_eq!(log[0]["status"], "delivered");
    assert_eq!(log[0]["last_status"], 200);
    assert_eq!(log[0]["attempts"], 1);
    assert_eq!(log[0]["event"], "alert.raised");
    assert_eq!(log[0]["alert_id"], alert_id);
    assert_eq!(log[0]["id"], delivery_id);
    assert!(log[0]["delivered_at"].is_string() && log[0]["next_attempt_at"].is_null());
    let (_, after) = call(
        &app,
        Method::GET,
        &format!("{base}/{hook_id}"),
        &owner,
        None,
    )
    .await;
    assert!(after["last_delivered_at"].is_string());
    assert_eq!(after["failures"], 0);

    // A receiver that is down: the delivery waits with backoff, keeps its id, and gets through later.
    received.answer.store(500, Ordering::SeqCst);
    received.seen.lock().unwrap().clear();
    let second = raise(&app, &wid, owner_id, Severity::Critical, "second").await;
    let progress = nexc::alerting::pass(&app.state).await.unwrap();
    assert_eq!(
        (progress.delivered, progress.retried, progress.failed),
        (0, 3, 0),
        "the critical-only destination joins in"
    );
    let first_try: Vec<Received> = received.seen.lock().unwrap().clone();
    assert_eq!(first_try.len(), 3);
    let first_id = first_try.iter().find(|r| r.path == "hook").unwrap().json()["id"].clone();
    let (_, log) = call(
        &app,
        Method::GET,
        &format!("{base}/{hook_id}/deliveries"),
        &owner,
        None,
    )
    .await;
    let waiting = log
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["status"] == "pending")
        .expect("a pending delivery");
    assert_eq!(waiting["last_status"], 500);
    assert_eq!(waiting["attempts"], 1);
    assert!(
        waiting["last_error"]
            .as_str()
            .unwrap()
            .starts_with("HTTP 500")
    );
    let due: chrono::DateTime<Utc> = waiting["next_attempt_at"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let wait = (due - Utc::now()).num_seconds();
    assert!(
        (20..=40).contains(&wait),
        "first retry after about 30 s, not {wait}"
    );
    let (_, hook_now) = call(
        &app,
        Method::GET,
        &format!("{base}/{hook_id}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(hook_now["failures"], 1);
    assert!(hook_now["last_error"].as_str().unwrap().contains("500"));
    // Not due yet: a pass leaves it alone.
    assert_eq!(
        nexc::alerting::pass(&app.state).await.unwrap(),
        nexc::alerting::Progress::default()
    );
    received.answer.store(200, Ordering::SeqCst);
    received.seen.lock().unwrap().clear();
    hurry(&pool).await;
    let progress = nexc::alerting::pass(&app.state).await.unwrap();
    assert_eq!(
        (progress.delivered, progress.retried, progress.failed),
        (3, 0, 0)
    );
    let retried = received.seen.lock().unwrap().clone();
    let again = retried.iter().find(|r| r.path == "hook").unwrap().json();
    assert_eq!(
        again["id"], first_id,
        "the idempotency id survives the retry"
    );
    assert_eq!(again["alert"]["id"], second["id"]);
    let (_, log) = call(
        &app,
        Method::GET,
        &format!("{base}/{hook_id}/deliveries"),
        &owner,
        None,
    )
    .await;
    let settled = log
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == first_id)
        .unwrap();
    assert_eq!(settled["status"], "delivered");
    assert_eq!(settled["attempts"], 2);
    let (_, hook_now) = call(
        &app,
        Method::GET,
        &format!("{base}/{hook_id}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(
        hook_now["failures"], 0,
        "a delivery that went through starts the count over"
    );

    // A receiver that refuses for good (4xx) is not retried.
    received.answer.store(404, Ordering::SeqCst);
    raise(&app, &wid, owner_id, Severity::High, "third").await;
    let progress = nexc::alerting::pass(&app.state).await.unwrap();
    assert_eq!(
        (progress.delivered, progress.retried, progress.failed),
        (0, 0, 2)
    );
    assert_eq!(pending(&pool).await, 0);
    received.answer.store(200, Ordering::SeqCst);

    // The test button: sent at once, logged, answered.
    received.seen.lock().unwrap().clear();
    let (status, test) = call(
        &app,
        Method::POST,
        &format!("{base}/{slack_id}/test"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{test}");
    assert_eq!(test["ok"], true);
    assert_eq!(test["status"], 200);
    assert!(test["latency_ms"].is_number());
    let seen = received.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    let message = seen[0].json();
    assert!(
        message["text"].as_str().unwrap().contains("test"),
        "{message}"
    );
    let (_, log) = call(
        &app,
        Method::GET,
        &format!("{base}/{slack_id}/deliveries"),
        &owner,
        None,
    )
    .await;
    assert_eq!(log[0]["event"], "test");
    assert_eq!(log[0]["status"], "delivered");
    received.answer.store(503, Ordering::SeqCst);
    let (status, test) = call(
        &app,
        Method::POST,
        &format!("{base}/{slack_id}/test"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{test}");
    assert_eq!(test["ok"], false);
    assert_eq!(test["status"], 503);
    assert!(test["detail"].as_str().unwrap().starts_with("HTTP 503"));
    received.answer.store(200, Ordering::SeqCst);

    // Approvals: requested and decided go to the destinations that take them (the Slack one here).
    let approval_id = Uuid::now_v7();
    let gid = app.graph(&owner, "approvals").await;
    let graph_uuid: Uuid = gid.parse().unwrap();
    sqlx::query("UPDATE graphs SET workspace_id = $2 WHERE id = $1")
        .bind(graph_uuid)
        .bind(wid.parse::<Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let run_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO runs (id, graph_id, owner_id, status, max_concurrency)
         VALUES ($1, $2, $3, 'running', 4)",
    )
    .bind(run_id)
    .bind(graph_uuid)
    .bind(owner_id)
    .execute(&pool)
    .await
    .unwrap();
    let versions = PolicyVersions::default();
    let principal = format!("agent://{wid}/Scribe");
    let approval = repo::approvals::insert(
        &pool,
        &ApprovalRequest {
            id: approval_id,
            workspace_id: wid.parse().unwrap(),
            run_id,
            node_id: Uuid::now_v7(),
            graph_id: graph_uuid,
            agent_id: None,
            principal: &principal,
            span_id: Uuid::now_v7(),
            tool: "make_docx",
            args_sha256: None,
            rule: "approvals.tools",
            reason: "exports a document",
            approver_ids: &[],
            expires_at: Utc::now() + chrono::Duration::minutes(10),
            policy_versions: &versions,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        nexc::alerting::enqueue_approval(&app.state, &approval, DeliveryEvent::ApprovalRequested)
            .await,
        1
    );
    received.seen.lock().unwrap().clear();
    let (status, decided) = call(
        &app,
        Method::POST,
        &format!("/approvals/{approval_id}/deny"),
        &owner,
        Some(json!({"reason": "not this quarter"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{decided}");
    assert_eq!(pending(&pool).await, 2);
    let progress = nexc::alerting::pass(&app.state).await.unwrap();
    assert_eq!(progress.delivered, 2);
    let seen = received.seen.lock().unwrap().clone();
    assert!(
        seen.iter().all(|r| r.path == "slack"),
        "only the destination that takes approvals"
    );
    let texts: Vec<String> = seen
        .iter()
        .map(|r| r.json()["text"].as_str().unwrap().to_owned())
        .collect();
    assert!(
        texts
            .iter()
            .any(|t| t.contains("Approval requested: make_docx")),
        "{texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("Denied: make_docx")),
        "{texts:?}"
    );
    let (_, log) = call(
        &app,
        Method::GET,
        &format!("{base}/{slack_id}/deliveries"),
        &owner,
        None,
    )
    .await;
    let events: Vec<&str> = log
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["event"].as_str().unwrap())
        .collect();
    assert!(
        events.contains(&"approval.requested") && events.contains(&"approval.decided"),
        "{events:?}"
    );
    assert!(
        log.as_array()
            .unwrap()
            .iter()
            .any(|d| d["approval_id"] == approval_id.to_string())
    );

    // The metric counts what went out, by kind and outcome.
    let metrics = app.state.metrics.render();
    assert!(
        metrics.contains("nexc_alert_deliveries_total{kind=\"webhook\",outcome=\"delivered\"}"),
        "{metrics}"
    );
    assert!(metrics.contains("nexc_alert_deliveries_total{kind=\"webhook\",outcome=\"retry\"}"));
    assert!(metrics.contains("nexc_alert_deliveries_total{kind=\"slack\",outcome=\"delivered\"}"));
    assert!(metrics.contains("nexc_alert_deliveries_total{kind=\"webhook\",outcome=\"failed\"}"));
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_destination_that_keeps_failing_is_switched_off(pool: PgPool) {
    let (base_url, received) = receiver().await;
    received.answer.store(500, Ordering::SeqCst);
    let app = TestApp::new(pool.clone(), &[("NEXC_ALERT_ALLOW_PRIVATE_HOSTS", "true")]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let owner_id = user_id(&app, &owner).await;
    let wid = workspace(&app, &owner).await;
    let base = format!("/workspaces/{wid}/alert-destinations");
    let hook = destination(
        &app,
        &owner,
        &wid,
        json!({"name": "flaky", "kind": "webhook", "url": format!("{base_url}/hook")}),
    )
    .await;
    let hook_id = hook["destination"]["id"].as_str().unwrap().to_owned();
    raise(&app, &wid, owner_id, Severity::Low, "first").await;
    for attempt in 1..=49 {
        let progress = nexc::alerting::pass(&app.state).await.unwrap();
        assert_eq!(progress.retried, 1, "attempt {attempt}");
        hurry(&pool).await;
    }
    let (_, before) = call(
        &app,
        Method::GET,
        &format!("{base}/{hook_id}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(before["failures"], 49);
    assert_eq!(before["enabled"], true);

    // The fiftieth failure in a row switches it off: audited, and an alert for the admins.
    let progress = nexc::alerting::pass(&app.state).await.unwrap();
    assert_eq!(progress.retried, 1);
    let (_, off) = call(
        &app,
        Method::GET,
        &format!("{base}/{hook_id}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(off["enabled"], false, "{off}");
    assert_eq!(off["failures"], 50);
    assert!(
        off["disabled_reason"]
            .as_str()
            .unwrap()
            .contains("50 deliveries in a row failed"),
        "{off}"
    );
    let (_, audit) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/audit"),
        &owner,
        None,
    )
    .await;
    let switched = audit
        .as_array()
        .unwrap()
        .iter()
        .find(|e| {
            e["action"] == "alert_destination_changed"
                && e["detail"].as_str().unwrap().starts_with("switched off")
        })
        .expect("the switch-off is audited");
    assert_eq!(switched["actor_name"], "system");
    assert!(switched["actor_id"].is_null());
    assert_eq!(switched["subject"], "alerts:flaky");
    let (_, alerts) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/alerts"),
        &owner,
        None,
    )
    .await;
    let notice = alerts
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["rule"] == "alert_destinations.failures")
        .expect("an alert of kind policy");
    assert_eq!(notice["kind"], "policy");
    assert_eq!(notice["severity"], "high");
    assert_eq!(notice["principal"], "system");
    assert!(notice["title"].as_str().unwrap().contains("flaky"));
    // That alert went to nobody: the only destination is off; the old delivery is given up.
    received.answer.store(200, Ordering::SeqCst);
    hurry(&pool).await;
    let progress = nexc::alerting::pass(&app.state).await.unwrap();
    assert_eq!(
        (progress.delivered, progress.retried, progress.failed),
        (0, 0, 1)
    );
    let (_, log) = call(
        &app,
        Method::GET,
        &format!("{base}/{hook_id}/deliveries"),
        &owner,
        None,
    )
    .await;
    assert_eq!(log[0]["status"], "failed");
    assert!(
        log[0]["last_error"]
            .as_str()
            .unwrap()
            .contains("switched off")
    );
    assert_eq!(log[0]["attempts"], 51);

    // Enabled again by an admin: the count starts over, and alerts flow again.
    let (status, on) = call(
        &app,
        Method::PATCH,
        &format!("{base}/{hook_id}"),
        &owner,
        Some(json!({"enabled": true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{on}");
    assert_eq!(on["failures"], 0);
    assert_eq!(on["disabled_reason"], "");
    raise(&app, &wid, owner_id, Severity::Low, "after").await;
    let progress = nexc::alerting::pass(&app.state).await.unwrap();
    assert_eq!(progress.delivered, 1);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_siem_pulls_alerts_and_audit_entries_over_a_window(pool: PgPool) {
    let app = TestApp::new(pool.clone(), &[]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let owner_id = user_id(&app, &owner).await;
    let wid = workspace(&app, &owner).await;
    let (member, _) = app.register("member@example.com").await;
    add_member(&app, &owner, &wid, "member@example.com", "member").await;
    let (outsider, _) = app.register("outsider@example.com").await;
    let old = raise(&app, &wid, owner_id, Severity::Low, "old").await;
    let old_id: Uuid = old["id"].as_str().unwrap().parse().unwrap();
    sqlx::query("UPDATE alerts SET created_at = now() - interval '2 days' WHERE id = $1")
        .bind(old_id)
        .execute(&pool)
        .await
        .unwrap();
    let a = raise(&app, &wid, owner_id, Severity::Medium, "a").await;
    let b = raise(&app, &wid, owner_id, Severity::High, "b").await;
    let export = format!("/workspaces/{wid}/alerts/export");

    // The default window is the last day, as NDJSON, oldest first.
    let r = app
        .request(Method::GET, &format!("/api/v1{export}"), Some(&owner), None)
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.headers["content-type"], "application/x-ndjson");
    let text = r
        .body
        .as_str()
        .expect("NDJSON is not a JSON document")
        .to_owned();
    let lines: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert_eq!(lines[0]["id"], a["id"]);
    assert_eq!(lines[1]["id"], b["id"]);
    assert_eq!(lines[1]["title"], "b");
    assert_eq!(lines[1]["workspace_id"], wid);
    assert!(lines[1]["deliveries"].is_number());

    // A wider window as a JSON array takes the old one too; `until` cuts the end.
    let since = (Utc::now() - chrono::Duration::days(3))
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    let r = app
        .request(
            Method::GET,
            &format!("/api/v1{export}?since={since}&format=json"),
            Some(&owner),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(
        r.headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("application/json")
    );
    let ids: Vec<&str> = r
        .body
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            old["id"].as_str().unwrap(),
            a["id"].as_str().unwrap(),
            b["id"].as_str().unwrap()
        ]
    );
    let until = (Utc::now() - chrono::Duration::days(1))
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    let r = app
        .request(
            Method::GET,
            &format!("/api/v1{export}?since={since}&until={until}&format=json"),
            Some(&owner),
            None,
        )
        .await;
    assert_eq!(r.body.as_array().unwrap().len(), 1, "{}", r.body);
    assert_eq!(r.body[0]["title"], "old");

    // What does not fit: a window the wrong way round, an unknown format, other roles.
    let (status, body) = call(
        &app,
        Method::GET,
        &format!(
            "{export}?since={}&until={since}",
            Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
        ),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("{export}?format=xml"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("{export}?since=yesterday"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(&app, Method::GET, &export, &member, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&app, Method::GET, &export, &outsider, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The audit log, the same way: the alerts raised are in it.
    let audit = format!("/workspaces/{wid}/audit/export");
    let r = app
        .request(
            Method::GET,
            &format!("/api/v1{audit}?format=json"),
            Some(&owner),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let entries = r.body.as_array().unwrap();
    let raised: Vec<&Value> = entries
        .iter()
        .filter(|e| e["action"] == "alert_raised")
        .collect();
    assert_eq!(raised.len(), 3, "{entries:?}");
    assert!(raised.iter().all(|e| e["actor_id"] == owner_id.to_string()));
    assert!(
        entries
            .windows(2)
            .all(|w| w[0]["created_at"].as_str() <= w[1]["created_at"].as_str()),
        "oldest first"
    );
    let r = app
        .request(Method::GET, &format!("/api/v1{audit}"), Some(&owner), None)
        .await;
    assert_eq!(r.headers["content-type"], "application/x-ndjson");
    assert_eq!(r.body.as_str().unwrap().lines().count(), entries.len());
    let (status, _) = call(&app, Method::GET, &audit, &member, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&app, Method::GET, &audit, &outsider, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Pulls are rate limited per account: a burst of ten, then 429 with Retry-After.
    let (puller, _) = app.register("puller@example.com").await;
    let own = workspace(&app, &puller).await;
    let mut limited = None;
    for n in 0..12 {
        let r = app
            .request(
                Method::GET,
                &format!("/api/v1/workspaces/{own}/alerts/export"),
                Some(&puller),
                None,
            )
            .await;
        if r.status == StatusCode::TOO_MANY_REQUESTS {
            limited = Some((n, r));
            break;
        }
        assert_eq!(r.status, StatusCode::OK, "pull {n}");
    }
    let (n, r) = limited.expect("the burst ran out");
    assert_eq!(n, 10);
    assert!(r.headers.contains_key("retry-after"));
}
