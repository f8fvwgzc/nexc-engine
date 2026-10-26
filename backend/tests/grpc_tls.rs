//! TLS on the gRPC hops: the Agent Firewall API serves it with the
//! operator's certificate (and, with a client CA, answers only callers that
//! present a certificate of it), and every channel the backend opens to an
//! `https://` endpoint (the runtime, onto-intelli, the OTLP collector)
//! verifies the server against the operator's CA and presents the
//! backend's own certificate. The PKI is `tests/fixtures/grpc-tls`.
#![forbid(unsafe_code)]

mod common;

use std::sync::Arc;

use common::TestApp;
use nexc::rpc::{ChannelSettings, ClientTls};
use sqlx::PgPool;
use tonic_health::pb::HealthCheckRequest;
use tonic_health::pb::health_check_response::ServingStatus;
use tonic_health::pb::health_client::HealthClient;

fn fixture(name: &str) -> String {
    format!(
        "{}/tests/fixtures/grpc-tls/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn pem(name: &str) -> Arc<[u8]> {
    std::fs::read(fixture(name)).unwrap().into()
}

/// Whether the health service answers `Serving` over a channel to `url`.
async fn serving(url: &str, settings: &ChannelSettings) -> Result<bool, tonic::Status> {
    let channel = nexc::rpc::channel(url, settings).expect("a channel");
    let reply = HealthClient::new(channel)
        .check(HealthCheckRequest {
            service: String::new(),
        })
        .await?;
    Ok(reply.into_inner().status == ServingStatus::Serving as i32)
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_firewall_serves_tls_and_the_backends_channels_verify_it(pool: PgPool) {
    let (server_pem, server_key) = (fixture("server.pem"), fixture("server-key.pem"));
    let (client_pem, client_key) = (fixture("client.pem"), fixture("client-key.pem"));
    let ca = fixture("ca.pem");
    let app = TestApp::new(
        pool,
        &[
            ("NEXC_GRPC_TLS_CERT_FILE", &server_pem),
            ("NEXC_GRPC_TLS_KEY_FILE", &server_key),
            ("NEXC_GRPC_TLS_CLIENT_CA_FILE", &ca),
            ("NEXC_GRPC_CA_FILE", &ca),
            ("NEXC_GRPC_CLIENT_CERT_FILE", &client_pem),
            ("NEXC_GRPC_CLIENT_KEY_FILE", &client_key),
        ],
    )
    .await;
    let server = nexc::firewall::serve(app.state.clone(), "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let port = server.addr.port();
    let https = format!("https://127.0.0.1:{port}");

    // The backend's own channel settings: the server verified, the backend's certificate shown.
    let ours = app.state.settings.grpc_channel();
    assert!(serving(&https, &ours).await.unwrap());
    // By name too: the certificate is for localhost.
    assert!(
        serving(&format!("https://localhost:{port}"), &ours)
            .await
            .unwrap()
    );

    // Without a client certificate the server answers nobody.
    let anonymous = ChannelSettings {
        tls: ClientTls {
            ca: Some(pem("ca.pem")),
            identity: None,
        },
        ..ChannelSettings::default()
    };
    assert!(serving(&https, &anonymous).await.is_err());

    // A client that trusts another CA does not accept the server's certificate.
    let elsewhere = ChannelSettings {
        tls: ClientTls {
            ca: Some(pem("other-ca.pem")),
            identity: ours.tls.identity.clone(),
        },
        ..ChannelSettings::default()
    };
    assert!(serving(&https, &elsewhere).await.is_err());

    // Nor does the system's trust store, which never heard of the test CA.
    let system = ChannelSettings {
        tls: ClientTls {
            ca: None,
            identity: ours.tls.identity.clone(),
        },
        ..ChannelSettings::default()
    };
    assert!(serving(&https, &system).await.is_err());

    // Plaintext to the TLS port gets no answer.
    assert!(
        serving(&format!("http://127.0.0.1:{port}"), &ours)
            .await
            .is_err()
    );
    server.stop().await;
}

/// Without TLS configured the firewall stays plaintext, as before; a
/// plaintext channel reaches it and an `https://` one does not.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn without_a_certificate_the_firewall_stays_plaintext(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let server = nexc::firewall::serve(app.state.clone(), "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let settings = app.state.settings.grpc_channel();
    assert!(serving(&server.url(), &settings).await.unwrap());
    let https = format!("https://127.0.0.1:{}", server.addr.port());
    assert!(serving(&https, &settings).await.is_err());
    server.stop().await;
}
