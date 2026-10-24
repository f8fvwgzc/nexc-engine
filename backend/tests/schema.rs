//! The schema every migration leaves behind (docs/adr/0001-http3-edge-caching-and-indexes.md):
//! every table has a primary key, every foreign key an index that a lookup by the key can
//! use, no index repeats another, and the projection triggers queue an export only when
//! what they project changed.
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode};
use common::TestApp;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

/// Tables of the schema with no primary key.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn every_table_has_a_primary_key(pool: PgPool) {
    let missing: Vec<String> = sqlx::query_scalar(
        "SELECT c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
          WHERE n.nspname = current_schema() AND c.relkind IN ('r', 'p')
            AND NOT EXISTS (SELECT 1 FROM pg_constraint k
                             WHERE k.conrelid = c.oid AND k.contype = 'p')
          ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(
        missing.is_empty(),
        "tables without a primary key: {missing:?}"
    );
}

/// A foreign key whose columns lead no index: deleting or re-keying the row it
/// points at would read the whole table. A partial index counts only when it
/// leaves out the rows without a key (`WHERE col IS NOT NULL`), which a lookup
/// by one value never reads.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn every_foreign_key_has_an_index(pool: PgPool) {
    let missing: Vec<String> = sqlx::query_scalar(
        "WITH fk AS (
           SELECT k.conname, k.conrelid, k.conkey, array_length(k.conkey, 1) AS n
             FROM pg_constraint k JOIN pg_class c ON c.oid = k.conrelid
             JOIN pg_namespace s ON s.oid = c.relnamespace
            WHERE k.contype = 'f' AND s.nspname = current_schema()
         )
         SELECT fk.conrelid::regclass::text || ' (' || fk.conname || ')'
           FROM fk
          WHERE NOT EXISTS (
            SELECT 1 FROM pg_index i
             WHERE i.indrelid = fk.conrelid AND i.indisvalid
               AND (string_to_array(i.indkey::text, ' ')::int2[])[1:fk.n] @> fk.conkey
               AND (string_to_array(i.indkey::text, ' ')::int2[])[1:fk.n] <@ fk.conkey
               AND (i.indpred IS NULL
                    OR (fk.n = 1 AND pg_get_expr(i.indpred, i.indrelid) =
                        '(' || (SELECT quote_ident(a.attname) FROM pg_attribute a
                                 WHERE a.attrelid = fk.conrelid AND a.attnum = fk.conkey[1])
                            || ' IS NOT NULL)')))
          ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(
        missing.is_empty(),
        "foreign keys without an index (add one, `WHERE col IS NOT NULL` for a nullable key): {missing:?}"
    );
}

/// Two indexes with the same definition, or a plain b-tree whose columns
/// lead another b-tree: the second costs every write and serves nothing the
/// first does not. The same definition is the same access method, keys or
/// expressions, operator classes, collations and orderings over the same
/// rows, so two copies of one partial, expression or unique index count too;
/// of such a pair the plain one is named, else the later.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn no_index_repeats_another(pool: PgPool) {
    let repeated: Vec<String> = sqlx::query_scalar(
        "SELECT a.indexrelid::regclass::text || ' repeats ' || b.indexrelid::regclass::text
           FROM pg_index a
           JOIN pg_index b ON a.indrelid = b.indrelid AND a.indexrelid <> b.indexrelid
           JOIN pg_class t ON t.oid = a.indrelid
           JOIN pg_namespace s ON s.oid = t.relnamespace
           JOIN pg_class ai ON ai.oid = a.indexrelid JOIN pg_am am ON am.oid = ai.relam
           JOIN pg_class bi ON bi.oid = b.indexrelid JOIN pg_am bm ON bm.oid = bi.relam
          WHERE s.nspname = current_schema()
            AND ((NOT a.indisunique AND NOT a.indisprimary
                  AND am.amname = 'btree' AND bm.amname = 'btree'
                  AND a.indexprs IS NULL AND b.indexprs IS NULL
                  AND a.indpred IS NULL AND b.indpred IS NULL
                  AND a.indnkeyatts <= b.indnkeyatts
                  AND (string_to_array(b.indkey::text, ' '))[1:a.indnkeyatts]
                      = (string_to_array(a.indkey::text, ' '))[1:a.indnkeyatts]
                  AND (string_to_array(b.indoption::text, ' '))[1:a.indnkeyatts]
                      = (string_to_array(a.indoption::text, ' '))[1:a.indnkeyatts]
                  AND (a.indnkeyatts < b.indnkeyatts OR a.indexrelid > b.indexrelid
                       OR b.indisunique))
                 OR (ai.relam = bi.relam
                     AND a.indnkeyatts = b.indnkeyatts
                     AND a.indkey::text = b.indkey::text
                     AND a.indclass::text = b.indclass::text
                     AND a.indcollation::text = b.indcollation::text
                     AND a.indoption::text = b.indoption::text
                     AND pg_get_expr(a.indexprs, a.indrelid)
                         IS NOT DISTINCT FROM pg_get_expr(b.indexprs, b.indrelid)
                     AND pg_get_expr(a.indpred, a.indrelid)
                         IS NOT DISTINCT FROM pg_get_expr(b.indpred, b.indrelid)
                     AND (a.indisprimary, a.indisunique, b.indexrelid)
                         < (b.indisprimary, b.indisunique, a.indexrelid)))
          ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(repeated.is_empty(), "redundant indexes: {repeated:?}");
}

async fn queued(pool: &PgPool, subject: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM onto_export_queue WHERE subject_id = $1")
        .bind(subject)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// An update that leaves every projected column as it was queues nothing;
/// one that changes one queues the row once.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn projection_triggers_skip_updates_that_change_nothing_projected(pool: PgPool) {
    let app = TestApp::new(pool.clone(), &[]).await;
    let (token, _) = app.register("owner@example.com").await;
    let list = app
        .request(Method::GET, "/api/v1/workspaces", Some(&token), None)
        .await;
    let wid = list.body[0]["id"].as_str().unwrap().to_owned();
    let team = app
        .request(
            Method::POST,
            &format!("/api/v1/workspaces/{wid}/teams"),
            Some(&token),
            Some(json!({"name": "Engineering", "key": "ENG"})),
        )
        .await;
    assert_eq!(team.status, StatusCode::CREATED, "{}", team.body);
    let tid = team.body["id"].as_str().unwrap().to_owned();
    let issue = app
        .request(
            Method::POST,
            &format!("/api/v1/workspaces/{wid}/teams/{tid}/issues"),
            Some(&token),
            Some(json!({"title": "Ship it"})),
        )
        .await;
    assert_eq!(issue.status, StatusCode::CREATED, "{}", issue.body);
    let iid: Uuid = issue.body["id"].as_str().unwrap().parse().unwrap();
    let tid: Uuid = tid.parse().unwrap();

    let cases: [(&str, Uuid, &str, &str); 3] = [
        (
            "issues",
            iid,
            "UPDATE issues SET title = title, priority = priority, description = 'more' WHERE id = $1",
            "UPDATE issues SET title = title || '!' WHERE id = $1",
        ),
        (
            "teams",
            tid,
            "UPDATE teams SET name = name, key = key, private = private WHERE id = $1",
            "UPDATE teams SET name = name || '!' WHERE id = $1",
        ),
        (
            "agents",
            Uuid::nil(),
            "UPDATE agents SET name = name, status = status WHERE id = $1",
            "UPDATE agents SET model = model || '-next' WHERE id = $1",
        ),
    ];
    for (table, id, unchanged, changed) in cases {
        let id = if table == "agents" {
            sqlx::query_scalar("SELECT id FROM agents WHERE workspace_id = $1 LIMIT 1")
                .bind(wid.parse::<Uuid>().unwrap())
                .fetch_one(&pool)
                .await
                .unwrap()
        } else {
            id
        };
        let before = queued(&pool, id).await;
        sqlx::query(unchanged)
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            queued(&pool, id).await,
            before,
            "{table}: nothing projected changed"
        );
        sqlx::query(changed).bind(id).execute(&pool).await.unwrap();
        assert_eq!(
            queued(&pool, id).await,
            before + 1,
            "{table}: a projected column changed"
        );
    }
}
