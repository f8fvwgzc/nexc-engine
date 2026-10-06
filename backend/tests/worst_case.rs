//! Worst cases, swept across every documented route rather than a chosen
//! few: nobody gets in without a token, an outsider reads nothing of another
//! workspace, no input however hostile makes the server fail, races resolve
//! to one winner, answers stay bounded, and a large workspace stays fast.
//!
//! The routes come from the OpenAPI document, so a route added later is
//! covered without anyone remembering to add it here.
#![forbid(unsafe_code)]

mod common;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use common::{PASSWORD, TestApp, refresh_cookie};
use futures::future::join_all;
use http_body_util::BodyExt;
use nexc::domain::user::Role;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// Routes that answer without a bearer token.
const PUBLIC: &[&str] = &[
    "/api/v1/healthz",
    "/api/v1/readyz",
    "/api/v1/auth/register",
    "/api/v1/auth/login",
    "/api/v1/auth/refresh",
    "/api/v1/auth/logout",
    "/api/v1/auth/password/reset",
];

struct Operation {
    method: Method,
    /// The path as documented, with `{name}` placeholders.
    path: String,
    query: Vec<String>,
    has_body: bool,
}

/// Every documented operation but the ticket-authenticated streams and the
/// metrics endpoint, which has its own token.
fn operations() -> Vec<Operation> {
    let spec = serde_json::to_value(nexc::http::openapi()).unwrap();
    let mut out = Vec::new();
    for (path, methods) in spec["paths"].as_object().unwrap() {
        let stream =
            path.ends_with("/ws") || path.ends_with("/events") && path.contains("/graphs/");
        if stream || path == "/metrics" {
            continue;
        }
        for (method, op) in methods.as_object().unwrap() {
            let query = op["parameters"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|p| p["in"] == "query")
                .map(|p| p["name"].as_str().unwrap().to_owned())
                .collect();
            out.push(Operation {
                method: Method::from_bytes(method.to_uppercase().as_bytes()).unwrap(),
                path: path.clone(),
                query,
                has_body: op["requestBody"].is_object(),
            });
        }
    }
    assert!(out.len() > 130, "the whole API was read: {}", out.len());
    out
}

/// A path with its placeholders filled from `ids`, or with `other`.
fn fill(path: &str, ids: &HashMap<&str, String>, other: &str) -> String {
    path.split('/')
        .map(
            |part| match part.strip_prefix('{').and_then(|p| p.strip_suffix('}')) {
                Some(name) => ids.get(name).map_or(other, String::as_str),
                None => part,
            },
        )
        .collect::<Vec<_>>()
        .join("/")
}

/// Sends a request whose body, if any, goes out exactly as given.
async fn send(
    app: &TestApp,
    method: Method,
    url: &str,
    token: Option<&str>,
    body: Option<String>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(url);
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let request = match body {
        Some(body) => request
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body)),
        None => request.body(Body::empty()),
    }
    .unwrap();
    let response = tower::ServiceExt::oneshot(app.router.clone(), request)
        .await
        .unwrap();
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
    let url = format!("/api/v1{path}");
    send(app, method, &url, Some(token), body.map(|b| b.to_string())).await
}

/// A workspace with one of everything, and the ids a path can name.
struct World {
    owner: String,
    owner_id: String,
    wid: String,
    team: String,
    ids: HashMap<&'static str, String>,
}

async fn world(app: &TestApp) -> World {
    let (owner, _) = app.register("owner@example.com").await;
    let (_, me) = call(app, Method::GET, "/auth/me", &owner, None).await;
    let owner_id = me["id"].as_str().unwrap().to_owned();
    let (_, list) = call(app, Method::GET, "/workspaces", &owner, None).await;
    let wid = list[0]["id"].as_str().unwrap().to_owned();
    let ws = format!("/workspaces/{wid}");
    let made = |path: String, body: Value| {
        let owner = &owner;
        async move {
            let (status, made) = call(app, Method::POST, &path, owner, Some(body)).await;
            assert!(status.is_success(), "{path}: {made}");
            made["id"].as_str().unwrap().to_owned()
        }
    };
    let team = made(
        format!("{ws}/teams"),
        json!({"name": "Engineering", "key": "ENG"}),
    )
    .await;
    let project = made(format!("{ws}/projects"), json!({"name": "Launch"})).await;
    let label = made(
        format!("{ws}/labels"),
        json!({"name": "Bug", "color": "#ef4444"}),
    )
    .await;
    let issue = made(
        format!("{ws}/teams/{team}/issues"),
        json!({"title": "Secret plan", "assignee_id": owner_id, "project_id": project}),
    )
    .await;
    let graph = made(
        "/graphs".into(),
        json!({"name": "Secret graph", "goal": "g", "workspace_id": wid}),
    )
    .await;
    let ids = HashMap::from([
        ("wid", wid.clone()),
        ("tid", team.clone()),
        ("iid", issue.clone()),
        ("gid", graph),
        ("lid", label),
        ("uid", owner_id.clone()),
        ("kind", "issue".to_owned()),
        ("id", issue),
        ("pid", project),
    ]);
    World {
        owner,
        owner_id,
        wid,
        team,
        ids,
    }
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn nobody_gets_in_without_a_valid_token(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    // A token of the right shape signed with another key, and plain garbage.
    let forged = nexc::security::jwt::JwtKeys::new(&[7; 32], Duration::from_secs(900))
        .issue(Uuid::parse_str(&w.owner_id).unwrap(), Role::Admin, 0)
        .unwrap();
    let mut checked = 0;
    for op in operations() {
        if PUBLIC.contains(&op.path.as_str()) {
            continue;
        }
        let url = fill(&op.path, &w.ids, &w.wid);
        for token in [None, Some("garbage"), Some(forged.as_str())] {
            let (status, _) = send(&app, op.method.clone(), &url, token, None).await;
            assert_eq!(
                status,
                StatusCode::UNAUTHORIZED,
                "{} {url} with {token:?}",
                op.method
            );
            checked += 1;
        }
    }
    assert!(
        checked > 390,
        "every protected route was tried three ways ({checked})"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_outsider_reads_nothing_of_another_workspace(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let (outsider, _) = app.register("outsider@example.com").await;
    let mut checked = 0;
    for op in operations() {
        let named = op.path.contains('{') || op.query.iter().any(|q| q == "workspace_id");
        let own = op.path.starts_with("/api/v1/auth/") || op.path.starts_with("/api/v1/settings/");
        if op.method != Method::GET || !named || own {
            continue;
        }
        // The victim's real ids everywhere a path or the query can name one.
        let mut url = fill(&op.path, &w.ids, &w.wid);
        if op.query.iter().any(|q| q == "workspace_id") {
            url.push_str(&format!("?workspace_id={}", w.wid));
        } else if op.query.iter().any(|q| q == "q") {
            url.push_str("?q=secret");
        }
        let (status, body) = send(&app, Method::GET, &url, Some(&outsider), None).await;
        // Refused, or an answer with nothing in it; never the victim's data.
        let empty = body.as_array().is_some_and(Vec::is_empty);
        assert!(
            !status.is_success() || empty,
            "GET {url} answered an outsider {status}: {body}"
        );
        assert!(!body.to_string().contains("Secret"), "GET {url}: {body}");
        checked += 1;
    }
    assert!(
        checked > 45,
        "every route that names something was tried ({checked})"
    );
}

/// Members of the world's workspace to spread a sweep over: one account may
/// send a burst of 120 requests.
async fn members(app: &TestApp, pool: &PgPool, wid: &str, count: usize) -> Vec<String> {
    let mut tokens = Vec::new();
    for n in 0..count {
        let email = format!("sweeper{n}@example.com");
        let user =
            nexc::http::handlers::auth::create_user(&app.state, &email, "Sweeper", Role::User, "!")
                .await
                .unwrap();
        sqlx::query("INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1::uuid, $2, 'owner')")
            .bind(wid)
            .bind(user.id)
            .execute(pool)
            .await
            .unwrap();
        tokens.push(app.state.jwt.issue(user.id, Role::User, 0).unwrap());
    }
    tokens
}

/// Bodies no handler expects: wrong shapes, wrong types, hostile strings, sizes at the limit.
fn hostile_bodies() -> Vec<(&'static str, String)> {
    let nasty =
        "'; DROP TABLE users; -- \u{0} \u{202e} <script>alert(1)</script> ../../etc/passwd %00 🙂";
    let keys = [
        "name",
        "title",
        "description",
        "email",
        "role",
        "key",
        "color",
        "url",
        "goal",
        "content",
        "body",
        "kind",
        "q",
        "password",
        "new_password",
        "current_password",
        "token",
        "confirm",
        "state_id",
        "assignee_id",
        "project_id",
        "parent_id",
        "cycle_id",
        "team_id",
        "graph_id",
        "workspace_id",
        "label_ids",
        "priority",
        "limit",
        "private",
        "status",
        "due_date",
        "starts_on",
        "ends_on",
        "source",
        "target",
        "x",
        "y",
        "provider",
        "model",
        "api_key",
        "suspended",
        "reason",
        "history",
        "message",
        "nodes",
        "edges",
        "ontology",
        "tags",
    ];
    let with = |value: Value| -> String {
        Value::Object(
            keys.iter()
                .map(|k| ((*k).to_owned(), value.clone()))
                .collect(),
        )
        .to_string()
    };
    let deep = (0..200).fold(json!(1), |inner, _| json!({ "a": inner }));
    vec![
        ("empty object", "{}".into()),
        ("array", "[]".into()),
        ("null", "null".into()),
        ("broken json", "{\"name\": ".into()),
        ("hostile strings", with(json!(nasty))),
        ("numbers", with(json!(-9_223_372_036_854_775_808_i64))),
        ("floats", with(json!(1.0e308))),
        ("nested", with(json!({"a": [1, {"b": null}]}))),
        ("arrays", with(json!([nasty, 1, null, []]))),
        ("booleans", with(json!(true))),
        ("uuids of nothing", with(json!(Uuid::now_v7().to_string()))),
        ("deeply nested", deep.to_string()),
        (
            "a long string",
            json!({"name": "x".repeat(300_000), "title": "y".repeat(300_000)}).to_string(),
        ),
    ]
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn no_route_fails_on_hostile_input(pool: PgPool) {
    let app = TestApp::new(pool.clone(), &[]).await;
    let w = world(&app).await;
    let tokens = members(&app, &pool, &w.wid, 40).await;
    // The console's routes are swept as platform administrators, on an account made to be hit.
    let mut admins = Vec::new();
    for n in 0..8 {
        let email = format!("root{n}@example.com");
        let root =
            nexc::http::handlers::auth::create_user(&app.state, &email, "Root", Role::Admin, "!")
                .await
                .unwrap();
        admins.push(app.state.jwt.issue(root.id, Role::Admin, 0).unwrap());
    }
    let (_, target) = app.register("target@example.com").await;
    let _ = target;
    let target: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email = 'target@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let mut console_ids = w.ids.clone();
    console_ids.insert("uid", target.to_string());
    let nowhere = Uuid::now_v7().to_string();
    let bodies = hostile_bodies();
    let hostile_values = [
        "%27%3B%20DROP%20TABLE%20users%3B--",
        "99999999999999999999999",
        "-1",
        "%00",
        "true",
        "..%2F..%2Fetc",
    ];
    let mut sent = 0;
    let mut broken = Vec::new();
    let mut seen = std::collections::BTreeMap::new();
    for op in operations() {
        // Ending the sweeper's own sessions or account would only stop the sweep.
        if op.path.ends_with("/auth/sessions/end") || op.path.ends_with("/auth/me/delete") {
            continue;
        }
        // Real things to read and write on; nothing real to delete.
        let console = op.path.starts_with("/api/v1/admin/");
        let ids = if console { &console_ids } else { &w.ids };
        let real = fill(&op.path, ids, &nowhere);
        let none = fill(&op.path, &HashMap::new(), &nowhere);
        let mut requests: Vec<(String, Option<String>, &str)> = Vec::new();
        let urls = if op.method == Method::DELETE {
            vec![none]
        } else {
            vec![real, none]
        };
        for url in urls {
            if op.has_body {
                for (tag, body) in &bodies {
                    requests.push((url.clone(), Some(body.clone()), tag));
                }
            } else {
                requests.push((url.clone(), None, "no body"));
            }
            for value in hostile_values {
                let query: Vec<String> = op
                    .query
                    .iter()
                    .map(|name| format!("{name}={value}"))
                    .collect();
                if !query.is_empty() {
                    requests.push((format!("{url}?{}", query.join("&")), None, "hostile query"));
                }
            }
            if !op.query.is_empty() {
                requests.push((
                    format!("{url}?unknown=1&q=x&limit=5"),
                    None,
                    "unknown parameter",
                ));
            }
        }
        for (url, body, tag) in requests {
            let pool_of = if console { &admins } else { &tokens };
            let token = &pool_of[(sent / 100) % pool_of.len()];
            sent += 1;
            let (status, answer) = send(&app, op.method.clone(), &url, Some(token), body).await;
            *seen.entry(status.as_u16()).or_insert(0usize) += 1;
            if status.is_server_error() {
                broken.push(format!("{} {url} [{tag}] -> {status}: {answer}", op.method));
            }
        }
    }
    assert!(sent > 1_500, "the sweep ran ({sent} requests)");
    eprintln!("hostile sweep: {sent} requests, answers by status: {seen:?}");
    // The sweep is only worth something if requests reach the handlers.
    let limited = seen.get(&429).copied().unwrap_or(0);
    assert!(
        limited * 20 < sent,
        "{limited} of {sent} requests were only rate limited"
    );
    for reached in [200, 400, 404, 422] {
        let count = seen.get(&reached).copied().unwrap_or(0);
        assert!(count > 20, "few {reached} answers: {seen:?}");
    }
    assert!(
        sent / 100 < tokens.len(),
        "no account was reused past its allowance ({sent} requests)"
    );
    assert!(
        broken.is_empty(),
        "{} of {sent} requests made the server fail:\n{}",
        broken.len(),
        broken.join("\n")
    );
    // The server is still itself afterwards.
    let (status, list) = call(&app, Method::GET, "/workspaces", &w.owner, None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn races_have_one_winner(pool: PgPool) {
    let app = TestApp::new(pool.clone(), &[]).await;
    let w = world(&app).await;
    let ws = format!("/workspaces/{}", w.wid);

    // Forty issues filed at once: every number is given once, with no gap.
    let issues = format!("{ws}/teams/{}/issues", w.team);
    let filed = join_all((0..40).map(|n| {
        let body = json!({"title": format!("Racer {n}")});
        call(&app, Method::POST, &issues, &w.owner, Some(body))
    }))
    .await;
    let mut numbers: Vec<i64> = filed
        .iter()
        .map(|(status, issue)| {
            assert_eq!(*status, StatusCode::CREATED, "{issue}");
            issue["number"].as_i64().unwrap()
        })
        .collect();
    numbers.sort_unstable();
    assert_eq!(
        numbers,
        (2..=41).collect::<Vec<i64>>(),
        "after the world's first issue"
    );

    // Two owners step down at the same moment: the workspace keeps one of them.
    let (second, _) = app.register("second@example.com").await;
    let (_, me) = call(&app, Method::GET, "/auth/me", &second, None).await;
    let second_id = me["id"].as_str().unwrap().to_owned();
    let invite = json!({"email": "second@example.com", "role": "owner"});
    call(
        &app,
        Method::POST,
        &format!("{ws}/members"),
        &w.owner,
        Some(invite),
    )
    .await;
    let demote = json!({"role": "member"});
    let both = join_all([
        call(
            &app,
            Method::PATCH,
            &format!("{ws}/members/{}", w.owner_id),
            &w.owner,
            Some(demote.clone()),
        ),
        call(
            &app,
            Method::PATCH,
            &format!("{ws}/members/{second_id}"),
            &second,
            Some(demote),
        ),
    ])
    .await;
    let stepped: Vec<StatusCode> = both.iter().map(|(status, _)| *status).collect();
    assert_eq!(
        stepped.iter().filter(|s| s.is_success()).count(),
        1,
        "exactly one owner stepped down: {stepped:?}"
    );
    let owners: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workspace_members WHERE workspace_id = $1::uuid AND role = 'owner'",
    )
    .bind(&w.wid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(owners, 1);

    // One refresh token presented five times at once starts at most one session.
    let session = app
        .request(
            Method::POST,
            "/api/v1/auth/login",
            None,
            Some(json!({"email": "second@example.com", "password": PASSWORD})),
        )
        .await;
    let cookie = format!("nexc_refresh={}", refresh_cookie(&session.headers).unwrap());
    let headers = [("cookie", cookie.as_str()), ("x-requested-with", "nexc")];
    let refreshed = join_all(
        (0..5)
            .map(|_| app.request_with(Method::POST, "/api/v1/auth/refresh", None, None, &headers)),
    )
    .await;
    let renewed = refreshed
        .iter()
        .filter(|r| r.status == StatusCode::OK)
        .count();
    assert!(
        renewed <= 1,
        "a refresh token is spent once ({renewed} sessions)"
    );

    // One reset link used eight times at once sets one password.
    let hash = nexc::security::password::hash_password(PASSWORD).unwrap();
    nexc::http::handlers::auth::create_user(
        &app.state,
        "root@example.com",
        "Root",
        Role::Admin,
        &hash,
    )
    .await
    .unwrap();
    let root = app
        .request(
            Method::POST,
            "/api/v1/auth/login",
            None,
            Some(json!({"email": "root@example.com", "password": PASSWORD})),
        )
        .await
        .body["access_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let issue = format!("/admin/users/{second_id}/password-reset");
    let (_, link) = call(&app, Method::POST, &issue, &root, None).await;
    let token = link["token"].as_str().unwrap();
    let used = join_all((0..8).map(|n| {
        let body = json!({"token": token, "new_password": format!("a new passphrase {n}")});
        app.request(
            Method::POST,
            "/api/v1/auth/password/reset",
            None,
            Some(body),
        )
    }))
    .await;
    let set = used
        .iter()
        .filter(|r| r.status == StatusCode::NO_CONTENT)
        .count();
    assert_eq!(set, 1, "a reset link is spent once");
}

/// How long an answer may take with a large workspace, on a debug build.
const BUDGET: Duration = Duration::from_secs(3);

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_large_workspace_stays_bounded_and_fast(pool: PgPool) {
    let app = TestApp::new(pool.clone(), &[]).await;
    let w = world(&app).await;
    let ws = format!("/workspaces/{}", w.wid);
    // 20,000 issues with a comment each, filed today, half of them the owner's.
    sqlx::query(
        "INSERT INTO issues (id, workspace_id, team_id, number, title, state_id, priority,
                             assignee_id, creator_id, due_date)
         SELECT gen_random_uuid(), $1::uuid, $2::uuid, 1000 + n,
                'Load issue ' || n || ' ' || md5(n::text),
                (SELECT id FROM issue_states WHERE team_id = $2::uuid ORDER BY position LIMIT 1),
                n % 5, CASE WHEN n % 2 = 0 THEN $3::uuid END, $3::uuid,
                CASE WHEN n % 10 = 0 THEN current_date END
         FROM generate_series(1, 20000) n",
    )
    .bind(&w.wid)
    .bind(&w.team)
    .bind(&w.owner_id)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO issue_events (id, issue_id, actor_id, kind, body)
         SELECT gen_random_uuid(), i.id, $2::uuid, 'comment', 'Looks fine ' || i.number
         FROM issues i WHERE i.workspace_id = $1::uuid",
    )
    .bind(&w.wid)
    .bind(&w.owner_id)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("ANALYZE issues, issue_events")
        .execute(&pool)
        .await
        .unwrap();

    // (path, the most rows it may answer with)
    let reads: Vec<(String, usize)> = vec![
        (format!("{ws}/issues"), 200),
        (format!("{ws}/issues?limit=100000"), 500),
        (format!("{ws}/issues?q=md5&open=true"), 200),
        (format!("{ws}/issues?q=ENG-15&limit=50"), 50),
        (
            format!("{ws}/issues?assignee_id={}&limit=50", w.owner_id),
            50,
        ),
        (
            format!("{ws}/issues?creator_id={}&limit=50", w.owner_id),
            50,
        ),
        (format!("{ws}/search?q=Load&limit=10"), 60),
        (format!("{ws}/map/issue?q=issue&limit=100000"), 50),
        (format!("{ws}/timeline"), 1_000),
        (format!("{ws}/timeline/days?days=366"), 366),
        (format!("{ws}/audit?limit=100000"), 200),
        (format!("{ws}/inbox"), 1_000),
        (format!("{ws}/projects"), 1_000),
    ];
    for (path, most) in &reads {
        let started = Instant::now();
        let (status, body) = call(&app, Method::GET, path, &w.owner, None).await;
        let took = started.elapsed();
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        let rows = body.as_array().map_or(0, Vec::len);
        eprintln!("{took:>10.1?}  {rows:>5} rows  {path}");
        assert!(
            rows <= *most,
            "{path} answered {rows} rows, more than {most}"
        );
        assert!(took < BUDGET, "{path} took {took:?}");
    }
    // The whole-workspace views answer too, and the day is summarised from a bounded digest.
    for path in [
        format!("{ws}/map"),
        format!("{ws}/usage"),
        format!("{ws}/map/member/{}", w.owner_id),
    ] {
        let started = Instant::now();
        let (status, body) = call(&app, Method::GET, &path, &w.owner, None).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        assert!(
            started.elapsed() < BUDGET,
            "{path} took {:?}",
            started.elapsed()
        );
    }
    let started = Instant::now();
    let (status, summary) = call(
        &app,
        Method::POST,
        &format!("{ws}/timeline/summary"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{summary}");
    assert!(summary["event_count"].as_i64().unwrap() > 40_000);
    assert!(
        started.elapsed() < BUDGET,
        "the summary took {:?}",
        started.elapsed()
    );

    // A body past the limit is refused before it is read into memory.
    let huge = json!({"name": "x".repeat(2 * 1024 * 1024)}).to_string();
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/workspaces",
        Some(&w.owner),
        Some(huge),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);

    // One account hammering the API is slowed down, and told when to come back.
    let mut limited = None;
    for _ in 0..200 {
        let response = app
            .request(Method::GET, "/api/v1/auth/me", Some(&w.owner), None)
            .await;
        if response.status == StatusCode::TOO_MANY_REQUESTS {
            limited = Some(response);
            break;
        }
    }
    let limited = limited.expect("200 requests in a row hit the limit");
    assert!(limited.headers.contains_key("retry-after"));
    // ... without that being anyone else's problem.
    let (other, _) = app.register("calm@example.com").await;
    let (status, _) = call(&app, Method::GET, "/auth/me", &other, None).await;
    assert_eq!(status, StatusCode::OK);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_api_stays_up_when_what_it_depends_on_is_down(pool: PgPool) {
    // No agent runtime is listening where the server expects one.
    let app = TestApp::new(pool, &[("NEXC_RUNTIME_URL", "http://127.0.0.1:9")]).await;
    let w = world(&app).await;
    let ws = format!("/workspaces/{}", w.wid);
    // A plain text file is read by the server itself; a PDF needs the runtime that is down.
    for (name, bytes) in [
        ("notes.txt", b"Goods under 150 EUR are exempt.".to_vec()),
        ("scan.pdf", b"%PDF-1.4 not really a document".to_vec()),
    ] {
        let path = format!("/api/v1{ws}/documents?name={name}");
        let upload = app.send_bytes(&path, &w.owner, bytes).await;
        assert!(upload.status.is_success(), "{name}: {}", upload.body);
    }
    // Reading the PDF is tried and fails; that is the document's state, not an outage.
    nexc::engine::knowledge::work(&app.state).await.unwrap();
    let (status, documents) = call(
        &app,
        Method::GET,
        &format!("{ws}/documents"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{documents}");
    let state_of = |name: &str| {
        let found = documents
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["name"] == name);
        found.unwrap()["status"].as_str().unwrap().to_owned()
    };
    assert_eq!(state_of("notes.txt"), "ready", "{documents}");
    assert_ne!(state_of("scan.pdf"), "ready", "{documents}");
    for path in [
        format!("{ws}/issues"),
        format!("{ws}/search?q=notes"),
        "/auth/me".to_owned(),
    ] {
        let (status, body) = call(&app, Method::GET, &path, &w.owner, None).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
    }
    let health = app.request(Method::GET, "/healthz", None, None).await;
    assert_eq!(health.status, StatusCode::OK);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_session_ended_on_one_server_ends_on_the_others(pool: PgPool) {
    // Two servers on one database, as behind a load balancer.
    let here = TestApp::new(pool.clone(), &[]).await;
    let there = TestApp::new(pool.clone(), &[]).await;
    let (token, _) = here.register("roaming@example.com").await;
    let (status, _) = call(&there, Method::GET, "/auth/me", &token, None).await;
    assert_eq!(status, StatusCode::OK, "both servers accept the token");

    let (status, _) = call(&here, Method::POST, "/auth/sessions/end", &token, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(&here, Method::GET, "/auth/me", &token, None).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "at once where it was ended"
    );
    // The other server learns of it at its next look, every ten seconds when running.
    there
        .state
        .sessions
        .sync(&pool, there.state.settings.access_ttl)
        .await
        .unwrap();
    let (status, _) = call(&there, Method::GET, "/auth/me", &token, None).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "and on the other after it looked"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_address_behind_a_proxy_is_the_visitors(pool: PgPool) {
    let sign_in = |app: &TestApp| {
        let body = json!({"email": "visitor@example.com", "password": PASSWORD});
        let headers = [("x-forwarded-for", "198.51.100.7, 203.0.113.9")];
        let router = app.router.clone();
        async move {
            let mut request = Request::post("/api/v1/auth/login")
                .header(header::CONTENT_TYPE, "application/json");
            for (name, value) in headers {
                request = request.header(name, value);
            }
            let response = tower::ServiceExt::oneshot(
                router,
                request.body(Body::from(body.to_string())).unwrap(),
            )
            .await
            .unwrap();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            body["access_token"].as_str().unwrap().to_owned()
        }
    };
    // Behind a trusted proxy the last hop it reports is the visitor's address.
    let trusting = TestApp::new(pool.clone(), &[("NEXC_TRUST_PROXY", "true")]).await;
    trusting.register("visitor@example.com").await;
    let token = sign_in(&trusting).await;
    let (_, activity) = call(&trusting, Method::GET, "/auth/activity", &token, None).await;
    assert_eq!(activity[0]["kind"], "signed_in");
    assert_eq!(activity[0]["ip"], "203.0.113.9");
    // Without one, a header anyone can send is not believed.
    let direct = TestApp::new(pool, &[]).await;
    let token = sign_in(&direct).await;
    let (_, activity) = call(&direct, Method::GET, "/auth/activity", &token, None).await;
    assert_eq!(activity[0]["kind"], "signed_in");
    assert!(activity[0]["ip"].is_null(), "{}", activity[0]);
}
