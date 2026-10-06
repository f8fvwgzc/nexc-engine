//! The knowledge base: uploading documents, ingesting them, searching them
//! and handing passages to prompts.
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode};
use common::TestApp;
use nexc::engine::knowledge::{self, Use};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const HANDBOOK: &str = "# Warehouse handbook

General rules for everyone on the floor.

## Customs

Invoices from the Rotterdam warehouse need a customs reference before they are released.
The reference is issued by the port authority and starts with NL.

## Plants

Office plants are watered on Mondays.
";

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

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn documents_are_ingested_searched_and_cited(pool: PgPool) {
    // No agent runtime in tests: a port nothing listens on.
    let app = TestApp::new(pool, &[("NEXC_RUNTIME_URL", "http://127.0.0.1:9")]).await;
    tokio::fs::create_dir_all(app.state.settings.documents_dir())
        .await
        .unwrap();
    let (owner, _) = user(&app, "owner@example.com").await;
    let (member, _) = user(&app, "member@example.com").await;
    let (guest, _) = user(&app, "guest@example.com").await;
    let (outsider, _) = user(&app, "outsider@example.com").await;
    let (_, list) = call(&app, Method::GET, "/workspaces", &owner, None).await;
    let wid = list[0]["id"].as_str().unwrap().to_owned();
    let workspace: Uuid = wid.parse().unwrap();
    let ws = format!("/workspaces/{wid}");
    for (email, role) in [
        ("member@example.com", "member"),
        ("guest@example.com", "guest"),
    ] {
        let body = json!({"email": email, "role": role});
        call(
            &app,
            Method::POST,
            &format!("{ws}/members"),
            &owner,
            Some(body),
        )
        .await;
    }
    let docs = format!("{ws}/documents");
    let upload = format!("/api/v1{docs}?name=");

    // Members add documents; guests and outsiders do not. The name is a name, not a path.
    let path = format!("{upload}..%2F..%2Fhandbook.md");
    let r = app.send_bytes(&path, &guest, HANDBOOK.into()).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = app.send_bytes(&path, &outsider, HANDBOOK.into()).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = app.send_bytes(&path, &member, HANDBOOK.into()).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let doc = r.body;
    assert_eq!(
        (doc["name"].as_str(), doc["status"].as_str()),
        (Some("handbook.md"), Some("pending"))
    );
    let did = doc["id"].as_str().unwrap().to_owned();
    // The same bytes again are the same document, whoever sends them.
    let again = app
        .send_bytes(&format!("{upload}copy.md"), &owner, HANDBOOK.into())
        .await;
    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(again.body["id"], doc["id"]);
    let empty = app
        .send_bytes(&format!("{upload}e.md"), &member, vec![])
        .await;
    assert_eq!(empty.status, StatusCode::UNPROCESSABLE_ENTITY);

    // Nothing is searchable until the worker has been round.
    let search =
        format!("{ws}/knowledge/search?q=customs%20reference%20for%20Rotterdam%20invoices");
    let (status, found) = call(&app, Method::GET, &search, &member, None).await;
    assert_eq!(
        (status, found.as_array().unwrap().len()),
        (StatusCode::OK, 0)
    );

    // A file that needs the runtime fails with a reason, and does not stop the others.
    let pdf = app
        .send_bytes(
            &format!("{upload}scan.pdf"),
            &member,
            b"%PDF-1.4 not really".to_vec(),
        )
        .await;
    assert_eq!(pdf.status, StatusCode::CREATED);
    knowledge::work(&app.state).await.unwrap();
    let one = format!("{docs}/{did}");
    let (_, ready) = call(&app, Method::GET, &one, &member, None).await;
    assert_eq!(ready["status"], "ready", "{ready}");
    assert_eq!(ready["chunk_count"], 3, "one passage per section");
    let failed = format!("{docs}/{}", pdf.body["id"].as_str().unwrap());
    let (_, failed) = call(&app, Method::GET, &failed, &member, None).await;
    assert_eq!(failed["status"], "failed");
    assert!(
        failed["error"].as_str().unwrap().contains("agent runtime"),
        "{failed}"
    );

    // Search returns the passage with where it sits, best first.
    let (_, found) = call(&app, Method::GET, &search, &member, None).await;
    let top = &found[0];
    assert_eq!(
        (
            top["document_name"].as_str(),
            top["section_path"].as_str(),
            top["kind"].as_str()
        ),
        (
            Some("handbook.md"),
            Some("Warehouse handbook › Customs"),
            Some("text")
        ),
        "{found}"
    );
    assert!(
        top["content"]
            .as_str()
            .unwrap()
            .contains("customs reference")
    );
    assert!(top["score"].as_f64().unwrap() > 0.0);
    let (status, _) = call(&app, Method::GET, &search, &guest, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (_, nothing) = call(
        &app,
        Method::GET,
        &format!("{ws}/knowledge/search?q=zeppelin%20maintenance"),
        &member,
        None,
    )
    .await;
    assert_eq!(nothing.as_array().unwrap().len(), 0);

    // A prompt gets passages headed by their citation, within the workspace's settings.
    let query = "customs reference for Rotterdam invoices";
    let context = knowledge::context(&app.state, workspace, query, Use::Node).await;
    assert!(
        context[0].starts_with("[handbook.md › Warehouse handbook › Customs]\n"),
        "{context:?}"
    );

    // Settings: the built-in embedding by default; only admins change them.
    let settings = format!("{ws}/knowledge/settings");
    let (_, current) = call(&app, Method::GET, &settings, &member, None).await;
    assert_eq!(
        (
            current["embed_model"].as_str(),
            current["semantic"].as_bool(),
            current["passages"].as_i64()
        ),
        (Some("builtin-hash-256"), Some(false), Some(5))
    );
    let off =
        json!({"passages": 0, "budget_chars": 6000, "use_in_nodes": true, "use_in_plan": false});
    let (status, _) = call(&app, Method::PUT, &settings, &member, Some(off.clone())).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let bad = json!({"passages": 99, "budget_chars": 6000, "use_in_nodes": true, "use_in_plan": true,
        "embed_base_url": "ftp://x"});
    let (status, _) = call(&app, Method::PUT, &settings, &owner, Some(bad)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, saved) = call(&app, Method::PUT, &settings, &owner, Some(off)).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["use_in_plan"], false);
    assert!(
        knowledge::context(&app.state, workspace, query, Use::Node)
            .await
            .is_empty(),
        "zero passages keeps documents out of prompts"
    );

    // Listing pages and filters by name; removing is for the uploader or an admin.
    let (_, all) = call(&app, Method::GET, &format!("{docs}?limit=1"), &member, None).await;
    assert_eq!(all.as_array().unwrap().len(), 1);
    let (_, named) = call(&app, Method::GET, &format!("{docs}?q=hand"), &member, None).await;
    assert_eq!(named.as_array().unwrap().len(), 1);
    let (third, _) = user(&app, "third@example.com").await;
    let body = json!({"email": "third@example.com", "role": "member"});
    call(
        &app,
        Method::POST,
        &format!("{ws}/members"),
        &owner,
        Some(body),
    )
    .await;
    let (status, _) = call(&app, Method::DELETE, &one, &third, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&app, Method::DELETE, &one, &member, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, found) = call(&app, Method::GET, &search, &member, None).await;
    assert_eq!(
        found.as_array().unwrap().len(),
        0,
        "its passages went with it"
    );
    assert!(
        !app.state.settings.documents_dir().join(&did).exists(),
        "and so did the file"
    );
}
