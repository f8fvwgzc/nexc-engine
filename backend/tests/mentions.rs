//! Mentions in comments: members hear of them in their inbox, and a
//! mentioned agent is handed the issue (delegated, then asked for a plan).
//! Replies: the author of the comment replied to hears of it too.
#![forbid(unsafe_code)]

mod common;

use std::time::Duration;

use axum::http::{Method, StatusCode};
use common::TestApp;
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
/// guest, and an outsider with a workspace of their own.
struct World {
    ws: String,
    wid: String,
    eng: String,
    secret: String,
    owner: String,
    owner_id: String,
    member: String,
    member_id: String,
    guest_id: String,
    outsider: String,
    outsider_id: String,
}

async fn world(app: &TestApp) -> World {
    let (owner, owner_id) = user(app, "owner@example.com").await;
    let (member, member_id) = user(app, "member@example.com").await;
    let (_, guest_id) = user(app, "guest@example.com").await;
    let (outsider, outsider_id) = user(app, "outsider@example.com").await;
    let (_, list) = call(app, Method::GET, "/workspaces", &owner, None).await;
    let wid = list[0]["id"].as_str().unwrap().to_owned();
    let ws = format!("/workspaces/{wid}");
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
    World {
        ws,
        wid,
        eng: teams[0].clone(),
        secret: teams[1].clone(),
        owner,
        owner_id,
        member,
        member_id,
        guest_id,
        outsider,
        outsider_id,
    }
}

/// Files an issue in a team as `token`; returns its id.
async fn issue(app: &TestApp, w: &World, team: &str, token: &str, title: &str) -> String {
    let path = format!("{}/teams/{team}/issues", w.ws);
    let (status, issue) = call(
        app,
        Method::POST,
        &path,
        token,
        Some(json!({"title": title})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{issue}");
    issue["id"].as_str().unwrap().to_owned()
}

/// Comments on the issue as `token`; returns the status and the comment.
async fn comment(app: &TestApp, token: &str, iid: &str, body: &str) -> (StatusCode, Value) {
    let path = format!("/issues/{iid}/comments");
    call(app, Method::POST, &path, token, Some(json!({"body": body}))).await
}

/// Adds a user to a team.
async fn join(app: &TestApp, w: &World, team: &str, user_id: &str) {
    let path = format!("{}/teams/{team}/members/{user_id}", w.ws);
    let (status, body) = call(app, Method::PUT, &path, &w.owner, Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// The kinds in a member's inbox, newest first.
async fn inbox(app: &TestApp, w: &World, token: &str) -> Vec<String> {
    let (status, list) = call(app, Method::GET, &format!("{}/inbox", w.ws), token, None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    list.as_array()
        .unwrap()
        .iter()
        .map(|n| n["kind"].as_str().unwrap().to_owned())
        .collect()
}

/// How many `mention` entries the user has, read from the table (a guest
/// or an outsider has no inbox to ask).
async fn mentions_of(app: &TestApp, user_id: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM notifications WHERE user_id = $1 AND kind = 'mention'")
        .bind(Uuid::parse_str(user_id).unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap()
}

fn user_token(name: &str, id: &str) -> String {
    format!("@[{name}](user:{id})")
}

fn agent_token(name: &str, id: &str) -> String {
    format!("@[{name}](agent:{id})")
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_mentioned_member_hears_of_it_once(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let iid = issue(&app, &w, &w.eng, &w.owner, "Fix login").await;

    // The member mentions the owner, who created the issue: one `mention`, no `comment` besides.
    let body = format!("{} can you look?", user_token("Owner", &w.owner_id));
    let (status, posted) = comment(&app, &w.member, &iid, &body).await;
    assert_eq!(status, StatusCode::CREATED, "{posted}");
    assert_eq!(
        posted["body"],
        body.as_str(),
        "the tokens are kept as written"
    );
    assert_eq!(inbox(&app, &w, &w.owner).await, ["mention"]);
    let (_, list) = call(
        &app,
        Method::GET,
        &format!("{}/inbox", w.ws),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(list[0]["issue"]["identifier"], "ENG-1");
    assert_eq!(list[0]["actor"]["user_id"], w.member_id.as_str());

    // Without a mention the creator gets the usual `comment`.
    comment(&app, &w.member, &iid, "An update.").await;
    assert_eq!(inbox(&app, &w, &w.owner).await, ["comment", "mention"]);

    // The owner mentions the member twice and themself: the member hears once, the owner not at all.
    let body = format!(
        "{} and {} again, says {}",
        user_token("Member", &w.member_id),
        user_token("M.", &w.member_id),
        user_token("Owner", &w.owner_id)
    );
    let (status, _) = comment(&app, &w.owner, &iid, &body).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(inbox(&app, &w, &w.member).await, ["mention"]);
    assert_eq!(inbox(&app, &w, &w.owner).await, ["comment", "mention"]);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn only_members_who_can_see_the_issue_are_told(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let unknown = Uuid::now_v7().to_string();
    let everyone = [
        user_token("Member", &w.member_id),
        user_token("Guest", &w.guest_id),
        user_token("Outsider", &w.outsider_id),
        user_token("Nobody", &unknown),
    ]
    .join(" ");

    // On the private team's issue, neither the member nor the guest (outside the team) hears, and
    // nor do the outsider and an unknown id; the comment still goes through.
    let secret = issue(&app, &w, &w.secret, &w.owner, "Reorg").await;
    let (status, _) = comment(&app, &w.owner, &secret, &everyone).await;
    assert_eq!(status, StatusCode::CREATED);
    // On the public team's issue the member hears, a guest outside the team does not.
    let public = issue(&app, &w, &w.eng, &w.owner, "Fix login").await;
    comment(&app, &w.owner, &public, &everyone).await;
    assert_eq!(mentions_of(&app, &w.member_id).await, 1);
    assert_eq!(mentions_of(&app, &w.guest_id).await, 0);
    assert_eq!(mentions_of(&app, &w.outsider_id).await, 0);
    let total: i64 =
        sqlx::query_scalar("SELECT count(*) FROM notifications WHERE kind = 'mention'")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(total, 1);

    // Once in the teams, both hear of the next mention.
    join(&app, &w, &w.secret, &w.member_id).await;
    join(&app, &w, &w.eng, &w.guest_id).await;
    comment(&app, &w.owner, &secret, &everyone).await;
    comment(&app, &w.owner, &public, &everyone).await;
    assert_eq!(mentions_of(&app, &w.member_id).await, 3);
    assert_eq!(mentions_of(&app, &w.guest_id).await, 1);
    assert_eq!(mentions_of(&app, &w.outsider_id).await, 0);
    // The outsider's own workspace hears nothing of it either.
    let (_, theirs) = call(&app, Method::GET, "/workspaces", &w.outsider, None).await;
    let own = theirs[0]["id"].as_str().unwrap();
    assert_ne!(own, w.wid);
    let (_, list) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{own}/inbox"),
        &w.outsider,
        None,
    )
    .await;
    assert_eq!(list, json!([]));
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_comment_mentions_at_most_twenty(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let iid = issue(&app, &w, &w.eng, &w.owner, "Fix login").await;
    let many = |n: usize| -> String {
        (0..n)
            .map(|i| user_token(&format!("P{i}"), &Uuid::now_v7().to_string()))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let (status, problem) = comment(&app, &w.owner, &iid, &many(21)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert!(
        problem.to_string().contains("mentions at most 20"),
        "{problem}"
    );
    // The same person twenty-one times is one mention.
    let same = vec![user_token("Member", &w.member_id); 21].join(" ");
    assert_eq!(
        comment(&app, &w.owner, &iid, &same).await.0,
        StatusCode::CREATED
    );
    assert_eq!(
        comment(&app, &w.owner, &iid, &many(20)).await.0,
        StatusCode::CREATED
    );
    // An edit is held to the same limit.
    let (_, posted) = comment(&app, &w.owner, &iid, "Short.").await;
    let path = format!("/issues/{iid}/comments/{}", posted["id"].as_str().unwrap());
    let (status, _) = call(
        &app,
        Method::PATCH,
        &path,
        &w.owner,
        Some(json!({"body": many(21)})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_edit_tells_only_whom_it_adds(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    join(&app, &w, &w.eng, &w.guest_id).await;
    let iid = issue(&app, &w, &w.eng, &w.member, "Fix login").await;
    let member = user_token("Member", &w.member_id);
    let guest = user_token("Guest", &w.guest_id);

    let (_, posted) = comment(&app, &w.owner, &iid, &format!("{member} first")).await;
    let path = format!("/issues/{iid}/comments/{}", posted["id"].as_str().unwrap());
    assert_eq!(mentions_of(&app, &w.member_id).await, 1);
    // The member keeps their mention and the guest is added: only the guest hears.
    let edit = json!({"body": format!("{member} first, then {guest}")});
    let (status, edited) = call(&app, Method::PATCH, &path, &w.owner, Some(edit.clone())).await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert!(edited["edited_at"].is_string());
    assert_eq!(mentions_of(&app, &w.member_id).await, 1);
    assert_eq!(mentions_of(&app, &w.guest_id).await, 1);
    // Saving the same text again tells nobody; nor does an edit tell the watchers.
    call(&app, Method::PATCH, &path, &w.owner, Some(edit)).await;
    assert_eq!(mentions_of(&app, &w.member_id).await, 1);
    assert_eq!(mentions_of(&app, &w.guest_id).await, 1);
    assert_eq!(inbox(&app, &w, &w.member).await, ["mention"]);
}

/// Creates an agent in the caller's first workspace; returns its id.
async fn agent(app: &TestApp, token: &str, name: &str, role: &str) -> String {
    let body = json!({"name": name, "role": role});
    let (status, agent) = call(app, Method::POST, "/agents", token, Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{agent}");
    agent["id"].as_str().unwrap().to_owned()
}

/// The issue's `agent` timeline entries, as `(body, to)`.
async fn agent_entries(app: &TestApp, token: &str, iid: &str) -> Vec<(String, String)> {
    let (_, events) = call(
        app,
        Method::GET,
        &format!("/issues/{iid}/events"),
        token,
        None,
    )
    .await;
    events
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "agent")
        .map(|e| {
            assert!(e["actor"]["user_id"].is_string(), "the author asked: {e}");
            (
                e["body"].as_str().unwrap().to_owned(),
                e["to"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

/// [`agent_entries`] once there are at least `want`: a mention asks its agent
/// for a plan in a task of its own, which the comment's response may not
/// wait out.
async fn agent_entries_once(
    app: &TestApp,
    token: &str,
    iid: &str,
    want: usize,
) -> Vec<(String, String)> {
    for _ in 0..100 {
        let entries = agent_entries(app, token, iid).await;
        if entries.len() >= want {
            return entries;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    agent_entries(app, token, iid).await
}

/// The plans of a graph, oldest first, as `(status, instructions)`, once
/// none of them is still streaming.
async fn plans_of(app: &TestApp, graph_id: &str) -> Vec<(String, String)> {
    let gid = Uuid::parse_str(graph_id).unwrap();
    for _ in 0..300 {
        let plans: Vec<(String, String)> = sqlx::query_as(
            "SELECT status, instructions FROM plans WHERE graph_id = $1 ORDER BY created_at, id",
        )
        .bind(gid)
        .fetch_all(&app.state.db)
        .await
        .unwrap();
        if plans.iter().all(|(status, _)| status != "streaming") {
            return plans;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("a plan of {graph_id} is still streaming");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_mentioned_agent_is_delegated_the_issue_and_asked_for_a_plan(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let scout = agent(&app, &w.owner, "Scout", "scout").await;
    let tally = agent(&app, &w.owner, "Tally", "tally").await;
    let quill = agent(&app, &w.owner, "Quill", "quill").await;
    let (status, _) = call(
        &app,
        Method::PATCH,
        &format!("/agents/{quill}"),
        &w.owner,
        Some(json!({"status": "paused"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let elsewhere = agent(&app, &w.outsider, "Spy", "spy").await;

    // A paused agent, another workspace's agent and a member id as an agent do nothing.
    let iid = issue(&app, &w, &w.eng, &w.owner, "Compare tender portals").await;
    let body = format!(
        "{} {} {}",
        agent_token("Quill", &quill),
        agent_token("Spy", &elsewhere),
        agent_token("Member", &w.member_id)
    );
    assert_eq!(
        comment(&app, &w.member, &iid, &body).await.0,
        StatusCode::CREATED
    );
    let (_, found) = call(
        &app,
        Method::GET,
        &format!("/issues/{iid}"),
        &w.member,
        None,
    )
    .await;
    assert_eq!(
        (&found["agent_id"], &found["graph_id"]),
        (&Value::Null, &Value::Null)
    );
    assert!(agent_entries(&app, &w.member, &iid).await.is_empty());

    // The member is not in the team: the active agent is delegated the issue, but nobody plans.
    let body = format!(
        "{} please compare the two portals.",
        agent_token("Scout", &scout)
    );
    let (status, posted) = comment(&app, &w.member, &iid, &body).await;
    assert_eq!(status, StatusCode::CREATED, "{posted}");
    let (_, found) = call(
        &app,
        Method::GET,
        &format!("/issues/{iid}"),
        &w.member,
        None,
    )
    .await;
    assert_eq!(found["agent_id"], scout.as_str());
    assert_eq!(
        found["graph_id"],
        Value::Null,
        "only team members plan its work"
    );
    assert_eq!(
        agent_entries(&app, &w.member, &iid).await,
        [("delegated".to_owned(), "Scout".to_owned())]
    );

    // In the team, the next mention asks for a plan: the issue gets its graph and the plan waits
    // there to be applied; nothing is applied or run by itself.
    join(&app, &w, &w.eng, &w.member_id).await;
    let (status, _) = comment(&app, &w.member, &iid, &body).await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, found) = call(
        &app,
        Method::GET,
        &format!("/issues/{iid}"),
        &w.member,
        None,
    )
    .await;
    let gid = found["graph_id"]
        .as_str()
        .expect("the issue has a graph")
        .to_owned();
    assert_eq!(found["agent_id"], scout.as_str());
    let plans = plans_of(&app, &gid).await;
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].0, "ready");
    assert_eq!(
        plans[0].1,
        "@Scout please compare the two portals.\n\n\
         Hand the steps to the scout agent (Scout)."
    );
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM runs WHERE graph_id = $1")
        .bind(Uuid::parse_str(&gid).unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(runs, 0);
    assert_eq!(
        agent_entries_once(&app, &w.member, &iid, 2).await,
        [
            ("delegated".to_owned(), "Scout".to_owned()),
            ("asked to plan".to_owned(), "Scout".to_owned()),
        ]
    );

    // Already delegated, the issue keeps its agent; another mentioned agent is asked to plan.
    let body = format!("{} check the numbers.", agent_token("Tally", &tally));
    comment(&app, &w.member, &iid, &body).await;
    let (_, found) = call(
        &app,
        Method::GET,
        &format!("/issues/{iid}"),
        &w.member,
        None,
    )
    .await;
    assert_eq!(found["agent_id"], scout.as_str());
    let plans = plans_of(&app, &gid).await;
    assert_eq!(plans.len(), 2);
    assert!(
        plans[1]
            .1
            .ends_with("Hand the steps to the tally agent (Tally).")
    );

    // While a plan is still streaming the graph is busy: a mention asks for no other.
    sqlx::query("UPDATE plans SET status = 'streaming' WHERE graph_id = $1")
        .bind(Uuid::parse_str(&gid).unwrap())
        .execute(&app.state.db)
        .await
        .unwrap();
    let (status, _) = comment(&app, &w.member, &iid, &body).await;
    assert_eq!(status, StatusCode::CREATED, "the comment stands");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM plans WHERE graph_id = $1")
        .bind(Uuid::parse_str(&gid).unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(count, 2);
    assert_eq!(agent_entries_once(&app, &w.member, &iid, 3).await.len(), 3);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_mention_gives_no_graph_when_no_plan_could_start(pool: PgPool) {
    // No LLM key: a plan cannot start, so the issue is not given an empty graph.
    let app = TestApp::new(pool, &[("ANTHROPIC_API_KEY", "")]).await;
    let w = world(&app).await;
    let scout = agent(&app, &w.owner, "Scout", "scout").await;
    join(&app, &w, &w.eng, &w.member_id).await;
    let iid = issue(&app, &w, &w.eng, &w.member, "Compare tender portals").await;
    let body = format!("{} please compare.", agent_token("Scout", &scout));
    let (status, posted) = comment(&app, &w.member, &iid, &body).await;
    assert_eq!(status, StatusCode::CREATED, "the comment stands: {posted}");
    let (_, found) = call(
        &app,
        Method::GET,
        &format!("/issues/{iid}"),
        &w.member,
        None,
    )
    .await;
    assert_eq!(found["agent_id"], scout.as_str());
    assert_eq!(found["graph_id"], Value::Null);
    assert_eq!(
        agent_entries(&app, &w.member, &iid).await,
        [("delegated".to_owned(), "Scout".to_owned())]
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn edits_saved_at_the_same_moment_tell_a_member_once(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let iid = issue(&app, &w, &w.eng, &w.member, "Fix login").await;
    let (_, posted) = comment(&app, &w.owner, &iid, "Who can look?").await;
    let path = format!("/issues/{iid}/comments/{}", posted["id"].as_str().unwrap());
    let edit = json!({"body": format!("Who can look? {}", user_token("Member", &w.member_id))});
    let saves = (0..4).map(|_| call(&app, Method::PATCH, &path, &w.owner, Some(edit.clone())));
    for (status, body) in futures::future::join_all(saves).await {
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    assert_eq!(mentions_of(&app, &w.member_id).await, 1);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_issue_given_a_graph_twice_at_once_gets_one(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    join(&app, &w, &w.eng, &w.member_id).await;
    let iid = issue(&app, &w, &w.eng, &w.member, "Compare tender portals").await;
    let path = format!("/issues/{iid}/graph");
    let asks = (0..4).map(|_| call(&app, Method::POST, &path, &w.member, None));
    let mut linked = Vec::new();
    for (status, found) in futures::future::join_all(asks).await {
        assert_eq!(status, StatusCode::OK, "{found}");
        linked.push(found["graph_id"].as_str().unwrap().to_owned());
    }
    linked.dedup();
    assert_eq!(linked.len(), 1, "{linked:?}");
    let graphs: i64 = sqlx::query_scalar("SELECT count(*) FROM graphs WHERE team_id = $1")
        .bind(Uuid::parse_str(&w.eng).unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(graphs, 1);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_edit_that_adds_an_agent_hands_it_the_issue(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let scout = agent(&app, &w.owner, "Scout", "scout").await;
    let iid = issue(&app, &w, &w.eng, &w.member, "Compare tender portals").await;

    let (_, posted) = comment(&app, &w.member, &iid, "Who can take this?").await;
    let path = format!("/issues/{iid}/comments/{}", posted["id"].as_str().unwrap());
    let body = format!("Who can take this? {}", agent_token("Scout", &scout));
    let (status, _) = call(
        &app,
        Method::PATCH,
        &path,
        &w.member,
        Some(json!({"body": body})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, found) = call(
        &app,
        Method::GET,
        &format!("/issues/{iid}"),
        &w.member,
        None,
    )
    .await;
    assert_eq!(found["agent_id"], scout.as_str());
    // Saved again, the mention is not new: nothing more is recorded.
    call(
        &app,
        Method::PATCH,
        &path,
        &w.member,
        Some(json!({"body": body})),
    )
    .await;
    assert_eq!(
        agent_entries(&app, &w.member, &iid).await,
        [("delegated".to_owned(), "Scout".to_owned())]
    );
}

/// Replies to the comment `parent` as `token`; returns the status and the reply.
async fn reply(
    app: &TestApp,
    token: &str,
    iid: &str,
    parent: &str,
    body: &str,
) -> (StatusCode, Value) {
    let path = format!("/issues/{iid}/comments");
    let body = json!({"body": body, "parent_id": parent});
    call(app, Method::POST, &path, token, Some(body)).await
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_reply_tells_the_author_of_its_comment_once(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let (dana, _) = user(&app, "dana@example.com").await;
    let invite = json!({"email": "dana@example.com", "role": "member"});
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("{}/members", w.ws),
        &w.owner,
        Some(invite),
    )
    .await;
    assert!(status.is_success(), "{status}");
    // The owner files the issue and so watches it.
    let iid = issue(&app, &w, &w.eng, &w.owner, "Fix login").await;
    let (_, first) = comment(&app, &w.member, &iid, "Reproduced on staging.").await;
    let first = first["id"].as_str().unwrap().to_owned();
    assert_eq!(inbox(&app, &w, &w.owner).await, ["comment"]);

    // Dana replies: the comment's author hears, and the owner as the issue's watcher.
    let (status, posted) = reply(&app, &dana, &iid, &first, "Me too.").await;
    assert_eq!(status, StatusCode::CREATED, "{posted}");
    assert_eq!(inbox(&app, &w, &w.member).await, ["comment"]);
    assert_eq!(inbox(&app, &w, &w.owner).await, ["comment", "comment"]);
    // The owner replies: the author hears again, the owner of their own reply not at all.
    reply(&app, &w.owner, &iid, &first, "On it.").await;
    assert_eq!(inbox(&app, &w, &w.member).await, ["comment", "comment"]);
    assert_eq!(inbox(&app, &w, &w.owner).await.len(), 2);
    // The author replies to their own comment: only the watcher hears.
    reply(&app, &w.member, &iid, &first, "Thanks.").await;
    assert_eq!(inbox(&app, &w, &w.member).await.len(), 2);
    assert_eq!(inbox(&app, &w, &w.owner).await.len(), 3);

    // The owner watches the issue and wrote the comment replied to: one entry, not two.
    let (_, second) = comment(&app, &w.owner, &iid, "Which browser?").await;
    let second = second["id"].as_str().unwrap().to_owned();
    reply(&app, &w.member, &iid, &second, "Firefox.").await;
    assert_eq!(inbox(&app, &w, &w.owner).await, ["comment"; 4]);
    // A reply that mentions them tells them once, as a mention.
    let body = format!("{} and Safari.", user_token("Owner", &w.owner_id));
    reply(&app, &w.member, &iid, &second, &body).await;
    let owner = inbox(&app, &w, &w.owner).await;
    assert_eq!(
        (owner[0].as_str(), owner.len()),
        ("mention", 5),
        "{owner:?}"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_reply_answers_a_top_level_comment_of_the_same_issue(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let w = world(&app).await;
    let iid = issue(&app, &w, &w.eng, &w.owner, "Fix login").await;
    let events = format!("/issues/{iid}/events");
    let (_, first) = comment(&app, &w.owner, &iid, "Reproduced on staging.").await;
    let first = first["id"].as_str().unwrap().to_owned();

    let (status, answer) = reply(&app, &w.member, &iid, &first, "Me too.").await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    assert_eq!(
        (answer["kind"].as_str(), answer["parent_id"].as_str()),
        (Some("comment"), Some(first.as_str()))
    );
    assert_eq!(answer["reactions"], json!([]));
    let answer = answer["id"].as_str().unwrap().to_owned();
    let (_, list) = call(&app, Method::GET, &events, &w.member, None).await;
    let parents: Vec<&Value> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|e| &e["parent_id"])
        .collect();
    assert_eq!(parents, [&Value::Null, &json!(first)]);

    // Not a reply to a reply, a change, another issue's comment or nothing at all.
    let (status, _) = call(
        &app,
        Method::PATCH,
        &format!("/issues/{iid}"),
        &w.owner,
        Some(json!({"priority": 2})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, list) = call(&app, Method::GET, &events, &w.owner, None).await;
    let change = list
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "priority")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let elsewhere = issue(&app, &w, &w.eng, &w.owner, "Other").await;
    let (_, theirs) = comment(&app, &w.owner, &elsewhere, "Elsewhere.").await;
    let nowhere = Uuid::now_v7().to_string();
    for parent in [
        answer.as_str(),
        change.as_str(),
        theirs["id"].as_str().unwrap(),
        nowhere.as_str(),
    ] {
        let (status, problem) = reply(&app, &w.member, &iid, parent, "Hm.").await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
        assert!(problem["errors"]["parent_id"].is_array(), "{problem}");
    }
    // An edit cannot move a comment into a thread.
    let path = format!("/issues/{iid}/comments/{answer}");
    let edit = json!({"body": "Me too!", "parent_id": theirs["id"]});
    let (status, _) = call(&app, Method::PATCH, &path, &w.member, Some(edit)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // A reply goes alone; the comment takes its replies along.
    let (_, other) = reply(&app, &w.member, &iid, &first, "Also on Safari.").await;
    let other = format!("/issues/{iid}/comments/{}", other["id"].as_str().unwrap());
    let (status, _) = call(&app, Method::DELETE, &other, &w.member, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let comments = |list: &Value| -> usize {
        list.as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "comment")
            .count()
    };
    let (_, list) = call(&app, Method::GET, &events, &w.owner, None).await;
    assert_eq!(comments(&list), 2);
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/issues/{iid}/comments/{first}"),
        &w.owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, list) = call(&app, Method::GET, &events, &w.owner, None).await;
    assert_eq!(comments(&list), 0, "{list}");
}
