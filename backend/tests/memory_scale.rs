//! Memory of a workspace too large to hold in process: searched, paged and
//! read through the database, with or without a vector index.
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode};
use common::TestApp;
use nexc::domain::memory::{MemoryKind, MemoryScope};
use nexc::memory::{self, View};
use nexc::repo::memories::{self, NewMemory};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

#[test]
fn a_query_becomes_its_longest_distinct_terms() {
    assert_eq!(
        memory::any_term_query("Gold reacts to the real yields; gold, YIELDS!"),
        "reacts | yields | gold | real"
    );
    assert_eq!(memory::any_term_query("a to of — !?"), "");
    let common = ["yields".to_owned()].into_iter().collect();
    assert_eq!(
        memory::any_term_query_without("Gold reacts to real yields", &common),
        "reacts | gold | real"
    );
    let long: String = (0..40).map(|i| format!("word{i:02}x ")).collect();
    assert_eq!(memory::any_term_query(&long).split(" | ").count(), 16);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_large_workspace_is_searched_in_the_database(pool: PgPool) {
    let app = TestApp::new(pool.clone(), &[]).await;
    let (token, _) = app.register("scale@example.com").await;
    let me = app
        .request(Method::GET, "/api/v1/auth/me", Some(&token), None)
        .await;
    let user: Uuid = me.body["id"].as_str().unwrap().parse().unwrap();
    let graph = app
        .request(
            Method::POST,
            "/api/v1/graphs",
            Some(&token),
            Some(json!({"name": "G"})),
        )
        .await;
    assert_eq!(graph.status, StatusCode::CREATED);
    let graph_id: Uuid = graph.body["id"].as_str().unwrap().parse().unwrap();
    let workspace: Uuid = graph.body["workspace_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();

    let learn = |content: String, owner: Uuid, graph: Option<Uuid>| {
        let pool = pool.clone();
        async move {
            let embedding = nexc::kernel::embed(&content);
            let new = NewMemory {
                owner_id: owner,
                workspace_id: Some(workspace),
                scope: if graph.is_some() {
                    MemoryScope::Graph
                } else {
                    MemoryScope::User
                },
                graph_id: graph,
                node_id: None,
                kind: MemoryKind::Fact,
                content: &content,
                embedding: &embedding,
                importance: 0.5,
            };
            memories::insert(&pool, &new).await.unwrap()
        }
    };
    // The oldest memory is the one looked for; sixty newer ones bury it.
    let target = learn(
        "Invoices from the Rotterdam warehouse need a customs reference".into(),
        user,
        Some(graph_id),
    )
    .await;
    let stranger = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, name, role, password_hash) VALUES ($1, $2, 'S', 'user', 'x')")
        .bind(stranger)
        .bind("stranger@example.com")
        .execute(&pool)
        .await
        .unwrap();
    learn(
        "Rotterdam warehouse customs gossip that is private to someone else".into(),
        stranger,
        None,
    )
    .await;
    for i in 0..60 {
        learn(
            format!("Filler note number {i} about scheduling and office plants"),
            user,
            Some(graph_id),
        )
        .await;
    }

    let state = &app.state;
    // Treat the workspace as large: nothing of it is held in process.
    state.memories.set_ann_threshold(10);
    let query = "customs reference for Rotterdam invoices";
    let found = memory::retrieve(&state.memories, &pool, user, workspace, View::All, query, 3)
        .await
        .unwrap();
    assert_eq!(found[0].id, target, "{found:?}");
    assert!(
        found.iter().all(|m| !m.content.contains("gossip")),
        "another member's own notes are never candidates"
    );
    // The same answer as the exact scan of a small workspace.
    state.memories.set_ann_threshold(usize::MAX);
    let exact = memory::retrieve(&state.memories, &pool, user, workspace, View::All, query, 3)
        .await
        .unwrap();
    assert_eq!(exact[0].id, target);
    state.memories.set_ann_threshold(10);

    // A query with no word worth searching still answers, from the recent memories.
    let vague = memory::retrieve(
        &state.memories,
        &pool,
        user,
        workspace,
        View::All,
        "a of to",
        3,
    )
    .await
    .unwrap();
    assert_eq!(vague.len(), 3);

    // Paging reaches the oldest memory, and it opens, however many came after it.
    let page = |offset: usize| {
        let pool = pool.clone();
        async move {
            memory::visible(&pool, user, workspace, View::All, 25, offset)
                .await
                .unwrap()
        }
    };
    let (first, second, third) = (page(0).await, page(25).await, page(50).await);
    assert_eq!((first.len(), second.len(), third.len()), (25, 25, 11));
    assert_eq!(third.last().unwrap().id, target, "oldest last");
    let one = app
        .request(
            Method::GET,
            &format!("/api/v1/memories/{target}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(one.status, StatusCode::OK, "{:?}", one.body);
    let other = memories::find_stored(&pool, target).await.unwrap().unwrap();
    assert_eq!(other.owner_id, user);
    assert!(
        memory::find(&pool, stranger, workspace, target)
            .await
            .unwrap()
            .is_none(),
        "someone outside the graph cannot open it"
    );
}
