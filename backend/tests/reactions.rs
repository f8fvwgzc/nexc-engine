//! Reactions to comments and run reports: who may give them, how the
//! timeline shows them, and the inbox entry they give the comment's author.
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode};
use common::TestApp;
use nexc::domain::issue::REACTION_USERS_MAX;
use nexc::domain::user::Role;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

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

/// A workspace with a public and a private team, its owner, a member, a
/// guest of the public team, and an outsider.
struct World {
    ws: String,
    eng: String,
    secret: String,
    owner: String,
    owner_id: String,
    member: String,
    member_id: String,
    guest: String,
    outsider: String,
}

async fn world(app: &TestApp) -> World {
    let (owner, owner_id) = user(app, "owner@example.com").await;
    let (member, member_id) = user(app, "member@example.com").await;
    let (guest, guest_id) = user(app, "guest@example.com").await;
    let (outsider, _) = user(app, "outsider@example.com").await;
    let (_, list) = call(app, Method::GET, "/workspaces", &owner, None).await;
    let ws = format!("/workspaces/{}", list[0]["id"].as_str().unwrap());
    for (email, role) in [
        ("member@example.com", "member"),
        ("guest@example.com", "guest"),
    ] {
        let body = json!({"email": email, "role": role});
        let (status, _) = call(
            app,
            Method::POST,
            &format!("{ws}/members"),
            &owner,
            Some(body),
        )
        .await;
        assert!(status.is_success(), "{status}");
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
    let w = World {
        ws,
        eng: teams[0].clone(),
        secret: teams[1].clone(),
        owner,
        owner_id,
        member,
        member_id,
        guest,
        outsider,
    };
    join(app, &w, &w.eng, &guest_id).await;
    w
}

/// Adds a user to a team.
async fn join(app: &TestApp, w: &World, team: &str, user_id: &str) {
    let path = format!("{}/teams/{team}/members/{user_id}", w.ws);
    let (status, body) = call(app, Method::PUT, &path, &w.owner, Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// Files an issue in a team as the owner; returns its id.
async fn issue(app: &TestApp, w: &World, team: &str, title: &str) -> String {
    let path = format!("{}/teams/{team}/issues", w.ws);
    let (status, issue) = call(
        app,
        Method::POST,
        &path,
        &w.owner,
        Some(json!({"title": title})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{issue}");
    issue["id"].as_str().unwrap().to_owned()
}

/// Comments on the issue as `token`; returns the comment's id.
async fn comment(app: &TestApp, token: &str, iid: &str, body: &str) -> String {
    let path = format!("/issues/{iid}/comments");
    let (status, posted) = call(app, Method::POST, &path, token, Some(json!({"body": body}))).await;
    assert_eq!(status, StatusCode::CREATED, "{posted}");
    posted["id"].as_str().unwrap().to_owned()
}

/// `emoji` written for a query string.
fn encoded(emoji: &str) -> String {
    emoji.bytes().map(|b| format!("%{b:02X}")).collect()
}

/// Reacts to the entry as `token`; returns the status and the entry's reactions.
async fn react(
    app: &TestApp,
    token: &str,
    iid: &str,
    eid: &str,
    emoji: &str,
) -> (StatusCode, Value) {
    let path = format!("/issues/{iid}/events/{eid}/reactions");
    call(
        app,
        Method::POST,
        &path,
        token,
        Some(json!({"emoji": emoji})),
    )
    .await
}

/// Takes the reaction back as `token`; returns the status and the entry's reactions.
async fn unreact(
    app: &TestApp,
    token: &str,
    iid: &str,
    eid: &str,
    emoji: &str,
) -> (StatusCode, Value) {
    let path = format!(
        "/issues/{iid}/events/{eid}/reactions?emoji={}",
        encoded(emoji)
    );
    call(app, Method::DELETE, &path, token, None).await
}

/// The reactions as `(emoji, count, reacted)`.
fn summary(reactions: &Value) -> Vec<(String, i64, bool)> {
    reactions
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["emoji"].as_str().unwrap().to_owned(),
                r["count"].as_i64().unwrap(),
                r["reacted"].as_bool().unwrap(),
            )
        })
        .collect()
}

/// The entry `eid` of the issue's timeline as `token` reads it.
async fn entry(app: &TestApp, token: &str, iid: &str, eid: &str) -> Value {
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
        .find(|e| e["id"] == eid)
        .cloned()
        .unwrap_or(Value::Null)
}

/// A member's `reaction` inbox entries, newest first, as `(emoji, actor)`.
async fn reactions_in_inbox(app: &TestApp, w: &World, token: &str) -> Vec<(String, String)> {
    let (status, list) = call(app, Method::GET, &format!("{}/inbox", w.ws), token, None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    list.as_array()
        .unwrap()
        .iter()
        .filter(|n| n["kind"] == "reaction")
        .map(|n| {
            assert!(n["issue"]["identifier"].is_string(), "{n}");
            (
                n["emoji"].as_str().unwrap().to_owned(),
                n["actor"]["user_id"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn people_react_with_the_listed_emoji_and_see_who_did(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let iid = issue(&app, &w, &w.eng, "Fix login").await;
    let cid = comment(&app, &w.member, &iid, "Reproduced on staging.").await;

    // The owner reacts; doing it again changes nothing.
    for _ in 0..2 {
        let (status, reactions) = react(&app, &w.owner, &iid, &cid, "🎉").await;
        assert_eq!(status, StatusCode::OK, "{reactions}");
        assert_eq!(summary(&reactions), [("🎉".into(), 1, true)]);
        assert_eq!(reactions[0]["users"][0]["user_id"], w.owner_id.as_str());
        assert!(reactions[0]["users"][0]["name"].is_string());
    }
    // The member gives the same emoji, then another: each emoji once, in the order first given,
    // its people newest first, and `reacted` is the reader's.
    react(&app, &w.member, &iid, &cid, "🎉").await;
    let (_, reactions) = react(&app, &w.member, &iid, &cid, "👍").await;
    assert_eq!(
        summary(&reactions),
        [("🎉".into(), 2, true), ("👍".into(), 1, true)]
    );
    let users: Vec<&str> = reactions[0]["users"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["user_id"].as_str().unwrap())
        .collect();
    assert_eq!(users, [w.member_id.as_str(), w.owner_id.as_str()]);
    let seen = entry(&app, &w.owner, &iid, &cid).await;
    assert_eq!(
        summary(&seen["reactions"]),
        [("🎉".into(), 2, true), ("👍".into(), 1, false)]
    );
    assert_eq!(seen["parent_id"], Value::Null);
    // A heart without its presentation selector is the same heart.
    let (status, reactions) = react(&app, &w.guest, &iid, &cid, "\u{2764}").await;
    assert_eq!(status, StatusCode::OK, "{reactions}");
    assert_eq!(reactions[2]["emoji"], "❤️");

    // Taking one back; one never given is no error.
    let (status, reactions) = unreact(&app, &w.owner, &iid, &cid, "🎉").await;
    assert_eq!(status, StatusCode::OK, "{reactions}");
    assert_eq!(summary(&reactions)[0], ("🎉".into(), 1, false));
    let (status, again) = unreact(&app, &w.owner, &iid, &cid, "🎉").await;
    assert_eq!((status, &again), (StatusCode::OK, &reactions));
    let (_, reactions) = unreact(&app, &w.guest, &iid, &cid, "❤️").await;
    assert_eq!(reactions.as_array().unwrap().len(), 2);

    // Only the listed emoji, given or taken back.
    for bad in ["🙂", "👍🏽", ":tada:", "", "🎉🎉"] {
        let (status, problem) = react(&app, &w.owner, &iid, &cid, bad).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}: {problem}");
        assert!(problem["errors"]["emoji"].is_array(), "{problem}");
    }
    let (status, _) = unreact(&app, &w.owner, &iid, &cid, "🙂").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn only_comments_and_run_reports_take_reactions_from_those_who_may_comment(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let iid = issue(&app, &w, &w.eng, "Fix login").await;
    let cid = comment(&app, &w.member, &iid, "Reproduced on staging.").await;

    // A change is not something to react to.
    let (status, _) = call(
        &app,
        Method::PATCH,
        &format!("/issues/{iid}"),
        &w.owner,
        Some(json!({"priority": 1})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, events) = call(
        &app,
        Method::GET,
        &format!("/issues/{iid}/events"),
        &w.owner,
        None,
    )
    .await;
    let change = events
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "priority")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, _) = react(&app, &w.owner, &iid, &change, "👍").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = unreact(&app, &w.owner, &iid, &change, "👍").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        entry(&app, &w.owner, &iid, &change).await["reactions"],
        json!([])
    );

    // A run report is: it has no author, so nobody is told.
    let run_entry = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO issue_events (id, issue_id, actor_id, kind, body, to_value)
         VALUES ($1, $2::uuid, NULL, 'run', 'Done.', 'succeeded')",
    )
    .bind(run_entry)
    .bind(&iid)
    .execute(&app.state.db)
    .await
    .unwrap();
    let (status, reactions) = react(&app, &w.member, &iid, &run_entry.to_string(), "🚀").await;
    assert_eq!(status, StatusCode::OK, "{reactions}");
    assert_eq!(summary(&reactions), [("🚀".into(), 1, true)]);

    // An entry of another issue, or of none, is not found through this one.
    let other = issue(&app, &w, &w.eng, "Other").await;
    let (status, _) = react(&app, &w.owner, &other, &cid, "👍").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let nowhere = Uuid::now_v7().to_string();
    let (status, _) = react(&app, &w.owner, &iid, &nowhere, "👍").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Outsiders see no issue; nor does a guest outside a private team, whose issues stay unknown.
    let (status, _) = react(&app, &w.outsider, &iid, &cid, "👍").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = unreact(&app, &w.outsider, &iid, &cid, "👍").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let secret = issue(&app, &w, &w.secret, "Reorg").await;
    let secret_comment = comment(&app, &w.owner, &secret, "Plans.").await;
    for token in [&w.guest, &w.member] {
        let (status, _) = react(&app, token, &secret, &secret_comment, "👍").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    // Deleting the comment takes its reactions along.
    react(&app, &w.owner, &iid, &cid, "👀").await;
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/issues/{iid}/comments/{cid}"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let left: i64 =
        sqlx::query_scalar("SELECT count(*) FROM issue_event_reactions WHERE event_id = $1::uuid")
            .bind(&cid)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(left, 0);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_author_of_a_comment_hears_of_a_reaction_once_a_day(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let iid = issue(&app, &w, &w.eng, "Fix login").await;
    let cid = comment(&app, &w.member, &iid, "Reproduced on staging.").await;
    let reacted = |emoji: &str| (emoji.to_owned(), w.owner_id.clone());

    // The owner reacts: the member hears, with the emoji; nobody else does.
    react(&app, &w.owner, &iid, &cid, "🎉").await;
    assert_eq!(
        reactions_in_inbox(&app, &w, &w.member).await,
        [reacted("🎉")]
    );
    assert!(reactions_in_inbox(&app, &w, &w.owner).await.is_empty());
    // Taken back and given again, or given twice at once: nothing more. Taking back tells nobody.
    unreact(&app, &w.owner, &iid, &cid, "🎉").await;
    let path = format!("/issues/{iid}/events/{cid}/reactions");
    let twice = (0..4).map(|_| {
        call(
            &app,
            Method::POST,
            &path,
            &w.owner,
            Some(json!({"emoji": "🎉"})),
        )
    });
    for (status, body) in futures::future::join_all(twice).await {
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    assert_eq!(
        reactions_in_inbox(&app, &w, &w.member).await,
        [reacted("🎉")]
    );
    // Another emoji is news.
    react(&app, &w.owner, &iid, &cid, "🚀").await;
    assert_eq!(
        reactions_in_inbox(&app, &w, &w.member).await,
        [reacted("🚀"), reacted("🎉")]
    );
    // A day later the same emoji given again is news again.
    sqlx::query(
        "UPDATE notifications SET created_at = now() - interval '25 hours' WHERE kind = 'reaction'",
    )
    .execute(&app.state.db)
    .await
    .unwrap();
    unreact(&app, &w.owner, &iid, &cid, "🎉").await;
    react(&app, &w.owner, &iid, &cid, "🎉").await;
    assert_eq!(reactions_in_inbox(&app, &w, &w.member).await.len(), 3);

    // Reacting to one's own comment tells nobody.
    react(&app, &w.member, &iid, &cid, "👀").await;
    assert_eq!(reactions_in_inbox(&app, &w, &w.member).await.len(), 3);

    // An author who can no longer see the issue is not told.
    join(&app, &w, &w.secret, &w.member_id).await;
    let secret = issue(&app, &w, &w.secret, "Reorg").await;
    let theirs = comment(&app, &w.member, &secret, "Plans.").await;
    let leave = format!("{}/teams/{}/members/{}", w.ws, w.secret, w.member_id);
    let (status, _) = call(&app, Method::DELETE, &leave, &w.owner, None).await;
    assert!(status.is_success(), "{status}");
    let (status, _) = react(&app, &w.owner, &secret, &theirs, "👍").await;
    assert_eq!(status, StatusCode::OK);
    let told: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM notifications WHERE kind = 'reaction' AND issue_id = $1::uuid",
    )
    .bind(&secret)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!(told, 0);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_reaction_names_the_latest_ten_who_gave_it(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let iid = issue(&app, &w, &w.eng, "Fix login").await;
    let cid = comment(&app, &w.member, &iid, "Ship it?").await;
    // Twelve people, a minute apart, the last one newest.
    let mut people = Vec::new();
    for n in 0..12 {
        let email = format!("fan{n}@example.com");
        let fan =
            nexc::http::handlers::auth::create_user(&app.state, &email, "Fan", Role::User, "!")
                .await
                .unwrap();
        sqlx::query(
            "INSERT INTO issue_event_reactions (event_id, user_id, emoji, created_at)
             VALUES ($1::uuid, $2, '🔥', now() - make_interval(mins => $3))",
        )
        .bind(&cid)
        .bind(fan.id)
        .bind(12 - n)
        .execute(&app.state.db)
        .await
        .unwrap();
        people.push(fan.id.to_string());
    }
    let seen = entry(&app, &w.owner, &iid, &cid).await;
    let fire = &seen["reactions"][0];
    assert_eq!(
        (fire["count"].as_i64(), fire["reacted"].as_bool()),
        (Some(12), Some(false))
    );
    let named: Vec<&str> = fire["users"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["user_id"].as_str().unwrap())
        .collect();
    let latest: Vec<&str> = people
        .iter()
        .rev()
        .take(REACTION_USERS_MAX)
        .map(String::as_str)
        .collect();
    assert_eq!(named, latest);
    // The owner joins in: first among them now, and counted.
    let (_, reactions) = react(&app, &w.owner, &iid, &cid, "🔥").await;
    assert_eq!(summary(&reactions), [("🔥".into(), 13, true)]);
    assert_eq!(reactions[0]["users"][0]["user_id"], w.owner_id.as_str());
    assert_eq!(
        reactions[0]["users"].as_array().unwrap().len(),
        REACTION_USERS_MAX
    );
}
