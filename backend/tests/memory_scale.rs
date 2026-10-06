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
            memory::visible(&pool, user, workspace, View::All, None, 25, offset)
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

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn memory_is_grouped_into_topics_named_from_shared_memories(pool: PgPool) {
    let app = TestApp::new(pool.clone(), &[]).await;
    let (token, _) = app.register("topics@example.com").await;
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
    let subjects = [
        "customs clearance invoices port tariff declaration",
        "plants watering soil pruning greenhouse compost",
        "payroll salary payslip overtime pension bonus",
    ];
    for words in subjects {
        for i in 0..10 {
            learn(
                format!("Note {i}: {words} for the team"),
                user,
                Some(graph_id),
            )
            .await;
        }
    }
    // Someone else's own note: placed under a topic, but never part of a name.
    let stranger = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, email, name, role, password_hash) VALUES ($1, $2, 'S', 'user', 'x')",
    )
    .bind(stranger)
    .bind("stranger@example.com")
    .execute(&pool)
    .await
    .unwrap();
    for i in 0..6 {
        let note = format!("Private {i}: acquisition zanzibar confidential merger payroll");
        learn(note, stranger, None).await;
    }

    // Nothing until the topics are found.
    let path = format!("/api/v1/memories/topics?workspace_id={workspace}");
    let none = app.request(Method::GET, &path, Some(&token), None).await;
    assert_eq!(none.body.as_array().unwrap().len(), 0);

    let found = memory::topics::rebuild(&pool, workspace).await.unwrap();
    assert!(found >= 3, "{found}");
    let listed = app.request(Method::GET, &path, Some(&token), None).await;
    assert_eq!(listed.status, StatusCode::OK);
    let topics = listed.body.as_array().unwrap().clone();
    let words: Vec<String> = topics
        .iter()
        .flat_map(|t| t["terms"].as_array().unwrap().clone())
        .map(|w| w.as_str().unwrap().to_owned())
        .collect();
    for expected in ["customs", "payroll"] {
        assert!(
            words.iter().any(|w| w == expected),
            "{expected} in {words:?}"
        );
    }
    for private in ["zanzibar", "acquisition", "confidential", "merger"] {
        assert!(
            !words.iter().any(|w| w == private),
            "{private} leaked into {words:?}"
        );
    }
    // The reader's counts cover exactly what they may read: the thirty shared memories.
    let counted: i64 = topics
        .iter()
        .map(|t| t["memory_count"].as_i64().unwrap())
        .sum();
    assert_eq!(counted, 30);

    // The list can be kept to one topic.
    let customs = topics
        .iter()
        .find(|t| {
            t["terms"]
                .as_array()
                .unwrap()
                .iter()
                .any(|w| w == "customs")
        })
        .unwrap();
    let within = format!(
        "/api/v1/memories?workspace_id={workspace}&limit=50&topic_id={}",
        customs["id"].as_str().unwrap()
    );
    let page = app.request(Method::GET, &within, Some(&token), None).await;
    let page = page.body.as_array().unwrap().clone();
    assert_eq!(page.len() as i64, customs["memory_count"].as_i64().unwrap());
    assert!(
        page.iter()
            .all(|m| m["content"].as_str().unwrap().contains("customs"))
    );

    // A memory learned later joins the nearest topic without a rebuild.
    let late = learn(
        "Late: customs tariff declaration at the port".into(),
        user,
        Some(graph_id),
    )
    .await;
    memory::topics::place_new(&pool, workspace).await.unwrap();
    let placed: Option<Uuid> = sqlx::query_scalar("SELECT topic_id FROM memories WHERE id = $1")
        .bind(late)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        placed.map(|id| id.to_string()).as_deref(),
        customs["id"].as_str()
    );

    // Only admins rebuild; outsiders do not see the workspace.
    let (other, _) = app.register("other@example.com").await;
    let rebuild = format!("/api/v1/memories/topics/rebuild?workspace_id={workspace}");
    let denied = app
        .request(Method::POST, &rebuild, Some(&other), None)
        .await;
    assert_eq!(denied.status, StatusCode::NOT_FOUND);
    let started = app
        .request(Method::POST, &rebuild, Some(&token), None)
        .await;
    assert_eq!(started.status, StatusCode::ACCEPTED);
}
