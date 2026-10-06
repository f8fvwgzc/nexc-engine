//! Memory retrieval through pgvector. Skipped when the server has no `vector` extension.
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode};
use common::TestApp;
use nexc::domain::memory::{MemoryKind, MemoryScope};
use nexc::memory::{self, View, vectors};
use nexc::repo::memories::{self, NewMemory};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn large_workspaces_are_searched_with_hnsw(pool: PgPool) {
    if !vectors::ensure(&pool).await.unwrap() {
        eprintln!("skipped: this PostgreSQL server has no pgvector extension");
        return;
    }
    assert!(
        vectors::ensure(&pool).await.unwrap(),
        "ensure is idempotent"
    );
    let index: Option<String> = sqlx::query_scalar(
        "SELECT indexdef FROM pg_indexes WHERE indexname = 'memories_embedding_hnsw'",
    )
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert!(index.unwrap().contains("hnsw"));

    let app = TestApp::new(pool.clone(), &[]).await;
    let (token, _) = app.register("vectors@example.com").await;
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

    let learn = |content: String, graph: Option<Uuid>| {
        let pool = pool.clone();
        async move {
            let embedding = nexc::kernel::embed(&content);
            let new = NewMemory {
                owner_id: user,
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
    for i in 0..40 {
        learn(
            format!(
                "Office note {i}: the kitchen rota changes on weekday {}",
                i % 5
            ),
            Some(graph_id),
        )
        .await;
    }
    let target = learn(
        "Gold rallies when the ten-year real yield falls".into(),
        Some(graph_id),
    )
    .await;
    // Rows written before the column existed (or by an instance without it) are converted.
    assert_eq!(vectors::backfill(&pool).await.unwrap(), 41);
    assert_eq!(vectors::backfill(&pool).await.unwrap(), 0);

    // Force the database path, which a real workspace takes from 5 000 memories on.
    let state = &app.state;
    state.memories.set_vector_search(true);
    state.memories.set_ann_threshold(0);
    let query = "what makes gold rally: real yield";
    let found = memory::retrieve(
        &state.memories,
        &pool,
        user,
        workspace,
        View::Prefer(graph_id),
        query,
        3,
    )
    .await
    .unwrap();
    assert_eq!(found[0].id, target, "{found:?}");
    assert_eq!(found.len(), 3);

    // The exact in-process scan agrees on the best match.
    state.memories.set_ann_threshold(usize::MAX);
    let exact = memory::retrieve(
        &state.memories,
        &pool,
        user,
        workspace,
        View::Prefer(graph_id),
        query,
        3,
    )
    .await
    .unwrap();
    assert_eq!(exact[0].id, target);

    // Visibility still applies to what the database returns: a stranger reads nothing.
    state.memories.set_ann_threshold(0);
    let stranger = Uuid::now_v7();
    let none = memory::retrieve(
        &state.memories,
        &pool,
        stranger,
        workspace,
        View::All,
        query,
        3,
    )
    .await
    .unwrap();
    assert!(none.is_empty());
}
