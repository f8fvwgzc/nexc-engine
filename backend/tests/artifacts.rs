//! Artifacts kept by hand: folders, text files with every version, the files
//! of an issue, and keeping what the runs of an issue's graph produced.
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode, header};
use common::{Resp, TestApp};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

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

/// Sends a file's bytes as the body (an upload, or a new version with PUT).
async fn send(app: &TestApp, method: Method, path: &str, token: &str, bytes: &[u8]) -> Resp {
    app.send_raw(method, &format!("/api/v1{path}"), token, bytes.to_vec())
        .await
}

/// A workspace with a public and a private team, a member, a guest and an
/// outsider with a workspace of their own.
struct World {
    ws: String,
    eng: String,
    secret: String,
    owner: String,
    member: String,
    member_id: String,
    guest: String,
    guest_id: String,
    outsider: String,
}

impl World {
    /// `POST {ws}/artifact-files` with the query given.
    fn upload(&self, query: &str) -> String {
        format!("{}/artifact-files?{query}", self.ws)
    }
}

async fn world(app: &TestApp) -> World {
    let (owner, _) = user(app, "owner@example.com").await;
    let (member, member_id) = user(app, "member@example.com").await;
    let (guest, guest_id) = user(app, "guest@example.com").await;
    let (outsider, _) = user(app, "outsider@example.com").await;
    let (_, list) = call(app, Method::GET, "/workspaces", &owner, None).await;
    let wid = list[0]["id"].as_str().unwrap().to_owned();
    let ws = format!("/workspaces/{wid}");
    for (email, role) in [
        ("member@example.com", "member"),
        ("guest@example.com", "guest"),
    ] {
        let (status, _) = call(
            app,
            Method::POST,
            &format!("{ws}/members"),
            &owner,
            Some(json!({"email": email, "role": role})),
        )
        .await;
        assert!(status.is_success());
    }
    let mut teams = Vec::new();
    for body in [
        json!({"name": "Engineering", "key": "ENG"}),
        json!({"name": "Leadership", "key": "LEAD", "private": true}),
    ] {
        let (status, team) = call(
            app,
            Method::POST,
            &format!("{ws}/teams"),
            &owner,
            Some(body),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{team}");
        teams.push(team["id"].as_str().unwrap().to_owned());
    }
    let secret = teams.pop().unwrap();
    let eng = teams.pop().unwrap();
    World {
        ws,
        eng,
        secret,
        owner,
        member,
        member_id,
        guest,
        guest_id,
        outsider,
    }
}

/// Files an issue with a team; returns its id.
async fn issue(app: &TestApp, w: &World, team: &str, token: &str, title: &str) -> String {
    let (status, issue) = call(
        app,
        Method::POST,
        &format!("{}/teams/{team}/issues", w.ws),
        token,
        Some(json!({"title": title})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{issue}");
    issue["id"].as_str().unwrap().to_owned()
}

/// Creates a folder as `token`; returns its id.
async fn folder(app: &TestApp, w: &World, token: &str, body: Value) -> String {
    let (status, folder) = call(
        app,
        Method::POST,
        &format!("{}/artifact-folders", w.ws),
        token,
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{folder}");
    folder["id"].as_str().unwrap().to_owned()
}

/// Uploads a file as `token`; returns it.
async fn upload(app: &TestApp, w: &World, token: &str, query: &str, bytes: &[u8]) -> Value {
    let r = send(app, Method::POST, &w.upload(query), token, bytes).await;
    assert_eq!(r.status, StatusCode::CREATED, "{query}: {}", r.body);
    r.body
}

/// The kinds and bodies of an issue's `artifact` timeline entries, in order.
async fn file_events(app: &TestApp, token: &str, iid: &str) -> Vec<(String, Value, String)> {
    let (status, events) = call(
        app,
        Method::GET,
        &format!("/issues/{iid}/events"),
        token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{events}");
    events
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "artifact")
        .map(|e| {
            (
                e["body"].as_str().unwrap().to_owned(),
                e["from"].clone(),
                e["to"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn names(list: &Value) -> Vec<&str> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect()
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn folders_nest_and_never_move_into_themselves(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let folders = format!("{}/artifact-folders", w.ws);
    let specs = folder(&app, &w, &w.member, json!({"name": " Specs "})).await;
    let api = folder(
        &app,
        &w,
        &w.member,
        json!({"name": "API", "parent_id": specs}),
    )
    .await;

    // Names are unique among siblings, ignoring case, and one segment of printable text.
    let (status, _) = call(
        &app,
        Method::POST,
        &folders,
        &w.owner,
        Some(json!({"name": "specs"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = call(
        &app,
        Method::POST,
        &folders,
        &w.owner,
        Some(json!({"name": "API", "parent_id": api})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the same name elsewhere is fine"
    );
    for bad in [
        json!({"name": "a/b"}),
        json!({"name": ""}),
        json!({"name": "x".repeat(121)}),
    ] {
        let (status, body) = call(&app, Method::POST, &folders, &w.member, Some(bad)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert!(body["errors"]["name"].is_array(), "{body}");
    }
    let nowhere = uuid::Uuid::now_v7();
    let (status, body) = call(
        &app,
        Method::POST,
        &folders,
        &w.member,
        Some(json!({"name": "Lost", "parent_id": nowhere})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["errors"]["parent_id"].is_array(), "{body}");

    // The list is flat, by name; guests see no folders and outsiders no workspace.
    let (_, list) = call(&app, Method::GET, &folders, &w.member, None).await;
    assert_eq!(names(&list), ["API", "API", "Specs"]);
    let nested = list
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == api.as_str())
        .unwrap();
    assert_eq!(nested["parent_id"], specs.as_str());
    let (status, list) = call(&app, Method::GET, &folders, &w.guest, None).await;
    assert_eq!((status, list), (StatusCode::OK, json!([])));
    let (status, _) = call(&app, Method::GET, &folders, &w.outsider, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &app,
        Method::POST,
        &folders,
        &w.guest,
        Some(json!({"name": "Mine"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // A folder never moves into itself or a folder under it.
    for parent in [&api, &specs] {
        let (status, body) = call(
            &app,
            Method::PATCH,
            &format!("{folders}/{specs}"),
            &w.member,
            Some(json!({"parent_id": parent})),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert!(body["errors"]["parent_id"].is_array());
    }
    let (status, moved) = call(
        &app,
        Method::PATCH,
        &format!("{folders}/{api}"),
        &w.member,
        Some(json!({"name": "API v2", "parent_id": null})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(
        (moved["name"].as_str(), &moved["parent_id"]),
        (Some("API v2"), &Value::Null)
    );
    let (status, _) = call(
        &app,
        Method::PATCH,
        &format!("{folders}/{api}"),
        &w.member,
        Some(json!({"name": "SPECS"})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "Specs is at the top level too now"
    );
    let (status, _) = call(
        &app,
        Method::PATCH,
        &format!("{folders}/{api}"),
        &w.outsider,
        Some(json!({"name": "Mine"})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn text_files_are_uploaded_and_anything_else_refused(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let specs = folder(&app, &w, &w.member, json!({"name": "Specs"})).await;

    let notes = upload(
        &app,
        &w,
        &w.member,
        &format!("name=notes.md&folder_id={specs}"),
        b"# Notes\n",
    )
    .await;
    assert_eq!(notes["name"], "notes.md");
    assert_eq!(
        (&notes["extension"], &notes["kind"], &notes["language"]),
        (&json!("md"), &json!("markdown"), &json!("markdown"))
    );
    assert_eq!(
        (&notes["current_version"], &notes["size"]),
        (&json!(1), &json!(8))
    );
    assert_eq!(notes["sha256"], hex::encode(Sha256::digest(b"# Notes\n")));
    assert_eq!(notes["folder_id"], specs.as_str());
    assert_eq!(
        (&notes["issue"], &notes["duplicated_from"]),
        (&Value::Null, &Value::Null)
    );
    assert_eq!(notes["created_by"]["user_id"], w.member_id.as_str());
    assert_eq!(notes["updated_by"]["user_id"], w.member_id.as_str());

    // A name is unique in its folder, ignoring case; another folder may have it.
    let r = send(
        &app,
        Method::POST,
        &w.upload(&format!("name=NOTES.md&folder_id={specs}")),
        &w.owner,
        b"x",
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    upload(&app, &w, &w.owner, "name=notes.md", b"top level").await;

    // Code, HTML, Markdown, config and text only; an empty file is a file.
    let dockerfile = upload(&app, &w, &w.member, "name=Dockerfile", b"FROM scratch\n").await;
    assert_eq!(
        (
            &dockerfile["extension"],
            &dockerfile["kind"],
            &dockerfile["language"]
        ),
        (&json!(""), &json!("code"), &json!("dockerfile"))
    );
    let page = upload(
        &app,
        &w,
        &w.member,
        "name=index.html",
        b"<h1>Hi</h1><script>x()</script>",
    )
    .await;
    assert_eq!(page["kind"], "html");
    let empty = upload(&app, &w, &w.member, "name=empty.txt", b"").await;
    assert_eq!(
        (&empty["size"], &empty["kind"]),
        (&json!(0), &json!("text"))
    );
    let refused = [
        ("name=report.pdf", b"%PDF-1.7".to_vec()),
        ("name=main.rs", b"fn main() {}\0\0".to_vec()),
        ("name=data.json", vec![0xff, 0xfe, 0x7b]),
    ];
    for (query, bytes) in refused {
        let r = send(&app, Method::POST, &w.upload(query), &w.member, &bytes).await;
        assert_eq!(
            r.status,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "{query}: {}",
            r.body
        );
        assert_eq!(
            r.body["detail"],
            "Only text files (code, HTML, Markdown) can be artifacts"
        );
    }
    for query in ["name=a%2Fb.md", "name=", "name=%20%20"] {
        let r = send(&app, Method::POST, &w.upload(query), &w.member, b"x").await;
        assert_eq!(
            r.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{query}: {}",
            r.body
        );
    }
    // At most 1 MiB, exactly the request body limit.
    let full = vec![b'a'; 1_048_576];
    let r = send(
        &app,
        Method::POST,
        &w.upload("name=big.txt"),
        &w.member,
        &full,
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let over = vec![b'a'; 1_048_577];
    let r = send(
        &app,
        Method::POST,
        &w.upload("name=bigger.txt"),
        &w.member,
        &over,
    )
    .await;
    assert_eq!(r.status, StatusCode::PAYLOAD_TOO_LARGE);
    // Never a credential.
    let r = send(
        &app,
        Method::POST,
        &w.upload("name=config.env.example"),
        &w.member,
        b"OPENAI_API_KEY=sk-live0123456789abcdefghijklmnop\n",
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", r.body);
    assert!(
        r.body["detail"].as_str().unwrap().contains("credential"),
        "{}",
        r.body
    );
    // Unknown folders and issues, guests and outsiders.
    let nowhere = uuid::Uuid::now_v7();
    for query in [
        format!("name=x.md&folder_id={nowhere}"),
        format!("name=x.md&issue_id={nowhere}"),
    ] {
        let r = send(&app, Method::POST, &w.upload(&query), &w.member, b"x").await;
        assert_eq!(
            r.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{query}: {}",
            r.body
        );
    }
    let r = send(&app, Method::POST, &w.upload("name=x.md"), &w.guest, b"x").await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = send(
        &app,
        Method::POST,
        &w.upload("name=x.md"),
        &w.outsider,
        b"x",
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    // Lists: one folder, the top level by name, a search, or everything newest first.
    let files = format!("{}/artifact-files", w.ws);
    let (_, list) = call(
        &app,
        Method::GET,
        &format!("{files}?folder_id={specs}"),
        &w.member,
        None,
    )
    .await;
    assert_eq!(names(&list), ["notes.md"]);
    let (_, list) = call(
        &app,
        Method::GET,
        &format!("{files}?root=true"),
        &w.member,
        None,
    )
    .await;
    assert_eq!(
        names(&list),
        [
            "big.txt",
            "Dockerfile",
            "empty.txt",
            "index.html",
            "notes.md"
        ]
    );
    let (_, list) = call(
        &app,
        Method::GET,
        &format!("{files}?q=NOTE"),
        &w.member,
        None,
    )
    .await;
    assert_eq!(list.as_array().unwrap().len(), 2);
    let (_, list) = call(&app, Method::GET, &files, &w.member, None).await;
    assert_eq!(names(&list)[0], "big.txt", "the latest first");
    assert_eq!(list.as_array().unwrap().len(), 6);
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("{files}?root=true&folder_id={specs}"),
        &w.member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, list) = call(&app, Method::GET, &files, &w.guest, None).await;
    assert_eq!(
        (status, list),
        (StatusCode::OK, json!([])),
        "files without an issue are not a guest's"
    );
    let id = notes["id"].as_str().unwrap();
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/artifact-files/{id}"),
        &w.guest,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/artifact-files/{id}"),
        &w.outsider,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_file_keeps_every_version(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let notes = upload(&app, &w, &w.member, "name=notes.md", b"one").await;
    let file = format!("/artifact-files/{}", notes["id"].as_str().unwrap());

    let r = send(
        &app,
        Method::PUT,
        &format!("{file}/content?base_version=1"),
        &w.owner,
        b"two",
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(
        (&r.body["current_version"], &r.body["size"]),
        (&json!(2), &json!(3))
    );
    assert_eq!(r.body["updated_by"]["name"], "Test");
    assert_ne!(r.body["sha256"], notes["sha256"]);
    // Someone who still has version 1 open cannot overwrite version 2.
    let r = send(
        &app,
        Method::PUT,
        &format!("{file}/content?base_version=1"),
        &w.member,
        b"mine",
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert!(
        r.body["detail"].as_str().unwrap().contains("v2"),
        "{}",
        r.body
    );
    // The same text again saves nothing.
    let r = send(
        &app,
        Method::PUT,
        &format!("{file}/content?base_version=2"),
        &w.member,
        b"two",
    )
    .await;
    assert_eq!(
        (r.status, &r.body["current_version"]),
        (StatusCode::OK, &json!(2))
    );
    // What a version must be, as for an upload.
    let r = send(
        &app,
        Method::PUT,
        &format!("{file}/content?base_version=2"),
        &w.member,
        b"bin\0",
    )
    .await;
    assert_eq!(r.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let r = send(
        &app,
        Method::PUT,
        &format!("{file}/content?base_version=2"),
        &w.member,
        b"token: ghp_0123456789abcdefghijklmnopqrstuv",
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = send(
        &app,
        Method::PUT,
        &format!("{file}/content"),
        &w.member,
        b"three",
    )
    .await;
    assert_eq!(
        r.status,
        StatusCode::BAD_REQUEST,
        "base_version is required"
    );
    let r = send(
        &app,
        Method::PUT,
        &format!("{file}/content?base_version=2"),
        &w.guest,
        b"x",
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "not a guest's to see");

    // Versions, newest first, and the text of each.
    let (_, versions) = call(
        &app,
        Method::GET,
        &format!("{file}/versions"),
        &w.member,
        None,
    )
    .await;
    let numbers: Vec<i64> = versions
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["version"].as_i64().unwrap())
        .collect();
    assert_eq!(numbers, [2, 1]);
    assert_eq!(versions[1]["created_by"]["user_id"], w.member_id.as_str());
    assert_eq!(versions[1]["sha256"], notes["sha256"]);
    let (_, current) = call(
        &app,
        Method::GET,
        &format!("{file}/content"),
        &w.member,
        None,
    )
    .await;
    assert_eq!(
        (&current["version"], &current["content"]),
        (&json!(2), &json!("two"))
    );
    let (_, first) = call(
        &app,
        Method::GET,
        &format!("{file}/content?version=1"),
        &w.member,
        None,
    )
    .await;
    assert_eq!(first["content"], "one");
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("{file}/content?version=9"),
        &w.member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The raw text is a download, never a page.
    let r = app
        .request(
            Method::GET,
            &format!("/api/v1{file}/raw?version=1"),
            Some(&w.member),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.headers[header::CONTENT_TYPE], "text/plain; charset=utf-8");
    let disposition = r.headers[header::CONTENT_DISPOSITION].to_str().unwrap();
    assert!(
        disposition.starts_with("attachment; filename=\"notes.md\""),
        "{disposition}"
    );
    assert_eq!(r.headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(r.body, json!("one"));
    let r = app
        .request(
            Method::GET,
            &format!("/api/v1{file}/raw"),
            Some(&w.member),
            None,
        )
        .await;
    assert_eq!(r.body, json!("two"));
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn files_are_copied_moved_renamed_and_deleted(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let specs = folder(&app, &w, &w.member, json!({"name": "Specs"})).await;
    let notes = upload(
        &app,
        &w,
        &w.member,
        &format!("name=notes.md&folder_id={specs}"),
        b"text",
    )
    .await;
    let id = notes["id"].as_str().unwrap().to_owned();
    let file = format!("/artifact-files/{id}");

    // Copies are named before the extension and remember where they came from.
    let mut copies = Vec::new();
    for expected in ["notes copy.md", "notes copy 2.md"] {
        let (status, copy) = call(
            &app,
            Method::POST,
            &format!("{file}/duplicate"),
            &w.member,
            Some(json!({})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{copy}");
        assert_eq!(copy["name"], expected);
        assert_eq!(
            (&copy["folder_id"], &copy["duplicated_from"]),
            (&json!(specs), &json!(id))
        );
        assert_eq!(
            (&copy["current_version"], &copy["sha256"]),
            (&json!(1), &notes["sha256"])
        );
        copies.push(copy["id"].as_str().unwrap().to_owned());
    }
    let (status, plan) = call(
        &app,
        Method::POST,
        &format!("{file}/duplicate"),
        &w.member,
        Some(json!({"folder_id": null, "name": "plan.md"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{plan}");
    assert_eq!(plan["folder_id"], Value::Null);
    let (_, text) = call(
        &app,
        Method::GET,
        &format!("/artifact-files/{}/content", plan["id"].as_str().unwrap()),
        &w.member,
        None,
    )
    .await;
    assert_eq!(text["content"], "text");
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("{file}/duplicate"),
        &w.member,
        Some(json!({"name": "notes copy.md"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Renames keep a text file's extension and a free name; moves need a folder of the workspace.
    let (status, renamed) = call(
        &app,
        Method::PATCH,
        &file,
        &w.member,
        Some(json!({"name": "spec.md"})),
    )
    .await;
    assert_eq!(
        (status, renamed["name"].as_str()),
        (StatusCode::OK, Some("spec.md"))
    );
    assert_eq!(renamed["current_version"], 1, "a rename is not a version");
    let (status, _) = call(
        &app,
        Method::PATCH,
        &file,
        &w.member,
        Some(json!({"name": "spec.exe"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let (status, _) = call(
        &app,
        Method::PATCH,
        &file,
        &w.member,
        Some(json!({"name": "Notes Copy.md"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = call(
        &app,
        Method::PATCH,
        &file,
        &w.member,
        Some(json!({"name": "a/b.md"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = call(
        &app,
        Method::PATCH,
        &file,
        &w.member,
        Some(json!({"folder_id": uuid::Uuid::now_v7()})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, moved) = call(
        &app,
        Method::PATCH,
        &file,
        &w.member,
        Some(json!({"folder_id": null})),
    )
    .await;
    assert_eq!(
        (status, &moved["folder_id"]),
        (StatusCode::OK, &Value::Null)
    );

    // Someone outside the workspace finds the file nowhere and changes nothing.
    for (method, path, body) in [
        (Method::GET, file.clone(), None),
        (Method::GET, format!("{file}/versions"), None),
        (Method::GET, format!("{file}/content"), None),
        (Method::GET, format!("{file}/content?version=1"), None),
        (Method::GET, format!("{file}/raw"), None),
        (
            Method::PATCH,
            file.clone(),
            Some(json!({"name": "theirs.md"})),
        ),
        (Method::POST, format!("{file}/duplicate"), Some(json!({}))),
        (Method::DELETE, file.clone(), None),
    ] {
        let (status, _) = call(&app, method.clone(), &path, &w.outsider, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
    }
    let r = send(
        &app,
        Method::PUT,
        &format!("{file}/content?base_version=1"),
        &w.outsider,
        b"theirs",
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let (_, unchanged) = call(&app, Method::GET, &file, &w.member, None).await;
    assert_eq!(
        (&unchanged["name"], &unchanged["current_version"]),
        (&json!("spec.md"), &json!(1))
    );

    // Deleting a file takes its versions; its copies stay, no longer pointing at it.
    let (status, _) = call(&app, Method::DELETE, &file, &w.member, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(&app, Method::GET, &file, &w.member, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, copy) = call(
        &app,
        Method::GET,
        &format!("/artifact-files/{}", copies[0]),
        &w.member,
        None,
    )
    .await;
    assert_eq!(copy["duplicated_from"], Value::Null);
    let versions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM artifact_file_versions WHERE file_id = $1::uuid")
            .bind(&id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(versions, 0);
    let (_, log) = call(
        &app,
        Method::GET,
        &format!("{}/audit", w.ws),
        &w.owner,
        None,
    )
    .await;
    let entry = log
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["action"] == "artifact_file_deleted")
        .expect("the delete is audited");
    assert_eq!(
        (&entry["subject"], &entry["detail"]),
        (&json!("spec.md"), &json!("v1"))
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn deleting_a_folder_takes_everything_under_it(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let folders = format!("{}/artifact-folders", w.ws);
    let docs = folder(&app, &w, &w.member, json!({"name": "Docs"})).await;
    let inner = folder(
        &app,
        &w,
        &w.member,
        json!({"name": "Inner", "parent_id": docs}),
    )
    .await;
    let keep = folder(&app, &w, &w.member, json!({"name": "Keep"})).await;
    let a = upload(
        &app,
        &w,
        &w.member,
        &format!("name=a.md&folder_id={docs}"),
        b"a",
    )
    .await;
    let b = upload(
        &app,
        &w,
        &w.member,
        &format!("name=b.md&folder_id={inner}"),
        b"b",
    )
    .await;
    let c = upload(
        &app,
        &w,
        &w.member,
        &format!("name=c.md&folder_id={keep}"),
        b"c",
    )
    .await;
    // A file of the private team's issue, which the member cannot see.
    let reorg = issue(&app, &w, &w.secret, &w.owner, "Reorg").await;
    upload(
        &app,
        &w,
        &w.owner,
        &format!("name=plan.md&folder_id={inner}&issue_id={reorg}"),
        b"p",
    )
    .await;

    let (status, body) = call(
        &app,
        Method::DELETE,
        &format!("{folders}/{docs}"),
        &w.member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body["detail"],
        "this folder holds files of issues you cannot see; a workspace admin can delete it",
        "how many is not said"
    );
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("{folders}/{docs}"),
        &w.guest,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // The owner sees everything: the folder goes with what is under it.
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("{folders}/{docs}"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, list) = call(&app, Method::GET, &folders, &w.member, None).await;
    assert_eq!(names(&list), ["Keep"]);
    for (file, gone) in [(&a, true), (&b, true), (&c, false)] {
        let (status, _) = call(
            &app,
            Method::GET,
            &format!("/artifact-files/{}", file["id"].as_str().unwrap()),
            &w.member,
            None,
        )
        .await;
        assert_eq!(status == StatusCode::NOT_FOUND, gone, "{}", file["name"]);
    }
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM artifact_file_versions")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(left, 1, "only c.md's version is left");
    // The issue notes that its file went.
    assert_eq!(
        file_events(&app, &w.owner, &reorg).await.last().unwrap(),
        &("plan.md".to_owned(), json!("1"), "deleted".to_owned())
    );
    let (_, log) = call(
        &app,
        Method::GET,
        &format!("{}/audit", w.ws),
        &w.owner,
        None,
    )
    .await;
    assert!(
        log.as_array()
            .unwrap()
            .iter()
            .any(|e| e["action"] == "artifact_folder_deleted" && e["subject"] == "Docs"),
        "{log}"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_files_of_an_issue_are_seen_as_the_issue_is(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let iid = issue(&app, &w, &w.eng, &w.member, "Write the spec").await;
    let notes = upload(
        &app,
        &w,
        &w.member,
        &format!("name=notes.md&issue_id={iid}"),
        b"v1",
    )
    .await;
    assert_eq!(
        (
            &notes["issue"]["id"],
            &notes["issue"]["identifier"],
            &notes["issue"]["title"]
        ),
        (&json!(iid), &json!("ENG-1"), &json!("Write the spec"))
    );
    let file = format!("/artifact-files/{}", notes["id"].as_str().unwrap());
    let r = send(
        &app,
        Method::PUT,
        &format!("{file}/content?base_version=1"),
        &w.member,
        b"v2",
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let (status, copy) = call(
        &app,
        Method::POST,
        &format!("{file}/duplicate"),
        &w.member,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        copy["issue"]["id"],
        iid.as_str(),
        "a copy stays on the issue"
    );
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/artifact-files/{}", copy["id"].as_str().unwrap()),
        &w.member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // Each of it is in the issue's timeline.
    assert_eq!(
        file_events(&app, &w.member, &iid).await,
        [
            ("notes.md".to_owned(), Value::Null, "uploaded".to_owned()),
            ("notes.md".to_owned(), json!("1"), "updated:v2".to_owned()),
            ("notes.md".to_owned(), Value::Null, "duplicated".to_owned()),
            ("notes copy.md".to_owned(), json!("1"), "deleted".to_owned()),
        ]
    );
    let (_, list) = call(
        &app,
        Method::GET,
        &format!("{}/artifact-files?issue_id={iid}", w.ws),
        &w.member,
        None,
    )
    .await;
    assert_eq!(names(&list), ["notes.md"]);

    // A guest on the issue's team reads its files, and only reads them.
    let join = format!("{}/teams/{}/members/{}", w.ws, w.eng, w.guest_id);
    let (status, _) = call(&app, Method::PUT, &join, &w.owner, Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK);
    let (status, seen) = call(&app, Method::GET, &file, &w.guest, None).await;
    assert_eq!(
        (status, &seen["name"]),
        (StatusCode::OK, &json!("notes.md"))
    );
    let (_, text) = call(
        &app,
        Method::GET,
        &format!("{file}/content"),
        &w.guest,
        None,
    )
    .await;
    assert_eq!(text["content"], "v2");
    let (_, list) = call(
        &app,
        Method::GET,
        &format!("{}/artifact-files", w.ws),
        &w.guest,
        None,
    )
    .await;
    assert_eq!(
        names(&list),
        ["notes.md"],
        "only the files of issues the guest sees"
    );
    let r = send(
        &app,
        Method::PUT,
        &format!("{file}/content?base_version=2"),
        &w.guest,
        b"v3",
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    for (method, path, body) in [
        (
            Method::PATCH,
            file.clone(),
            Some(json!({"name": "mine.md"})),
        ),
        (Method::POST, format!("{file}/duplicate"), Some(json!({}))),
        (Method::DELETE, file.clone(), None),
    ] {
        let (status, _) = call(&app, method.clone(), &path, &w.guest, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
    }
    let r = send(
        &app,
        Method::POST,
        &w.upload(&format!("name=x.md&issue_id={iid}")),
        &w.guest,
        b"x",
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // The private team's files are its own (and the workspace admins').
    let reorg = issue(&app, &w, &w.secret, &w.owner, "Reorg").await;
    let plan = upload(
        &app,
        &w,
        &w.owner,
        &format!("name=plan.md&issue_id={reorg}"),
        b"secret plan",
    )
    .await;
    let plan_path = format!("/artifact-files/{}", plan["id"].as_str().unwrap());
    let (status, _) = call(&app, Method::GET, &plan_path, &w.member, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, list) = call(
        &app,
        Method::GET,
        &format!("{}/artifact-files", w.ws),
        &w.member,
        None,
    )
    .await;
    assert!(!names(&list).contains(&"plan.md"));
    let r = send(
        &app,
        Method::POST,
        &w.upload(&format!("name=x.md&issue_id={reorg}")),
        &w.member,
        b"x",
    )
    .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "an issue the member cannot see"
    );
    let (status, _) = call(
        &app,
        Method::PATCH,
        &file,
        &w.member,
        Some(json!({"issue_id": reorg})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Detached, a file is the workspace's.
    let (status, detached) = call(
        &app,
        Method::PATCH,
        &file,
        &w.member,
        Some(json!({"issue_id": null})),
    )
    .await;
    assert_eq!((status, &detached["issue"]), (StatusCode::OK, &Value::Null));
    let (status, _) = call(&app, Method::GET, &file, &w.guest, None).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "no longer the guest's to see"
    );

    // A deleted issue of a public team leaves its files to the workspace; one of a private team
    // takes them along, as does a deleted private team, so they never fall to everyone.
    let spec = upload(
        &app,
        &w,
        &w.member,
        &format!("name=spec.md&issue_id={iid}"),
        b"spec",
    )
    .await;
    let spec_path = format!("/artifact-files/{}", spec["id"].as_str().unwrap());
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/issues/{iid}"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, kept) = call(&app, Method::GET, &spec_path, &w.member, None).await;
    assert_eq!((status, &kept["issue"]), (StatusCode::OK, &Value::Null));
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/issues/{reorg}"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    for token in [&w.member, &w.owner] {
        let (status, _) = call(&app, Method::GET, &plan_path, token, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "the private file went");
    }
    let budget = issue(&app, &w, &w.secret, &w.owner, "Budget").await;
    let sheet = upload(
        &app,
        &w,
        &w.owner,
        &format!("name=budget.csv&issue_id={budget}"),
        b"q4,1",
    )
    .await;
    let sheet_path = format!("/artifact-files/{}", sheet["id"].as_str().unwrap());
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("{}/teams/{}", w.ws, w.secret),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    for token in [&w.member, &w.owner] {
        let (status, _) = call(&app, Method::GET, &sheet_path, token, None).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "the private team's file went"
        );
    }
    let (status, _) = call(&app, Method::GET, &spec_path, &w.member, None).await;
    assert_eq!(status, StatusCode::OK, "the public file stays");

    // A deleted public team leaves its issues' files to the workspace.
    let (status, ops) = call(
        &app,
        Method::POST,
        &format!("{}/teams", w.ws),
        &w.owner,
        Some(json!({"name": "Operations", "key": "OPS"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{ops}");
    let ops = ops["id"].as_str().unwrap().to_owned();
    let runbook = issue(&app, &w, &ops, &w.owner, "Runbook").await;
    let book = upload(
        &app,
        &w,
        &w.owner,
        &format!("name=runbook.md&issue_id={runbook}"),
        b"restart it",
    )
    .await;
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("{}/teams/{ops}", w.ws),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, kept) = call(
        &app,
        Method::GET,
        &format!("/artifact-files/{}", book["id"].as_str().unwrap()),
        &w.member,
        None,
    )
    .await;
    assert_eq!((status, &kept["issue"]), (StatusCode::OK, &Value::Null));
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn what_runs_produced_is_listed_and_kept_on_the_issue(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let iid = issue(&app, &w, &w.eng, &w.owner, "Write the gold note").await;
    let runs = format!("/issues/{iid}/run-artifacts");
    let (status, list) = call(&app, Method::GET, &runs, &w.member, None).await;
    assert_eq!(
        (status, list),
        (StatusCode::OK, json!([])),
        "no graph, nothing yet"
    );
    let (status, _) = call(&app, Method::GET, &runs, &w.outsider, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The member joins the team, plans the issue as a graph and runs it: the draft is a file.
    let join = format!("{}/teams/{}/members/{}", w.ws, w.eng, w.member_id);
    call(&app, Method::PUT, &join, &w.owner, Some(json!({}))).await;
    let (status, linked) = call(
        &app,
        Method::POST,
        &format!("/issues/{iid}/graph"),
        &w.member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{linked}");
    let gid = linked["graph_id"].as_str().unwrap().to_owned();
    let research = app
        .node(
            &w.member,
            &gid,
            json!({"title": "Research", "kind": "research"}),
        )
        .await;
    let draft = app
        .node(
            &w.member,
            &gid,
            json!({"title": "Draft", "kind": "document"}),
        )
        .await;
    app.edge(&w.member, &gid, &research, &draft).await;
    let mut rx = app.events(&gid);
    let r = app
        .request(
            Method::POST,
            &format!("/api/v1/graphs/{gid}/runs"),
            Some(&w.member),
            Some(json!({})),
        )
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
    let run = common::collect_until(&mut rx, "run.finished")
        .await
        .last()
        .unwrap()
        .1["run"]
        .clone();
    assert_eq!(run["status"], "succeeded");

    let (_, list) = call(&app, Method::GET, &runs, &w.member, None).await;
    let produced = list.as_array().unwrap();
    assert_eq!(produced.len(), 1, "{list}");
    let made = &produced[0];
    assert_eq!(
        (&made["path"], &made["run_id"], &made["run_status"]),
        (&json!("draft.md"), &run["id"], &json!("succeeded"))
    );
    assert!(made["run_created_at"].is_string() && made["mime"].is_string());
    let aid = made["id"].as_str().unwrap().to_owned();
    let keep = format!("{runs}/{aid}/keep");

    // Kept, it is one of the issue's files; kept again, a copy.
    let (status, kept) = call(&app, Method::POST, &keep, &w.member, Some(json!({}))).await;
    assert_eq!(status, StatusCode::CREATED, "{kept}");
    assert_eq!(
        (&kept["name"], &kept["issue"]["id"]),
        (&json!("draft.md"), &json!(iid))
    );
    let (_, text) = call(
        &app,
        Method::GET,
        &format!("/artifact-files/{}/content", kept["id"].as_str().unwrap()),
        &w.member,
        None,
    )
    .await;
    assert_eq!(text["content"], "Result of Draft (after: Research)");
    let specs = folder(&app, &w, &w.member, json!({"name": "Specs"})).await;
    let (status, again) = call(
        &app,
        Method::POST,
        &keep,
        &w.member,
        Some(json!({"folder_id": specs})),
    )
    .await;
    assert_eq!(
        (status, &again["name"], &again["folder_id"]),
        (StatusCode::CREATED, &json!("draft.md"), &json!(specs))
    );
    let (_, again) = call(&app, Method::POST, &keep, &w.member, Some(json!({}))).await;
    assert_eq!(again["name"], "draft copy.md");
    assert_eq!(
        file_events(&app, &w.member, &iid).await[0],
        ("draft.md".to_owned(), Value::Null, "uploaded".to_owned())
    );

    // Only what the issue's runs produced, never for guests or outsiders.
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("{runs}/{}/keep", uuid::Uuid::now_v7()),
        &w.member,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&app, Method::POST, &keep, &w.guest, Some(json!({}))).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the guest cannot see the issue"
    );
    let (status, _) = call(&app, Method::POST, &keep, &w.outsider, Some(json!({}))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let other = issue(&app, &w, &w.eng, &w.owner, "Other").await;
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/issues/{other}/run-artifacts/{aid}/keep"),
        &w.member,
        Some(json!({})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "not a file of that issue's runs"
    );

    // Quarantined, too large or not text: not kept.
    let db = &app.state.db;
    sqlx::query("UPDATE artifacts SET quarantined_at = now(), quarantine_reason = 'dlp' WHERE id = $1::uuid")
        .bind(&aid)
        .execute(db)
        .await
        .unwrap();
    let (status, _) = call(&app, Method::POST, &keep, &w.member, Some(json!({}))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    sqlx::query("UPDATE artifacts SET released_at = now(), size = 2000000 WHERE id = $1::uuid")
        .bind(&aid)
        .execute(db)
        .await
        .unwrap();
    let (status, _) = call(&app, Method::POST, &keep, &w.member, Some(json!({}))).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    let storage: String = sqlx::query_scalar(
        "UPDATE artifacts SET size = 4 WHERE id = $1::uuid RETURNING storage_path",
    )
    .bind(&aid)
    .fetch_one(db)
    .await
    .unwrap();
    std::fs::write(
        app.state.settings.artifacts_dir().join(storage),
        b"\0\x01\x02\x03",
    )
    .unwrap();
    let (status, _) = call(&app, Method::POST, &keep, &w.member, Some(json!({}))).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn names_are_unique_among_the_files_of_one_issue_in_a_folder(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let one = issue(&app, &w, &w.eng, &w.member, "One").await;
    let two = issue(&app, &w, &w.eng, &w.member, "Two").await;

    // Two issues each keep a notes.md at the top level, and so do the files of no issue.
    let first = upload(
        &app,
        &w,
        &w.member,
        &format!("name=notes.md&issue_id={one}"),
        b"1",
    )
    .await;
    let second = upload(
        &app,
        &w,
        &w.member,
        &format!("name=notes.md&issue_id={two}"),
        b"2",
    )
    .await;
    upload(&app, &w, &w.member, "name=notes.md", b"workspace").await;
    // The same issue, or no issue, twice is a 409.
    for (query, detail) in [
        (
            format!("name=NOTES.md&issue_id={one}"),
            "the issue already has a file with that name in this folder",
        ),
        (
            "name=Notes.md".to_owned(),
            "a file with that name is already in this folder",
        ),
    ] {
        let r = send(&app, Method::POST, &w.upload(&query), &w.member, b"x").await;
        assert_eq!(r.status, StatusCode::CONFLICT, "{query}: {}", r.body);
        assert_eq!(r.body["detail"], detail);
    }
    // Moving a file to another issue, or off any, checks the names there.
    let second_path = format!("/artifact-files/{}", second["id"].as_str().unwrap());
    for issue_id in [json!(one), Value::Null] {
        let (status, body) = call(
            &app,
            Method::PATCH,
            &second_path,
            &w.member,
            Some(json!({"issue_id": issue_id})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{issue_id}: {body}");
    }
    let (status, moved) = call(
        &app,
        Method::PATCH,
        &second_path,
        &w.member,
        Some(json!({"name": "todo.md", "issue_id": one})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["issue"]["id"], one.as_str());
    // So does moving one to another folder.
    let drafts = folder(&app, &w, &w.member, json!({"name": "Drafts"})).await;
    let other = upload(
        &app,
        &w,
        &w.member,
        &format!("name=notes.md&folder_id={drafts}"),
        b"draft",
    )
    .await;
    let (status, body) = call(
        &app,
        Method::PATCH,
        &format!("/artifact-files/{}", other["id"].as_str().unwrap()),
        &w.member,
        Some(json!({"folder_id": null})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    // A file of a private team's issue takes no name from someone who cannot see it: not on an
    // upload, a rename or a copy's name.
    let reorg = issue(&app, &w, &w.secret, &w.owner, "Reorg").await;
    let specs = folder(&app, &w, &w.owner, json!({"name": "Specs"})).await;
    for name in ["plan.md", "plan%20copy.md", "roadmap.md"] {
        upload(
            &app,
            &w,
            &w.owner,
            &format!("name={name}&folder_id={specs}&issue_id={reorg}"),
            b"secret",
        )
        .await;
    }
    let mine = upload(
        &app,
        &w,
        &w.member,
        &format!("name=PLAN.md&folder_id={specs}"),
        b"mine",
    )
    .await;
    let (status, copy) = call(
        &app,
        Method::POST,
        &format!("/artifact-files/{}/duplicate", mine["id"].as_str().unwrap()),
        &w.member,
        Some(json!({})),
    )
    .await;
    assert_eq!(
        (status, &copy["name"]),
        (StatusCode::CREATED, &json!("PLAN copy.md"))
    );
    let draft = upload(
        &app,
        &w,
        &w.member,
        &format!("name=draft.md&folder_id={specs}"),
        b"d",
    )
    .await;
    let (status, renamed) = call(
        &app,
        Method::PATCH,
        &format!("/artifact-files/{}", draft["id"].as_str().unwrap()),
        &w.member,
        Some(json!({"name": "ROADMAP.md"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    let (_, list) = call(
        &app,
        Method::GET,
        &format!("{}/artifact-files?folder_id={specs}", w.ws),
        &w.member,
        None,
    )
    .await;
    assert_eq!(names(&list), ["PLAN copy.md", "PLAN.md", "ROADMAP.md"]);
    let (_, list) = call(
        &app,
        Method::GET,
        &format!("{}/artifact-files?folder_id={specs}", w.ws),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(list.as_array().unwrap().len(), 6, "the owner sees both");

    // A deleted issue of a public team, or the deleted team, leaves its files to the workspace
    // under the first copy name free among the files of no issue there.
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/issues/{one}"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    for (file, name) in [(&first, "notes copy.md"), (&moved, "todo.md")] {
        let (status, kept) = call(
            &app,
            Method::GET,
            &format!("/artifact-files/{}", file["id"].as_str().unwrap()),
            &w.member,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{kept}");
        assert_eq!(
            (&kept["name"], &kept["issue"]),
            (&json!(name), &Value::Null)
        );
    }
    let (_, ops) = call(
        &app,
        Method::POST,
        &format!("{}/teams", w.ws),
        &w.owner,
        Some(json!({"name": "Operations", "key": "OPS"})),
    )
    .await;
    let ops = ops["id"].as_str().unwrap().to_owned();
    let runbook = issue(&app, &w, &ops, &w.owner, "Runbook").await;
    let book = upload(
        &app,
        &w,
        &w.owner,
        &format!("name=Notes.md&issue_id={runbook}"),
        b"restart it",
    )
    .await;
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("{}/teams/{ops}", w.ws),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, kept) = call(
        &app,
        Method::GET,
        &format!("/artifact-files/{}", book["id"].as_str().unwrap()),
        &w.member,
        None,
    )
    .await;
    assert_eq!(kept["name"], "Notes copy 2.md");

    // A workspace goes with every file, whatever names they share.
    upload(
        &app,
        &w,
        &w.member,
        &format!("name=notes.md&issue_id={two}"),
        b"again",
    )
    .await;
    let wid = w.ws.trim_start_matches("/workspaces/").parse().unwrap();
    let owner_id = sqlx::query_scalar("SELECT id FROM users WHERE email = 'owner@example.com'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    nexc::engine::workspaces::erase(&app.state, wid, owner_id, "deleted in a test")
        .await
        .unwrap();
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM artifact_files")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_database_names_copies_as_the_server_does(pool: PgPool) {
    let names = [
        "notes.md".to_owned(),
        "Dockerfile".to_owned(),
        ".gitignore".to_owned(),
        ".env.example".to_owned(),
        "prod.ENV.Example".to_owned(),
        "archive.tar.json".to_owned(),
        "spaced .md".to_owned(),
        format!("{}.md", "x".repeat(197)),
        format!("{}.tsx", "é".repeat(196)),
    ];
    for name in &names {
        for n in [1, 2, 10, 1000] {
            let in_db: String = sqlx::query_scalar("SELECT artifact_copy_name($1, $2)")
                .bind(name)
                .bind(n as i32)
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(
                in_db,
                nexc::domain::artifact_file::copy_name(name, n),
                "{name} {n}"
            );
        }
    }
}

/// Sends a request, with its body's type and bytes if any, in a task of its
/// own, to stand in the way of another.
fn spawn_request(
    app: &TestApp,
    method: Method,
    path: &str,
    token: &str,
    body: Option<(&str, Vec<u8>)>,
) -> tokio::task::JoinHandle<(StatusCode, Value)> {
    let mut request = axum::http::Request::builder()
        .method(method)
        .uri(format!("/api/v1{path}"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    let body = match body {
        Some((kind, bytes)) => {
            request = request.header(header::CONTENT_TYPE, kind);
            axum::body::Body::from(bytes)
        }
        None => axum::body::Body::empty(),
    };
    let request = request.body(body).unwrap();
    let router = app.router.clone();
    tokio::spawn(async move {
        let response = tower::ServiceExt::oneshot(router, request).await.unwrap();
        let status = response.status();
        let bytes = http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    })
}

/// Waits until a statement in this test's database waits on a lock.
async fn until_blocked(pool: &PgPool) {
    for _ in 0..400 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity
             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if waiting > 0 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("nothing waited on a lock");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_folder_delete_counts_what_lands_in_it_meanwhile(pool: PgPool) {
    let app = TestApp::new(pool.clone(), &[]).await;
    let w = world(&app).await;
    let owner_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email = 'owner@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let reorg = issue(&app, &w, &w.secret, &w.owner, "Reorg").await;
    let ship = issue(&app, &w, &w.eng, &w.owner, "Ship").await;
    let keep = folder(&app, &w, &w.owner, json!({"name": "Keep"})).await;
    let plan = upload(
        &app,
        &w,
        &w.owner,
        &format!("name=plan.md&folder_id={keep}&issue_id={reorg}"),
        b"p",
    )
    .await;
    let plan_id: uuid::Uuid = plan["id"].as_str().unwrap().parse().unwrap();

    // Someone else uploads a file of the private team's issue into the folder, moves one in or
    // attaches one in it to that issue, while the member, who cannot see it, deletes the folder:
    // the delete waits for it, counts it and is refused.
    for case in ["upload", "move", "attach"] {
        let docs = folder(&app, &w, &w.member, json!({"name": format!("Docs {case}")})).await;
        let notes = upload(
            &app,
            &w,
            &w.member,
            &format!("name=notes.md&folder_id={docs}&issue_id={ship}"),
            b"n",
        )
        .await;
        let notes_id: uuid::Uuid = notes["id"].as_str().unwrap().parse().unwrap();
        let docs_id: uuid::Uuid = docs.parse().unwrap();
        let mut tx = pool.begin().await.unwrap();
        match case {
            "upload" => {
                let new = nexc::repo::artifact_files::NewFile {
                    workspace_id: w.ws.trim_start_matches("/workspaces/").parse().unwrap(),
                    folder_id: Some(docs_id),
                    issue_id: Some(reorg.parse().unwrap()),
                    name: "secret.md",
                    content: "s",
                    sha256: "0",
                    duplicated_from: None,
                    created_by: owner_id,
                };
                nexc::repo::artifact_files::create_file(&mut tx, &new)
                    .await
                    .unwrap();
            }
            "move" => {
                sqlx::query("UPDATE artifact_files SET folder_id = $2 WHERE id = $1")
                    .bind(plan_id)
                    .bind(docs_id)
                    .execute(&mut *tx)
                    .await
                    .unwrap();
            }
            _ => {
                sqlx::query("UPDATE artifact_files SET issue_id = $2::uuid WHERE id = $1")
                    .bind(notes_id)
                    .bind(&reorg)
                    .execute(&mut *tx)
                    .await
                    .unwrap();
            }
        }
        let delete = spawn_request(
            &app,
            Method::DELETE,
            &format!("{}/artifact-folders/{docs}", w.ws),
            &w.member,
            None,
        );
        until_blocked(&pool).await;
        tx.commit().await.unwrap();
        let (status, body) = delete.await.unwrap();
        assert_eq!(status, StatusCode::CONFLICT, "{case}: {body}");
        let files: i64 =
            sqlx::query_scalar("SELECT count(*) FROM artifact_files WHERE folder_id = $1")
                .bind(docs_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let expected = if case == "attach" { 1 } else { 2 };
        assert_eq!(files, expected, "{case}: nothing went");
    }

    // A file put or moved into a folder whose delete holds it waits, then finds it gone.
    let wid: uuid::Uuid = w.ws.trim_start_matches("/workspaces/").parse().unwrap();
    let draft = upload(&app, &w, &w.member, "name=draft.md", b"d").await;
    for case in ["upload", "move"] {
        let gone = folder(&app, &w, &w.member, json!({"name": format!("Gone {case}")})).await;
        let gone_id: uuid::Uuid = gone.parse().unwrap();
        let mut tx = pool.begin().await.unwrap();
        nexc::repo::artifact_files::lock_folders(&mut *tx, wid)
            .await
            .unwrap();
        nexc::repo::artifact_files::lock_under(&mut tx, gone_id)
            .await
            .unwrap();
        let write = if case == "upload" {
            spawn_request(
                &app,
                Method::POST,
                &w.upload(&format!("name=late.md&folder_id={gone}")),
                &w.member,
                Some(("application/octet-stream", b"late".to_vec())),
            )
        } else {
            let body = json!({"folder_id": gone}).to_string().into_bytes();
            spawn_request(
                &app,
                Method::PATCH,
                &format!("/artifact-files/{}", draft["id"].as_str().unwrap()),
                &w.member,
                Some(("application/json", body)),
            )
        };
        until_blocked(&pool).await;
        nexc::repo::artifact_files::delete_folder(&mut *tx, gone_id)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let (status, body) = write.await.unwrap();
        assert_eq!(status, StatusCode::NOT_FOUND, "{case}: {body}");
        assert_eq!(body["detail"], "folder not found", "{case}");
    }
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_search_with_a_control_character_is_refused(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    upload(&app, &w, &w.member, "name=notes.md", b"n").await;
    for list in [
        format!("{}/artifact-files", w.ws),
        format!("{}/issues", w.ws),
    ] {
        for bad in ["%00", "a%00b", "a%1Bb", "%7F"] {
            let (status, body) = call(
                &app,
                Method::GET,
                &format!("{list}?q={bad}"),
                &w.member,
                None,
            )
            .await;
            assert_eq!(
                status,
                StatusCode::UNPROCESSABLE_ENTITY,
                "{list}?q={bad}: {body}"
            );
            assert!(body["errors"]["q"].is_array(), "{body}");
        }
        // Spaces around it, tabs and new lines among them, are trimmed as before.
        let (status, body) = call(
            &app,
            Method::GET,
            &format!("{list}?q=%09%20%0A"),
            &w.member,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{list}: {body}");
    }
    let (_, found) = call(
        &app,
        Method::GET,
        &format!("{}/artifact-files?q=%20NOTES%0A", w.ws),
        &w.member,
        None,
    )
    .await;
    assert_eq!(names(&found), ["notes.md"]);
}
