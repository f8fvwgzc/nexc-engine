//! The Agent Firewall API (AgentSecOps phase 4, roadmap §6): an external
//! agent holding an API token asks whether it may act and gets a signed
//! decision with a span id, reports a run whose chain verifies like a native
//! one, reads its card; a wrong, revoked or missing token is refused; the
//! agent appears in the registry, the trace and the audit like any other.
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode};
use common::TestApp;
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
    let secret = issued["secret"].as_str().unwrap().to_owned();
    assert!(secret.starts_with("nexc_agt_"));
    assert_eq!(issued["token"]["name"], "sidecar");
    assert_eq!(issued["token"]["prefix"], &secret[..15]);
    (agent, secret)
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

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_external_agent_gets_signed_decisions_and_reports_a_verifiable_run(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let server = nexc::firewall::serve(app.state.clone(), "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    // A workspace policy on top of the agent's own capabilities.
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
    let (agent, secret) = external_agent(
        &app,
        &owner,
        "Courier",
        json!({"tools": ["send_parcel", "finish"], "egress_hosts": ["api.partner.example"]}),
    )
    .await;
    let principal = format!("agent://{wid}/Courier");
    assert_eq!(agent["principal_id"], principal);
    assert_eq!(agent["runtime"], "external");
    let mut client = client(&server.url()).await;

    // Allowed, denied by the capabilities, denied by the policy: each signed, each a span.
    let ask = |action: &str, subject: &str| pb::AuthorizeRequest {
        principal: principal.clone(),
        action: action.into(),
        subject: subject.into(),
        labels: vec!["internal".into()],
        context: None,
    };
    let allowed = client
        .authorize_action(authed(ask("tool_call", "send_parcel"), &secret))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(allowed.decision, "allow");
    assert_eq!(allowed.rule, "tools.allow");
    assert_eq!(allowed.policy_versions.unwrap().workspace, 1);
    assert!(signing::verify(
        &allowed.public_key,
        &allowed.signed,
        &allowed.signature
    ));
    let signed: Value = serde_json::from_slice(&allowed.signed).unwrap();
    assert_eq!(signed["decision"], "allow");
    assert_eq!(signed["principal"], principal);
    assert_eq!(signed["span_id"], allowed.span_id);
    assert_eq!(signed["run_id"], allowed.run_id);
    assert_eq!(signed["decided_at"], allowed.decided_at);
    assert_eq!(
        allowed.public_key,
        app.state.firewall_signer.public_key().to_vec()
    );
    let mut forged = allowed.signed.clone();
    forged[10] ^= 1;
    assert!(!signing::verify(
        &allowed.public_key,
        &forged,
        &allowed.signature
    ));

    let by_caps = client
        .authorize_action(authed(ask("tool_call", "run_python"), &secret))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        (by_caps.decision.as_str(), by_caps.rule.as_str()),
        ("deny", "tools.allow")
    );
    let by_policy = client
        .authorize_action(authed(ask("egress", "api.evil.example"), &secret))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        (by_policy.decision.as_str(), by_policy.rule.as_str()),
        ("deny", "egress.deny")
    );
    let host = client
        .authorize_action(authed(ask("egress", "api.partner.example"), &secret))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(host.decision, "allow");
    let sub_agent = client
        .authorize_action(authed(
            pb::AuthorizeRequest {
                principal: format!("{principal}/helper-1"),
                action: "spawn".into(),
                subject: "helper".into(),
                labels: vec![],
                context: Some(json::to_struct(&json!({"depth": 3, "children": 0}))),
            },
            &secret,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(sub_agent.rule, "delegation.max_depth");
    assert_eq!(
        allowed.run_id, by_caps.run_id,
        "one session run for the decisions"
    );

    // Bad asks are refused without a decision.
    let other = client
        .authorize_action(authed(
            pb::AuthorizeRequest {
                principal: format!("agent://{wid}/Someone"),
                action: "tool_call".into(),
                subject: "send_parcel".into(),
                labels: vec![],
                context: None,
            },
            &secret,
        ))
        .await
        .unwrap_err();
    assert_eq!(other.code(), Code::PermissionDenied);
    let unknown = client
        .authorize_action(authed(ask("teleport", "x"), &secret))
        .await
        .unwrap_err();
    assert_eq!(unknown.code(), Code::InvalidArgument);

    // A run reported span by span: chained, re-checked, sealed.
    let now = || chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
    let span = |kind: &str, name: &str, attrs: Value| pb::Span {
        id: Uuid::now_v7().to_string(),
        parent_id: String::new(),
        kind: kind.into(),
        name: name.into(),
        agent: principal.clone(),
        started_at: now(),
        ended_at: now(),
        status: "ok".into(),
        attributes: Some(json::to_struct(&attrs)),
    };
    let reports = vec![
        pb::SpanReport {
            report: Some(span_report::Report::Start(pb::RunStart {
                external_run_id: "job-42".into(),
                title: "Deliver the parcel".into(),
            })),
        },
        pb::SpanReport {
            report: Some(span_report::Report::Span(span(
                "llm_call",
                "claude-test",
                json!({"model": "claude-test", "turn": 1}),
            ))),
        },
        pb::SpanReport {
            report: Some(span_report::Report::Span(span(
                "tool_call",
                "send_parcel",
                json!({"tool": "send_parcel", "args_sha256": "ab".repeat(32)}),
            ))),
        },
        // Outside the allow-list: on the record as a mismatch.
        pb::SpanReport {
            report: Some(span_report::Report::Span(span(
                "tool_call",
                "run_python",
                json!({"tool": "run_python"}),
            ))),
        },
        pb::SpanReport {
            report: Some(span_report::Report::Finish(pb::RunFinish {
                status: "succeeded".into(),
                output: "delivered".into(),
            })),
        },
    ];
    let summary = client
        .report_span(authed(futures::stream::iter(reports), &secret))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(summary.spans, 4, "three reported, one mismatch");
    assert_eq!(summary.policy_mismatches, 1);
    assert_eq!(summary.trace_hash.len(), 64);
    let run_id = summary.run_id.clone();
    let verified = client
        .verify_trace(authed(
            pb::VerifyRequest {
                run_id: run_id.clone(),
            },
            &secret,
        ))
        .await
        .unwrap()
        .into_inner();
    assert!(verified.ok, "{verified:?}");
    assert_eq!(verified.spans, 4, "three reported, one mismatch");
    assert_eq!(verified.sealed_hash, summary.trace_hash);
    // The same run, through the console's routes: in the registry, the trace and the audit.
    let (status, http_verified) = call(
        &app,
        Method::GET,
        &format!("/runs/{run_id}/trace/verify"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{http_verified}");
    assert_eq!(http_verified["ok"], true);
    assert_eq!(http_verified["sealed_hash"], summary.trace_hash);
    let (_, run) = call(&app, Method::GET, &format!("/runs/{run_id}"), &owner, None).await;
    assert_eq!(run["status"], "succeeded");
    assert_eq!(run["policy_mismatches"], 1);
    assert_eq!(run["node_runs"][0]["output_preview"], "delivered");
    let aid = agent["id"].as_str().unwrap();
    let (_, activity) = call(
        &app,
        Method::GET,
        &format!("/agents/{aid}/activity?limit=50"),
        &owner,
        None,
    )
    .await;
    let names: Vec<&str> = activity
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"send_parcel") && names.contains(&"policy_mismatch"),
        "{names:?}"
    );
    let decision_span = activity
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == allowed.span_id)
        .expect("the decision span is the agent's");
    assert_eq!(decision_span["attributes"]["firewall"], true);
    assert_eq!(decision_span["attributes"]["decision"], "allow");
    let (_, entries) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/audit?limit=100"),
        &owner,
        None,
    )
    .await;
    let actions: Vec<&str> = entries
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert!(actions.contains(&"agent_token_issued"), "{actions:?}");
    assert!(actions.contains(&"policy_mismatch"), "{actions:?}");
    assert!(actions.contains(&"trace_sealed"), "{actions:?}");
    let (_, agents) = call(&app, Method::GET, "/agents", &owner, None).await;
    assert!(agents.as_array().unwrap().iter().any(|a| a["id"] == aid));

    // The card, over gRPC.
    let card = client
        .get_agent_card(authed(pb::AgentCardRequest::default(), &secret))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(card.principal, principal);
    assert_eq!(card.runtime, "external");
    assert_eq!(card.owner, "Test");
    assert_eq!(card.policy_versions.unwrap().workspace, 1);
    assert_eq!(card.workspace_id, wid);
    let caps = card.capabilities.map(|c| json::from_struct(&c)).unwrap();
    assert_eq!(caps["tools"], json!(["send_parcel", "finish"]));
    let missing = client
        .get_agent_card(authed(
            pb::AgentCardRequest {
                principal: format!("agent://{wid}/Nobody"),
            },
            &secret,
        ))
        .await
        .unwrap_err();
    assert_eq!(missing.code(), Code::NotFound);

    // The session run of the decisions is sealed when idle, and verifies.
    let sealed = app
        .state
        .firewall_sessions
        .seal_idle(&app.state, std::time::Duration::ZERO)
        .await;
    assert_eq!(sealed, 1);
    let (status, session) = call(
        &app,
        Method::GET,
        &format!("/runs/{}/trace/verify", allowed.run_id),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    assert_eq!(session["ok"], true);
    assert_eq!(session["spans"], 5, "five decisions were recorded");

    // The firewall's status, for the operator.
    let status = nexc::firewall::status(&app.state);
    assert_eq!(
        status.public_key,
        hex::encode(app.state.firewall_signer.public_key())
    );
    assert_eq!(status.open_sessions, 0);
    server.stop().await;
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_wrong_token_is_refused_and_nothing_crosses_workspaces(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let server = nexc::firewall::serve(app.state.clone(), "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    let (agent, secret) = external_agent(&app, &owner, "Courier", json!({})).await;
    let aid = agent["id"].as_str().unwrap().to_owned();
    let (other_owner, _) = app.register("other@example.com").await;
    let other_wid = workspace(&app, &other_owner).await;
    let (_, other_secret) = external_agent(&app, &other_owner, "Stranger", json!({})).await;
    let mut client = client(&server.url()).await;
    let ask = pb::AuthorizeRequest {
        principal: String::new(),
        action: "tool_call".into(),
        subject: "finish".into(),
        labels: vec![],
        context: None,
    };

    // No token, garbage, a token of the right shape that was never issued.
    let refused = client
        .authorize_action(Request::new(ask.clone()))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), Code::Unauthenticated);
    for bad in [
        "garbage",
        "nexc_agt_",
        &format!("nexc_agt_{}", "A".repeat(43)),
    ] {
        let refused = client
            .authorize_action(authed(ask.clone(), bad))
            .await
            .unwrap_err();
        assert_eq!(refused.code(), Code::Unauthenticated, "{bad}");
    }
    // A token of a native agent cannot be issued; a quarantined agent's is refused.
    let (status, native) = call(
        &app,
        Method::POST,
        "/agents",
        &owner,
        Some(json!({"name": "Native", "role": "native", "runtime": "python"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/agents/{}/tokens", native["id"].as_str().unwrap()),
        &owner,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // The stranger's run is not this agent's to verify, nor is this agent's card the stranger's.
    let reports = vec![pb::SpanReport {
        report: Some(span_report::Report::Start(pb::RunStart {
            external_run_id: "theirs".into(),
            title: "Stranger's run".into(),
        })),
    }];
    let theirs = client
        .report_span(authed(futures::stream::iter(reports), &other_secret))
        .await
        .unwrap()
        .into_inner();
    let refused = client
        .verify_trace(authed(
            pb::VerifyRequest {
                run_id: theirs.run_id.clone(),
            },
            &secret,
        ))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), Code::NotFound);
    let refused = client
        .get_agent_card(authed(
            pb::AgentCardRequest {
                principal: format!("agent://{other_wid}/Stranger"),
            },
            &secret,
        ))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), Code::NotFound);
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/runs/{}", theirs.run_id),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "another workspace's run");
    assert_ne!(wid, other_wid);

    // Revoked: refused at once; quarantined: refused while it lasts.
    let (status, tokens) = call(
        &app,
        Method::GET,
        &format!("/agents/{aid}/tokens"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let tid = tokens[0]["id"].as_str().unwrap().to_owned();
    assert!(
        tokens[0]["last_used_at"].is_string(),
        "its use is noted: {tokens}"
    );
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/agents/{aid}/tokens/{tid}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let refused = client
        .authorize_action(authed(ask.clone(), &secret))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), Code::Unauthenticated);
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/agents/{aid}/tokens/{tid}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "revoked once");
    let (status, issued) = call(
        &app,
        Method::POST,
        &format!("/agents/{aid}/tokens"),
        &owner,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let fresh = issued["secret"].as_str().unwrap().to_owned();
    assert!(
        client
            .authorize_action(authed(ask.clone(), &fresh))
            .await
            .is_ok()
    );
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/agents/{aid}/quarantine"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // The token and the agent's status are looked up on every call.
    let (_, issued) = call(
        &app,
        Method::POST,
        &format!("/agents/{aid}/tokens"),
        &owner,
        Some(json!({})),
    )
    .await;
    let after = issued["secret"].as_str().unwrap().to_owned();
    for token in [&fresh, &after] {
        let refused = client
            .authorize_action(authed(ask.clone(), token))
            .await
            .unwrap_err();
        assert_eq!(
            refused.code(),
            Code::PermissionDenied,
            "quarantined at once"
        );
    }
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/agents/{aid}/release"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        client.authorize_action(authed(ask, &fresh)).await.is_ok(),
        "released at once"
    );
    let (_, entries) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/audit?limit=100"),
        &owner,
        None,
    )
    .await;
    let actions: Vec<&str> = entries
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert!(actions.contains(&"agent_token_revoked"), "{actions:?}");
    // Only admins handle tokens.
    let (member, _) = app.register("member@example.com").await;
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/members"),
        &owner,
        Some(json!({"email": "member@example.com", "role": "member"})),
    )
    .await;
    assert!(status.is_success());
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/agents/{aid}/tokens"),
        &member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    server.stop().await;
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn reflection_and_health_are_served_beside_the_firewall(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let server = nexc::firewall::serve(app.state.clone(), "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let channel = Channel::from_shared(server.url())
        .unwrap()
        .connect()
        .await
        .unwrap();
    let mut health = tonic_health::pb::health_client::HealthClient::new(channel.clone());
    let reply = health
        .check(tonic_health::pb::HealthCheckRequest {
            service: "nexc.firewall.v1.AgentFirewall".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        reply.status,
        tonic_health::pb::health_check_response::ServingStatus::Serving as i32
    );
    let mut reflection =
        tonic_reflection::pb::v1::server_reflection_client::ServerReflectionClient::new(channel);
    let request = tonic_reflection::pb::v1::ServerReflectionRequest {
        host: String::new(),
        message_request: Some(
            tonic_reflection::pb::v1::server_reflection_request::MessageRequest::ListServices(
                String::new(),
            ),
        ),
    };
    let mut responses = reflection
        .server_reflection_info(futures::stream::iter(vec![request]))
        .await
        .unwrap()
        .into_inner();
    let response = responses.message().await.unwrap().unwrap();
    let services = match response.message_response {
        Some(tonic_reflection::pb::v1::server_reflection_response::MessageResponse::ListServicesResponse(list)) => {
            list.service.into_iter().map(|s| s.name).collect::<Vec<_>>()
        }
        other => panic!("{other:?}"),
    };
    assert!(
        services.contains(&"nexc.firewall.v1.AgentFirewall".to_owned()),
        "{services:?}"
    );
    server.stop().await;
}
