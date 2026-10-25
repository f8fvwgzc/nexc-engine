//! The Agent Firewall scored and held: the runs an external agent reports
//! and the sessions its decisions go to are scored like native runs when
//! they close (baselines, alerts, quarantine) and live while they are open
//! (an agent that starts normally and then exfiltrates is quarantined and
//! its run cancelled before it ends, with the time it took on the trace); a
//! `require_approval` decision is an approval of the approvals engine the
//! agent awaits, and the decision it finally gets is signed like any other.
#![forbid(unsafe_code)]

mod common;

use std::time::{Duration, Instant};

use axum::http::{Method, StatusCode};
use common::TestApp;
use nexc::domain::risk::Scope;
use nexc::ontology::json;
use nexc::rpc::firewall::agent_firewall_client::AgentFirewallClient;
use nexc::rpc::firewall::{self as pb, span_report};
use nexc::security::signing;
use serde_json::{Value, json};
use sqlx::PgPool;
use tonic::metadata::MetadataValue;
use tonic::transport::Channel;
use tonic::{Code, Request};
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

async fn set_policy(app: &TestApp, token: &str, wid: &str, document: Value) {
    let (status, body) = call(
        app,
        Method::POST,
        &format!("/workspaces/{wid}/policies"),
        token,
        Some(json!({"scope": "workspace", "scope_id": null, "document": document})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

/// An external agent with `capabilities`, and an API token for it.
async fn external_agent(
    app: &TestApp,
    token: &str,
    name: &str,
    capabilities: Value,
) -> (Value, String) {
    let (status, agent) = call(
        app,
        Method::POST,
        "/agents",
        token,
        Some(json!({
            "name": name, "role": "courier", "runtime": "external",
            "purpose": "Runs somewhere else and asks nexc before acting.",
            "capabilities": capabilities,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{agent}");
    let id = agent["id"].as_str().unwrap();
    let (status, issued) = call(
        app,
        Method::POST,
        &format!("/agents/{id}/tokens"),
        token,
        Some(json!({"name": "sidecar"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{issued}");
    (agent, issued["secret"].as_str().unwrap().to_owned())
}

fn authed<T>(message: T, token: &str) -> Request<T> {
    let mut request = Request::new(message);
    let value: MetadataValue<_> = format!("Bearer {token}").parse().unwrap();
    request.metadata_mut().insert("authorization", value);
    request
}

async fn client(url: &str) -> AgentFirewallClient<Channel> {
    AgentFirewallClient::connect(url.to_owned()).await.unwrap()
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}

fn span(principal: &str, kind: &str, name: &str, attrs: Value) -> pb::SpanReport {
    pb::SpanReport {
        report: Some(span_report::Report::Span(pb::Span {
            id: Uuid::now_v7().to_string(),
            parent_id: String::new(),
            kind: kind.into(),
            name: name.into(),
            agent: principal.into(),
            started_at: now(),
            ended_at: now(),
            status: "ok".into(),
            attributes: Some(json::to_struct(&attrs)),
        })),
    }
}

fn start(id: &str) -> pb::SpanReport {
    pb::SpanReport {
        report: Some(span_report::Report::Start(pb::RunStart {
            external_run_id: id.into(),
            title: format!("Job {id}"),
        })),
    }
}

fn finish() -> pb::SpanReport {
    pb::SpanReport {
        report: Some(span_report::Report::Finish(pb::RunFinish {
            status: "succeeded".into(),
            output: "done".into(),
        })),
    }
}

/// What the courier usually does in one run: two turns, a delivery to its
/// partner, a little data out.
fn usual_run(principal: &str, bytes: u64) -> Vec<pb::SpanReport> {
    vec![
        span(
            principal,
            "llm_call",
            "claude-test",
            json!({"tokens_in": 100, "tokens_out": 20}),
        ),
        span(
            principal,
            "tool_call",
            "send_parcel",
            json!({"tool": "send_parcel", "egress_host": "api.partner.example", "payload_bytes": bytes}),
        ),
        span(
            principal,
            "llm_call",
            "claude-test",
            json!({"tokens_in": 120, "tokens_out": 30}),
        ),
        span(principal, "tool_call", "finish", json!({"tool": "finish"})),
    ]
}

async fn report(
    client: &mut AgentFirewallClient<Channel>,
    secret: &str,
    reports: Vec<pb::SpanReport>,
) -> pb::ReportSummary {
    client
        .report_span(authed(futures::stream::iter(reports), secret))
        .await
        .unwrap()
        .into_inner()
}

async fn spans_of(app: &TestApp, owner: &str, run_id: &str) -> Vec<Value> {
    let (status, page) = call(
        app,
        Method::GET,
        &format!("/runs/{run_id}/trace?limit=1000"),
        owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    page["spans"].as_array().unwrap().clone()
}

async fn audit_actions(app: &TestApp, owner: &str, wid: &str) -> Vec<(String, String)> {
    let (_, entries) = call(
        app,
        Method::GET,
        &format!("/workspaces/{wid}/audit?limit=200"),
        owner,
        None,
    )
    .await;
    entries
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["action"].as_str().unwrap().to_owned(),
                e["detail"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

fn ask(
    principal: &str,
    action: &str,
    subject: &str,
    context: Option<Value>,
) -> pb::AuthorizeRequest {
    pb::AuthorizeRequest {
        principal: principal.into(),
        action: action.into(),
        subject: subject.into(),
        labels: vec![],
        context: context.map(|c| json::to_struct(&c)),
    }
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn reported_runs_are_scored_when_they_close_and_a_quarantined_agent_gets_no_allow(
    pool: PgPool,
) {
    let app = TestApp::new(pool, &[]).await;
    let server = nexc::firewall::serve(app.state.clone(), "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    set_policy(
        &app,
        &owner,
        &wid,
        json!({"alerts": {"risk_score": 3.0, "quarantine_risk_score": 6.0}}),
    )
    .await;
    let (agent, secret) = external_agent(
        &app,
        &owner,
        "Courier",
        json!({"tools": ["send_parcel", "finish"], "egress_hosts": ["api.partner.example"]}),
    )
    .await;
    let aid: Uuid = agent["id"].as_str().unwrap().parse().unwrap();
    let principal = format!("agent://{wid}/Courier");
    let mut client = client(&server.url()).await;

    // Three usual runs: each closes scored, and the baseline learns it.
    for (i, bytes) in [900, 1000, 1100].into_iter().enumerate() {
        let mut reports = vec![start(&format!("usual-{i}"))];
        reports.extend(usual_run(&principal, bytes));
        reports.push(finish());
        let summary = report(&mut client, &secret, reports).await;
        assert_eq!(summary.status, "succeeded");
        assert!(!summary.quarantined);
    }
    let baseline = nexc::repo::baselines::find(&app.state.db, aid, Scope::Runs)
        .await
        .unwrap()
        .expect("the reported runs were learned");
    assert_eq!(baseline.runs, 3);
    assert!(baseline.hosts.contains("api.partner.example"));
    assert!(
        nexc::repo::baselines::find(&app.state.db, aid, Scope::Sessions)
            .await
            .unwrap()
            .is_none(),
        "runs and sessions are not one baseline"
    );

    // A run whose counts look usual but whose turns do not: the live score (the counts)
    // stays quiet; the score taken when the run closes crosses the quarantine threshold.
    let mut reports = vec![
        start("odd"),
        span(
            &principal,
            "llm_call",
            "claude-test",
            json!({"tokens_in": 40_000, "tokens_out": 9_000}),
        ),
    ];
    for _ in 0..6 {
        reports.push(span(
            &principal,
            "tool_call",
            "finish",
            json!({"tool": "finish"}),
        ));
    }
    reports.push(finish());
    let summary = report(&mut client, &secret, reports).await;
    assert_eq!(summary.status, "succeeded", "{summary:?}");
    assert!(
        !summary.quarantined,
        "the live score saw nothing: {summary:?}"
    );
    assert_eq!(summary.risk_score, 0.0);

    let (_, card) = call(&app, Method::GET, &format!("/agents/{aid}"), &owner, None).await;
    let card = &card["agent"];
    assert_eq!(card["status"], "quarantined", "{card}");
    assert!(card["risk_score"].as_f64().unwrap() >= 6.0, "{card}");
    let reasons = card["risk_reasons"].to_string();
    assert!(reasons.contains("tokens per turn"), "{reasons}");
    let (_, alerts) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/alerts"),
        &owner,
        None,
    )
    .await;
    let rules: Vec<&str> = alerts
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["rule"].as_str().unwrap())
        .collect();
    assert!(rules.contains(&"alerts.risk_score"), "{rules:?}");
    assert!(rules.contains(&"alerts.quarantine_risk_score"), "{rules:?}");
    let audit = audit_actions(&app, &owner, &wid).await;
    assert!(
        audit
            .iter()
            .any(|(a, d)| a == "agent_quarantined" && d.starts_with("automatic: risk score")),
        "{audit:?}"
    );

    // Quarantined: no decision at all, so no allow; no run either.
    let refused = client
        .authorize_action(authed(
            ask(&principal, "tool_call", "send_parcel", None),
            &secret,
        ))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), Code::PermissionDenied);
    let refused = client
        .report_span(authed(futures::stream::iter(vec![start("after")]), &secret))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), Code::PermissionDenied);
    server.stop().await;
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_external_agent_that_exfiltrates_mid_run_is_quarantined_before_its_run_ends(
    pool: PgPool,
) {
    let app = TestApp::new(pool, &[]).await;
    let server = nexc::firewall::serve(app.state.clone(), "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    set_policy(
        &app,
        &owner,
        &wid,
        json!({"alerts": {"quarantine_risk_score": 6.0}}),
    )
    .await;
    let (agent, secret) = external_agent(&app, &owner, "Courier", json!({})).await;
    let aid: Uuid = agent["id"].as_str().unwrap().parse().unwrap();
    let principal = format!("agent://{wid}/Courier");
    let mut client = client(&server.url()).await;
    // The live score waits for a baseline of five runs before it may stop one.
    for (i, bytes) in [900, 1000, 1100, 950, 1050].into_iter().enumerate() {
        let mut reports = vec![start(&format!("usual-{i}"))];
        reports.extend(usual_run(&principal, bytes));
        reports.push(finish());
        report(&mut client, &secret, reports).await;
    }

    // The fourth run starts like the others, then posts a lot to hosts it never called.
    // The client keeps its side of the stream open: the run has not ended for it.
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut streaming = client.clone();
    let token = secret.clone();
    let reply = tokio::spawn(async move {
        streaming
            .report_span(authed(
                tokio_stream::wrappers::UnboundedReceiverStream::new(rx),
                &token,
            ))
            .await
    });
    tx.send(start("rogue")).unwrap();
    for s in usual_run(&principal, 1000).into_iter().take(3) {
        tx.send(s).unwrap();
    }
    let mut sent = 0;
    for i in 0..200 {
        let posted = tx.send(span(
            &principal,
            "tool_call",
            "send_parcel",
            json!({"tool": "send_parcel", "egress_host": format!("drop{i}.evil.example"),
                "payload_bytes": 250_000, "marker": i}),
        ));
        if posted.is_err() || reply.is_finished() {
            break;
        }
        sent += 1;
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let summary = tokio::time::timeout(Duration::from_secs(10), reply)
        .await
        .expect("the server ends the run without the client")
        .unwrap()
        .unwrap()
        .into_inner();
    assert!(
        sent < 200,
        "the run was stopped before the client finished it"
    );
    drop(tx);
    assert!(summary.quarantined, "{summary:?}");
    assert_eq!(summary.status, "cancelled");
    assert!(summary.risk_score >= 6.0, "{summary:?}");
    assert!(summary.reason.contains("never called"), "{summary:?}");
    assert!(summary.time_to_contain_ms < 5_000, "{summary:?}");

    // On the trace: the spans up to the one that tripped it, then `live_risk`.
    let spans = spans_of(&app, &owner, &summary.run_id).await;
    let live = spans
        .iter()
        .find(|s| s["name"] == "live_risk")
        .expect("the containment is on the trace");
    let attrs = &live["attributes"];
    assert_eq!(attrs["decision"], "quarantine");
    assert_eq!(attrs["rule"], "alerts.quarantine_risk_score");
    assert_eq!(attrs["quarantined"], true);
    assert!(attrs["time_to_contain_ms"].as_u64().unwrap() <= summary.time_to_contain_ms + 1);
    let first = attrs["first_anomaly_span_id"].as_str().unwrap();
    let trigger = attrs["trigger_span_id"].as_str().unwrap();
    let markers: Vec<i64> = spans
        .iter()
        .filter_map(|s| s["attributes"]["marker"].as_i64())
        .collect();
    // Nothing the agent sent after the span that tripped it was taken.
    assert_eq!(markers, vec![0], "{markers:?}");
    assert_eq!(
        spans
            .iter()
            .filter(|s| s["id"] == first || s["id"] == trigger)
            .count(),
        if first == trigger { 1 } else { 2 },
        "both spans are on the trace"
    );
    let (_, verified) = call(
        &app,
        Method::GET,
        &format!("/runs/{}/trace/verify", summary.run_id),
        &owner,
        None,
    )
    .await;
    assert_eq!(verified["ok"], true, "{verified}");
    let (_, run) = call(
        &app,
        Method::GET,
        &format!("/runs/{}", summary.run_id),
        &owner,
        None,
    )
    .await;
    assert_eq!(run["status"], "cancelled");
    assert!(
        run["node_runs"][0]["output_preview"]
            .as_str()
            .unwrap()
            .starts_with("quarantined mid-run"),
        "{run}"
    );

    // The agent is quarantined: alerts point at the span, the audit says mid-run.
    let (_, card) = call(&app, Method::GET, &format!("/agents/{aid}"), &owner, None).await;
    assert_eq!(card["agent"]["status"], "quarantined");
    let (_, alerts) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/alerts"),
        &owner,
        None,
    )
    .await;
    let quarantine = alerts
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["rule"] == "alerts.quarantine_risk_score")
        .expect("a quarantine alert");
    assert_eq!(quarantine["severity"], "critical");
    assert_eq!(quarantine["span_id"], live["id"]);
    assert_eq!(quarantine["run_id"].as_str(), Some(summary.run_id.as_str()));
    let audit = audit_actions(&app, &owner, &wid).await;
    assert!(
        audit
            .iter()
            .any(|(a, d)| a == "agent_quarantined" && d.starts_with("automatic, mid-run")),
        "{audit:?}"
    );
    // The rogue run is not what the agent usually does: the baseline did not learn it.
    let baseline = nexc::repo::baselines::find(&app.state.db, aid, Scope::Runs)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(baseline.runs, 5);
    assert!(!baseline.hosts.iter().any(|h| h.ends_with("evil.example")));
    // And it gets no allow any more.
    let refused = client
        .authorize_action(authed(
            ask(&principal, "egress", "api.partner.example", None),
            &secret,
        ))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), Code::PermissionDenied);
    server.stop().await;
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_decisions_only_agent_is_contained_by_the_decision_that_crosses(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let server = nexc::firewall::serve(app.state.clone(), "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    set_policy(
        &app,
        &owner,
        &wid,
        json!({"alerts": {"quarantine_risk_score": 5.0}}),
    )
    .await;
    // Egress reaches only the hosts named: the partner, and the new hosts it asks for below.
    let (agent, secret) = external_agent(
        &app,
        &owner,
        "Sidecar",
        json!({"egress_hosts": ["api.partner.example", "*.evil.example"]}),
    )
    .await;
    let aid: Uuid = agent["id"].as_str().unwrap().parse().unwrap();
    let principal = format!("agent://{wid}/Sidecar");
    let mut client = client(&server.url()).await;
    // Five windows of usual decisions, each sealed and learned as a session.
    for _ in 0..5 {
        for subject in ["api.partner.example", "api.partner.example"] {
            let reply = client
                .authorize_action(authed(ask(&principal, "egress", subject, None), &secret))
                .await
                .unwrap()
                .into_inner();
            assert_eq!(reply.decision, "allow");
        }
        app.state
            .firewall_sessions
            .seal_idle(&app.state, Duration::from_nanos(1))
            .await;
    }
    let baseline = nexc::repo::baselines::find(&app.state.db, aid, Scope::Sessions)
        .await
        .unwrap()
        .expect("sessions are learned");
    assert_eq!(baseline.runs, 5);
    // Then it asks to reach one new host after another: allowed by the policy, until the
    // decision that takes the live score over the threshold, which is refused and signed.
    let mut last = None;
    for i in 0..10 {
        let reply = client
            .authorize_action(authed(
                ask(&principal, "egress", &format!("x{i}.evil.example"), None),
                &secret,
            ))
            .await;
        match reply {
            Ok(reply) => {
                let reply = reply.into_inner();
                if reply.decision == "deny" {
                    last = Some(reply);
                    break;
                }
                assert_eq!(reply.decision, "allow", "{reply:?}");
            }
            Err(status) => panic!("refused before the crossing was answered: {status:?}"),
        }
    }
    let crossed = last.expect("the live score crossed");
    assert_eq!(crossed.rule, "alerts.quarantine_risk_score");
    assert!(crossed.reason.contains("live risk score"), "{crossed:?}");
    assert!(signing::verify(
        &crossed.public_key,
        &crossed.signed,
        &crossed.signature
    ));
    let refused = client
        .authorize_action(authed(
            ask(&principal, "egress", "api.partner.example", None),
            &secret,
        ))
        .await
        .unwrap_err();
    assert_eq!(
        refused.code(),
        Code::PermissionDenied,
        "no allow after the crossing"
    );
    let spans = spans_of(&app, &owner, &crossed.run_id).await;
    assert!(spans.iter().any(|s| s["name"] == "live_risk"), "{spans:?}");
    let (_, run) = call(
        &app,
        Method::GET,
        &format!("/runs/{}", crossed.run_id),
        &owner,
        None,
    )
    .await;
    assert_eq!(run["status"], "cancelled", "the session was closed at once");
    let (_, verified) = call(
        &app,
        Method::GET,
        &format!("/runs/{}/trace/verify", crossed.run_id),
        &owner,
        None,
    )
    .await;
    assert_eq!(verified["ok"], true, "{verified}");
    assert_eq!(nexc::firewall::status(&app.state).open_sessions, 0);
    server.stop().await;
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_held_action_becomes_an_approval_the_agent_awaits_and_gets_signed(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let server = nexc::firewall::serve(app.state.clone(), "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    set_policy(
        &app,
        &owner,
        &wid,
        json!({"require_approval": {"tools": ["wire_money"]}}),
    )
    .await;
    let (agent, secret) = external_agent(
        &app,
        &owner,
        "Treasurer",
        json!({"tools": ["wire_money", "finish"]}),
    )
    .await;
    let aid = agent["id"].as_str().unwrap().to_owned();
    let principal = format!("agent://{wid}/Treasurer");
    let mut client = client(&server.url()).await;
    let digest = "ab".repeat(32);
    let held = client
        .authorize_action(authed(
            ask(
                &principal,
                "tool_call",
                "wire_money",
                Some(json!({"args_sha256": digest})),
            ),
            &secret,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(held.decision, "require_approval");
    assert_eq!(held.rule, "approvals.tools");
    let approval_id = held.approval_id.clone();
    assert!(approval_id.parse::<Uuid>().is_ok(), "{held:?}");
    assert!(!held.approval_expires_at.is_empty());
    assert!(signing::verify(
        &held.public_key,
        &held.signed,
        &held.signature
    ));
    let signed: Value = serde_json::from_slice(&held.signed).unwrap();
    assert_eq!(signed["approval_id"], approval_id);
    assert_eq!(signed["args_sha256"], digest);
    assert_eq!(signed["decision"], "require_approval");
    // Asked again before anyone decided: the same approval answers it.
    let again = client
        .authorize_action(authed(
            ask(
                &principal,
                "tool_call",
                "wire_money",
                Some(json!({"args_sha256": digest})),
            ),
            &secret,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(again.approval_id, approval_id);
    // A malformed digest is refused, not recorded.
    let bad = client
        .authorize_action(authed(
            ask(
                &principal,
                "tool_call",
                "wire_money",
                Some(json!({"args_sha256": "not-hex"})),
            ),
            &secret,
        ))
        .await
        .unwrap_err();
    assert_eq!(bad.code(), Code::InvalidArgument);

    // It is the approvals engine's: listed for the admins, in their inbox.
    let (_, pending) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/approvals?status=pending"),
        &owner,
        None,
    )
    .await;
    let listed = pending
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == approval_id)
        .expect("the approval is listed");
    assert_eq!(listed["tool"], "wire_money");
    assert_eq!(listed["agent_id"], aid);
    assert_eq!(listed["args_sha256"], digest);
    assert_eq!(listed["run_id"].as_str(), Some(held.run_id.as_str()));
    assert_eq!(listed["span_id"].as_str(), Some(held.span_id.as_str()));

    // A poll: still pending, nothing signed yet.
    let polled = client
        .await_approval(authed(
            pb::AwaitApprovalRequest {
                approval_id: approval_id.clone(),
                wait_ms: 0,
            },
            &secret,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(polled.status, "pending");
    assert!(polled.decision.is_none());
    // Another agent does not see it.
    let (_, other_secret) = external_agent(&app, &owner, "Other", json!({})).await;
    let hidden = client
        .await_approval(authed(
            pb::AwaitApprovalRequest {
                approval_id: approval_id.clone(),
                wait_ms: 0,
            },
            &other_secret,
        ))
        .await
        .unwrap_err();
    assert_eq!(hidden.code(), Code::NotFound);

    // The agent awaits; an admin approves; the wait ends with a signed allow.
    let mut waiting = client.clone();
    let (id, token) = (approval_id.clone(), secret.clone());
    let awaited = tokio::spawn(async move {
        let began = Instant::now();
        let reply = waiting
            .await_approval(authed(
                pb::AwaitApprovalRequest {
                    approval_id: id,
                    wait_ms: 20_000,
                },
                &token,
            ))
            .await
            .unwrap()
            .into_inner();
        (reply, began.elapsed())
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (status, decided) = call(
        &app,
        Method::POST,
        &format!("/approvals/{approval_id}/approve"),
        &owner,
        Some(json!({"reason": "invoice checked"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{decided}");
    let (reply, waited) = awaited.await.unwrap();
    assert!(waited < Duration::from_secs(5), "{waited:?}");
    assert_eq!(reply.status, "approved");
    assert_eq!(reply.decided_by, "Test");
    assert_eq!(reply.reason, "invoice checked");
    let decision = reply.decision.expect("a final decision");
    assert_eq!(decision.decision, "allow");
    assert_eq!(decision.approval_id, approval_id);
    assert!(signing::verify(
        &decision.public_key,
        &decision.signed,
        &decision.signature
    ));
    let signed: Value = serde_json::from_slice(&decision.signed).unwrap();
    assert_eq!(signed["approval_id"], approval_id);
    assert_eq!(signed["args_sha256"], digest);
    assert_eq!(signed["decision"], "allow");
    assert_eq!(signed["subject"], "wire_money");
    assert_eq!(signed["span_id"].as_str(), Some(decision.span_id.as_str()));
    assert!(
        signed["reason"]
            .as_str()
            .unwrap()
            .contains("approved by Test: invoice checked")
    );
    // Asked again: the same signed decision, byte for byte.
    let again = client
        .await_approval(authed(
            pb::AwaitApprovalRequest {
                approval_id: approval_id.clone(),
                wait_ms: 0,
            },
            &secret,
        ))
        .await
        .unwrap()
        .into_inner()
        .decision
        .unwrap();
    assert_eq!(again.signed, decision.signed);
    assert_eq!(again.signature, decision.signature);

    // The agent reports the call it made with the approval: no mismatch. The same
    // approval named again is not an approval any more.
    let mut reports = vec![start("wire")];
    for _ in 0..2 {
        reports.push(span(
            &principal,
            "tool_call",
            "wire_money",
            json!({"tool": "wire_money", "approval_id": approval_id, "args_sha256": digest}),
        ));
    }
    reports.push(finish());
    let summary = report(&mut client, &secret, reports).await;
    assert_eq!(summary.policy_mismatches, 1, "{summary:?}");
    let consumed: Option<Uuid> =
        sqlx::query_scalar("SELECT consumed_span_id FROM approvals WHERE id = $1")
            .bind(approval_id.parse::<Uuid>().unwrap())
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert!(consumed.is_some());

    // Denied, and expired: both a signed deny.
    let ask_again = |n: u8| {
        ask(
            &principal,
            "tool_call",
            "wire_money",
            Some(json!({"args_sha256": format!("{n:02x}").repeat(32)})),
        )
    };
    let second = client
        .authorize_action(authed(ask_again(1), &secret))
        .await
        .unwrap()
        .into_inner();
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/approvals/{}/deny", second.approval_id),
        &owner,
        Some(json!({"reason": "not this month"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let denied = client
        .await_approval(authed(
            pb::AwaitApprovalRequest {
                approval_id: second.approval_id.clone(),
                wait_ms: 5_000,
            },
            &secret,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(denied.status, "denied");
    let denial = denied.decision.unwrap();
    assert_eq!(denial.decision, "deny");
    assert!(denial.reason.contains("not this month"), "{denial:?}");
    assert!(signing::verify(
        &denial.public_key,
        &denial.signed,
        &denial.signature
    ));
    let third = client
        .authorize_action(authed(ask_again(2), &secret))
        .await
        .unwrap()
        .into_inner();
    sqlx::query("UPDATE approvals SET expires_at = now() - interval '1 second' WHERE id = $1")
        .bind(third.approval_id.parse::<Uuid>().unwrap())
        .execute(&app.state.db)
        .await
        .unwrap();
    let expired = client
        .await_approval(authed(
            pb::AwaitApprovalRequest {
                approval_id: third.approval_id.clone(),
                wait_ms: 0,
            },
            &secret,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(expired.status, "expired");
    let expiry = expired.decision.unwrap();
    assert_eq!(expiry.decision, "deny");
    assert!(expiry.reason.contains("nobody decided"), "{expiry:?}");

    // A quarantine expires what is still pending: nobody can grant it any more.
    let fourth = client
        .authorize_action(authed(ask_again(3), &secret))
        .await
        .unwrap()
        .into_inner();
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/agents/{aid}/quarantine"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, pending) = call(
        &app,
        Method::GET,
        &format!("/approvals/{}", fourth.approval_id),
        &owner,
        None,
    )
    .await;
    assert_eq!(pending["status"], "expired", "{pending}");
    let refused = client
        .await_approval(authed(
            pb::AwaitApprovalRequest {
                approval_id: fourth.approval_id.clone(),
                wait_ms: 0,
            },
            &secret,
        ))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), Code::PermissionDenied);

    // The session's trace records each decision once, as an `approval` span with its
    // signature, and verifies.
    let session = held.run_id.clone();
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    app.state
        .firewall_sessions
        .seal_idle(&app.state, Duration::ZERO)
        .await;
    let spans = spans_of(&app, &owner, &session).await;
    let approvals: Vec<&Value> = spans.iter().filter(|s| s["kind"] == "approval").collect();
    assert!((3..=4).contains(&approvals.len()), "{approvals:?}");
    let ids: std::collections::HashSet<&str> = approvals
        .iter()
        .map(|s| s["attributes"]["approval_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), approvals.len(), "each decision once");
    let first = approvals
        .iter()
        .find(|s| s["id"].as_str() == Some(decision.span_id.as_str()))
        .expect("the signed decision names its span");
    assert_eq!(first["attributes"]["decision"], "approved");
    assert_eq!(
        first["attributes"]["signature"],
        hex::encode(&decision.signature)
    );
    let (_, verified) = call(
        &app,
        Method::GET,
        &format!("/runs/{session}/trace/verify"),
        &owner,
        None,
    )
    .await;
    assert_eq!(verified["ok"], true, "{verified}");
    server.stop().await;
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn reported_runs_close_when_the_stream_goes_and_tell_the_agent_at_shutdown(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let server = nexc::firewall::serve(app.state.clone(), "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    set_policy(
        &app,
        &owner,
        &wid,
        json!({"egress": {"deny": ["*.evil.example"]}}),
    )
    .await;
    let (_, secret) = external_agent(&app, &owner, "Courier", json!({})).await;
    let principal = format!("agent://{wid}/Courier");
    let mut client = client(&server.url()).await;

    // A reported call to a host the policy denies is a mismatch.
    let summary = report(
        &mut client,
        &secret,
        vec![
            start("denied-host"),
            span(
                &principal,
                "tool_call",
                "http_post",
                json!({"tool": "http_post", "egress_host": "drop.evil.example"}),
            ),
            finish(),
        ],
    )
    .await;
    assert_eq!(summary.policy_mismatches, 1, "{summary:?}");

    // The client goes away mid-run: the run is closed all the same.
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut streaming = client.clone();
    let token = secret.clone();
    let gone = tokio::spawn(async move {
        streaming
            .report_span(authed(
                tokio_stream::wrappers::UnboundedReceiverStream::new(rx),
                &token,
            ))
            .await
    });
    tx.send(start("gone")).unwrap();
    tx.send(span(&principal, "llm_call", "m", json!({"tokens_in": 1})))
        .unwrap();
    let open = || nexc::firewall::status(&app.state).open_sessions;
    let began = Instant::now();
    while open() == 0 && began.elapsed() < Duration::from_secs(5) {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(open(), 1, "the run is open");
    gone.abort();
    drop(tx);
    drop(client);
    let began = Instant::now();
    while open() > 0 && began.elapsed() < Duration::from_secs(10) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(open(), 0, "the run went with its stream");

    // At shutdown, a run being reported is closed where it stood and the agent is told to
    // try again; it is not taken for a quarantine.
    let mut client = self::client(&server.url()).await;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut streaming = client.clone();
    let token = secret.clone();
    let reply = tokio::spawn(async move {
        streaming
            .report_span(authed(
                tokio_stream::wrappers::UnboundedReceiverStream::new(rx),
                &token,
            ))
            .await
    });
    tx.send(start("shutdown")).unwrap();
    let began = Instant::now();
    while open() == 0 && began.elapsed() < Duration::from_secs(5) {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    app.state.firewall_sessions.seal_all(&app.state).await;
    let refused = tokio::time::timeout(Duration::from_secs(5), reply)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(refused.code(), Code::Unavailable, "{refused:?}");
    drop(tx);
    // The agent is not quarantined: it asks again.
    let allowed = client
        .authorize_action(authed(
            ask(&principal, "tool_call", "finish", None),
            &secret,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(allowed.decision, "allow");
    server.stop().await;
}

/// p50, p99 and max of `samples`, in µs.
fn quantiles(samples: &mut [Duration]) -> (f64, f64, f64) {
    samples.sort();
    let at = |q: f64| samples[((samples.len() as f64 * q) as usize).min(samples.len() - 1)];
    let us = |d: Duration| d.as_secs_f64() * 1e6;
    (us(at(0.5)), us(at(0.99)), us(*samples.last().unwrap()))
}

/// What a usual courier run looks like to a baseline, learned `runs` times.
fn learned_baseline(runs: usize) -> nexc::domain::risk::Baseline {
    use nexc::domain::risk::{Baseline, Fold};
    use nexc::domain::trace::SpanKind;
    let mut baseline = Baseline::default();
    for i in 0..runs {
        let mut fold = Fold::default();
        fold.push(
            SpanKind::LlmCall,
            "m",
            &json!({"tokens_in": 100, "tokens_out": 20}),
        );
        fold.push(
            SpanKind::ToolCall,
            "send_parcel",
            &json!({"tool": "send_parcel", "egress_host": "api.partner.example",
                "payload_bytes": 900 + 50 * i}),
        );
        fold.push(SpanKind::ToolCall, "finish", &json!({"tool": "finish"}));
        // The same window seen as decisions: one egress to the partner.
        fold.push(
            SpanKind::Policy,
            "api.partner.example",
            &json!({"decision": "allow", "action": "egress",
                "subject": "api.partner.example", "firewall": true}),
        );
        baseline.learn(&fold.features(0));
    }
    baseline
}

/// `make prove-firewall`: what live risk adds to the Agent Firewall — one
/// span folded and scored in process, an `AuthorizeAction` round trip with
/// live scoring on and off — the time from the first exfiltrating span to
/// the run stopped, and the DLP's false positives on random numbers.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
#[ignore = "a measurement: run with `make prove-firewall`"]
async fn prove_firewall(pool: PgPool) {
    use std::sync::Arc;

    use nexc::domain::policy::{AlertRules, Rules};
    use nexc::domain::policy_document::PiiKind;
    use nexc::domain::trace::SpanKind;
    use nexc::engine::policy::Resolved;
    use nexc::firewall::live::Live;

    // 1. In process: one span folded into the live score and scored.
    let resolved = Arc::new(Resolved {
        capabilities: Default::default(),
        rules: Rules {
            alerts: AlertRules {
                // Above the cap: every span is scored, none trips it.
                quarantine_risk_score: Some(10.5),
                ..AlertRules::default()
            },
            ..Rules::default()
        },
        origins: Default::default(),
        levels: Vec::new(),
    });
    let shapes = [
        (
            SpanKind::LlmCall,
            json!({"tokens_in": 100, "tokens_out": 20}),
        ),
        (
            SpanKind::ToolCall,
            json!({"tool": "send_parcel", "egress_host": "api.partner.example", "payload_bytes": 1000}),
        ),
        (
            SpanKind::Policy,
            json!({"decision": "allow", "action": "egress", "subject": "api.partner.example", "firewall": true}),
        ),
        (SpanKind::ToolCall, json!({"tool": "finish"})),
        (
            SpanKind::Policy,
            json!({"decision": "deny", "action": "egress", "subject": "x.example"}),
        ),
    ];
    const SPANS: usize = 100_000;
    let mut live = Live::new(Scope::Runs, learned_baseline(10), resolved);
    let mut samples = Vec::with_capacity(SPANS);
    for i in 0..SPANS {
        let (kind, attrs) = &shapes[i % shapes.len()];
        let id = Uuid::now_v7();
        let began = Instant::now();
        let crossed = live.observe(id, *kind, "s", attrs);
        samples.push(began.elapsed());
        assert!(crossed.is_none());
    }
    let (p50, p99, max) = quantiles(&mut samples);
    println!(
        "prove-firewall: live risk in process, {SPANS} spans folded and scored: p50 {p50:.2} µs, p99 {p99:.2} µs, max {max:.1} µs"
    );
    let budget: f64 = std::env::var("NEXC_PROVE_LIVE_P99_US")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50.0);
    assert!(p99 < budget, "live risk p99 {p99:.2} µs over {budget} µs");

    // 2. AuthorizeAction round trips, live scoring off (no threshold) and on.
    let app = TestApp::new(pool, &[]).await;
    let server = nexc::firewall::serve(app.state.clone(), "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let mut client = client(&server.url()).await;
    let (plain_owner, _) = app.register("plain@example.com").await;
    let plain_wid = workspace(&app, &plain_owner).await;
    let (_, plain) = external_agent(&app, &plain_owner, "Plain", json!({})).await;
    let (owner, _) = app.register("watched@example.com").await;
    let wid = workspace(&app, &owner).await;
    set_policy(
        &app,
        &owner,
        &wid,
        json!({"alerts": {"quarantine_risk_score": 10.0}}),
    )
    .await;
    let (agent, watched) = external_agent(&app, &owner, "Watched", json!({})).await;
    let aid: Uuid = agent["id"].as_str().unwrap().parse().unwrap();
    nexc::repo::baselines::upsert(&app.state.db, aid, Scope::Sessions, &learned_baseline(10))
        .await
        .unwrap();
    let asks = [
        (format!("agent://{plain_wid}/Plain"), plain),
        (format!("agent://{wid}/Watched"), watched),
    ];
    const CALLS: usize = 800;
    let mut rounds: [Vec<Duration>; 2] = [Vec::new(), Vec::new()];
    for i in 0..(CALLS + 50) {
        for (which, (principal, secret)) in asks.iter().enumerate() {
            let began = Instant::now();
            let reply = client
                .authorize_action(authed(
                    ask(principal, "egress", "api.partner.example", None),
                    secret,
                ))
                .await
                .unwrap()
                .into_inner();
            let took = began.elapsed();
            assert_eq!(reply.decision, "allow");
            if i >= 50 {
                rounds[which].push(took);
            }
        }
    }
    let (off50, off99, _) = quantiles(&mut rounds[0]);
    let (on50, on99, _) = quantiles(&mut rounds[1]);
    println!(
        "prove-firewall: AuthorizeAction over gRPC, {CALLS} calls each: live risk off p50 {off50:.0} µs p99 {off99:.0} µs; on p50 {on50:.0} µs p99 {on99:.0} µs (added p50 {:+.0} µs, p99 {:+.0} µs)",
        on50 - off50,
        on99 - off99
    );

    // 3. Time to contain: a run that starts as usual, then posts to new hosts.
    const ROUNDS: usize = 5;
    let mut contained = Vec::new();
    for round in 0..ROUNDS {
        let name = format!("Courier{round}");
        let (agent, secret) = external_agent(&app, &owner, &name, json!({})).await;
        let aid: Uuid = agent["id"].as_str().unwrap().parse().unwrap();
        nexc::repo::baselines::upsert(&app.state.db, aid, Scope::Runs, &learned_baseline(10))
            .await
            .unwrap();
        let principal = format!("agent://{wid}/{name}");
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut streaming = client.clone();
        let reply = tokio::spawn(async move {
            streaming
                .report_span(authed(
                    tokio_stream::wrappers::UnboundedReceiverStream::new(rx),
                    &secret,
                ))
                .await
        });
        tx.send(start("rogue")).unwrap();
        for s in usual_run(&principal, 1000).into_iter().take(2) {
            tx.send(s).unwrap();
        }
        for i in 0..1000 {
            if reply.is_finished()
                || tx
                    .send(span(
                        &principal,
                        "tool_call",
                        "send_parcel",
                        json!({"tool": "send_parcel", "egress_host": format!("d{i}.evil.example"),
                            "payload_bytes": 250_000}),
                    ))
                    .is_err()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let summary = reply.await.unwrap().unwrap().into_inner();
        assert!(summary.quarantined, "{summary:?}");
        contained.push(Duration::from_millis(summary.time_to_contain_ms));
    }
    let (c50, _, cmax) = quantiles(&mut contained);
    assert!(
        cmax < 1e6,
        "an exfiltrating run took {cmax:.0} µs to contain"
    );
    println!(
        "prove-firewall: exfiltrating external run, {ROUNDS} runs: quarantined and cancelled mid-run every time; time to contain p50 {:.1} ms, max {:.1} ms",
        c50 / 1e3,
        cmax / 1e3
    );
    server.stop().await;

    // 4. DLP: random numbers taken for identifiers.
    let rate = |len: usize, kind: PiiKind| {
        let mut state = 0x9E37_79B9_7F4A_7C15u64 ^ len as u64;
        let mut digit = || {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            char::from(b'0' + ((state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) % 10) as u8)
        };
        const SAMPLES: usize = 100_000;
        let hits = (0..SAMPLES)
            .filter(|_| {
                let number: String = (0..len).map(|_| digit()).collect();
                nexc::domain::dlp::classify(&format!("Reference {number} on file."), &[kind], &[])
                    .pii_total()
                    > 0
            })
            .count();
        hits as f64 * 100.0 / SAMPLES as f64
    };
    println!(
        "prove-firewall: DLP on 100,000 random numbers each: 12 digits taken for a My Number {:.1} % (before the check digit: 100 %); 13 digits taken for a corporate number {:.1} %; 13 digits taken for a national id {:.1} %",
        rate(12, PiiKind::NationalId),
        rate(13, PiiKind::CorporateNumber),
        rate(13, PiiKind::NationalId)
    );
}
