//! Evidence that survives (docs/AGENTSECOPS.md §12; the evidence-bound
//! outputs of §2 are `tests/evidence.rs`): a deleted graph's runs keep their
//! hash-chained trace and their signed seal, and the deletion is audited;
//! spans and audit entries refuse to be changed or removed in the database
//! itself; a workspace's audit log is a hash chain that verifies, shows a
//! changed entry or a removed tail, and arrives whole through a transfer or
//! not at all; a run's evidence bundle verifies offline with the published
//! key alone (the `nexc-verify` binary), and a changed span, seal or audit
//! entry in it is found; the retention purge is the way evidence leaves, no
//! sooner than 30 days, and it is audited too.
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode};
use common::{TestApp, collect_until};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

async fn call(
    app: &TestApp,
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let r = app
        .request(method, &format!("/api/v1{path}"), token, body)
        .await;
    (r.status, r.body)
}

async fn get(app: &TestApp, token: &str, path: &str) -> Value {
    let (status, body) = call(app, Method::GET, path, Some(token), None).await;
    assert_eq!(status, StatusCode::OK, "GET {path}: {body}");
    body
}

/// A finished run of a two-step graph on the built-in executor.
struct Finished {
    app: TestApp,
    token: String,
    user: String,
    wid: String,
    gid: String,
    rid: String,
}

async fn finished_run(pool: PgPool) -> Finished {
    let app = TestApp::new(pool, &[]).await;
    let (token, _) = app.register("keeper@example.com").await;
    let user = get(&app, &token, "/auth/me").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let wid = get(&app, &token, "/workspaces").await[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let gid = app.graph(&token, "Ship the quarterly report").await;
    let outline = app
        .node(&token, &gid, json!({"title": "Outline", "kind": "task"}))
        .await;
    let draft = app
        .node(&token, &gid, json!({"title": "Draft", "kind": "task"}))
        .await;
    app.edge(&token, &gid, &outline, &draft).await;
    let mut rx = app.events(&gid);
    let (status, run) = call(
        &app,
        Method::POST,
        &format!("/graphs/{gid}/runs"),
        Some(&token),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{run}");
    let events = collect_until(&mut rx, "run.finished").await;
    assert_eq!(events.last().unwrap().1["run"]["status"], "succeeded");
    Finished {
        rid: run["id"].as_str().unwrap().to_owned(),
        app,
        token,
        user,
        wid,
        gid,
    }
}

async fn span_count(pool: &PgPool, rid: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM agent_spans WHERE run_id = $1::uuid")
        .bind(rid)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// A statement the database must refuse, with the reason it gives.
async fn refused(pool: &PgPool, sql: &str) -> String {
    let err = sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
        .execute(pool)
        .await
        .expect_err(&format!("`{sql}` must be refused"));
    err.to_string()
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_deleted_graph_keeps_its_runs_evidence_and_the_deletion_is_audited(pool: PgPool) {
    let f = finished_run(pool.clone()).await;
    let (app, token, rid, gid, wid) = (&f.app, f.token.as_str(), &f.rid, &f.gid, &f.wid);
    let before = get(app, token, &format!("/runs/{rid}/trace/verify")).await;
    assert_eq!(before["ok"], true, "{before}");
    let spans = before["spans"].as_i64().unwrap();
    assert!(spans >= 4, "data_read and llm_call per step: {before}");
    // The seal is signed with the key published at the well-known address.
    let jwks = app
        .request(Method::GET, "/.well-known/jwks.json", None, None)
        .await;
    assert_eq!(jwks.status, StatusCode::OK);
    let kid = jwks.body["keys"][0]["kid"].as_str().unwrap().to_owned();
    assert_eq!(jwks.body["keys"][0]["status"], "current");
    assert_eq!(before["seal"]["ok"], true, "{before}");
    assert_eq!(before["seal"]["kid"], kid.as_str());
    let run = get(app, token, &format!("/runs/{rid}")).await;
    assert_eq!(run["seal_kid"], kid.as_str());

    let (status, _) = call(
        app,
        Method::DELETE,
        &format!("/graphs/{gid}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Gone for work: not listed, not opened, not run, not deleted twice.
    let listed = get(app, token, "/graphs").await;
    assert!(!listed.to_string().contains(gid.as_str()), "{listed}");
    for (method, path) in [
        (Method::GET, format!("/graphs/{gid}")),
        (Method::POST, format!("/graphs/{gid}/runs")),
        (Method::DELETE, format!("/graphs/{gid}")),
    ] {
        let body = (method == Method::POST).then(|| json!({}));
        let (status, _) = call(app, method, &path, Some(token), body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }

    // Kept as evidence: the run, its spans, its chain and its seal.
    assert_eq!(span_count(&pool, rid).await, spans);
    let after = get(app, token, &format!("/runs/{rid}/trace/verify")).await;
    assert_eq!(after["ok"], true, "{after}");
    assert_eq!(after["spans"], before["spans"]);
    assert_eq!(after["sealed_hash"], before["sealed_hash"]);
    assert_eq!(after["seal"]["ok"], true);
    let page = get(app, token, &format!("/runs/{rid}/trace?limit=1000")).await;
    assert_eq!(page["spans"].as_array().unwrap().len() as i64, spans);
    let runs = get(app, token, &format!("/graphs/{gid}/runs")).await;
    assert_eq!(
        runs[0]["id"],
        rid.as_str(),
        "a deleted graph's runs are still listed"
    );
    for path in [
        format!("/runs/{rid}"),
        format!("/runs/{rid}/sessions"),
        format!("/runs/{rid}/artifacts"),
    ] {
        get(app, token, &path).await;
    }

    // On record, by whom.
    let audit = get(app, token, &format!("/workspaces/{wid}/audit")).await;
    let deleted = audit
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["action"] == "graph_deleted")
        .unwrap_or_else(|| panic!("no graph_deleted entry: {audit}"));
    assert_eq!(deleted["actor_id"], f.user.as_str());
    assert_eq!(deleted["actor_name"], "Test");
    assert_eq!(deleted["subject"], "G");
    let detail = deleted["detail"].as_str().unwrap();
    assert!(
        detail.contains(gid.as_str())
            && detail.contains("1 run(s) kept")
            && detail.contains("1 sealed"),
        "{detail}"
    );
    let chain = get(app, token, &format!("/workspaces/{wid}/audit/verify")).await;
    assert_eq!(chain["ok"], true, "{chain}");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn spans_and_audit_entries_refuse_to_change_or_go(pool: PgPool) {
    let f = finished_run(pool.clone()).await;
    let (app, token, rid, wid) = (&f.app, f.token.as_str(), &f.rid, &f.wid);
    for sql in [
        "UPDATE agent_spans SET name = 'rewritten'".to_owned(),
        format!("DELETE FROM agent_spans WHERE run_id = '{rid}'"),
        "TRUNCATE agent_spans".to_owned(),
        "UPDATE audit_log SET detail = 'rewritten'".to_owned(),
        "UPDATE audit_log SET actor_name = 'Mallory'".to_owned(),
        format!("DELETE FROM audit_log WHERE workspace_id = '{wid}'"),
        "TRUNCATE audit_log".to_owned(),
    ] {
        let reason = refused(&pool, &sql).await;
        assert!(reason.contains("append-only"), "{sql}: {reason}");
    }
    let reason = refused(
        &pool,
        &format!("UPDATE runs SET trace_hash = decode(repeat('00', 32), 'hex') WHERE id = '{rid}'"),
    )
    .await;
    assert!(reason.contains("is final"), "{reason}");
    let reason = refused(
        &pool,
        &format!("UPDATE runs SET seal_kid = NULL, seal_signed = NULL, seal_signature = NULL WHERE id = '{rid}'"),
    )
    .await;
    assert!(
        reason.contains("is final"),
        "a seal cannot be stripped either: {reason}"
    );
    // Deleting the run itself would take its spans: refused the same way.
    let reason = refused(&pool, &format!("DELETE FROM runs WHERE id = '{rid}'")).await;
    assert!(reason.contains("append-only"), "{reason}");

    // The purge paths say so first, in their own transaction.
    let mut tx = pool.begin().await.unwrap();
    nexc::repo::evidence::allow_purge(&mut tx).await.unwrap();
    let gone = sqlx::query("DELETE FROM agent_spans WHERE run_id = $1::uuid")
        .bind(rid)
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(gone.rows_affected() > 0);
    tx.rollback().await.unwrap();
    // Erasing a name is the one change: the salt goes, nothing chained moves.
    let mut tx = pool.begin().await.unwrap();
    nexc::repo::evidence::allow_redact(&mut tx).await.unwrap();
    for sql in [
        "UPDATE audit_log SET detail = 'rewritten', salt = NULL",
        "UPDATE audit_log SET actor_name = 'Mallory'",
    ] {
        let mut savepoint = sqlx::Acquire::begin(&mut *tx).await.unwrap();
        let err = sqlx::query(sql).execute(&mut *savepoint).await.unwrap_err();
        assert!(err.to_string().contains("append-only"), "{sql}: {err}");
        savepoint.rollback().await.unwrap();
    }
    sqlx::query("UPDATE audit_log SET actor_name = 'Deleted account', salt = NULL")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    // The setting ends with its transaction.
    let reason = refused(&pool, "DELETE FROM agent_spans").await;
    assert!(reason.contains("append-only"), "{reason}");

    // Whoever owns the tables can drop the guard; the chains still show the change.
    let mut tx = pool.begin().await.unwrap();
    for sql in [
        "ALTER TABLE audit_log DISABLE TRIGGER audit_log_append_only",
        "UPDATE audit_log SET detail = detail || ' (edited)' WHERE seq = 0",
        "ALTER TABLE audit_log ENABLE TRIGGER audit_log_append_only",
        "ALTER TABLE agent_spans DISABLE TRIGGER agent_spans_append_only",
        "UPDATE agent_spans SET name = name || '!' WHERE seq = 1",
        "ALTER TABLE agent_spans ENABLE TRIGGER agent_spans_append_only",
    ] {
        sqlx::query(sql).execute(&mut *tx).await.unwrap();
    }
    tx.commit().await.unwrap();
    let chain = get(app, token, &format!("/workspaces/{wid}/audit/verify")).await;
    assert_eq!(chain["ok"], false, "{chain}");
    assert_eq!(chain["broken"]["seq"], 0);
    assert!(
        chain["broken"]["reason"]
            .as_str()
            .unwrap()
            .contains("content"),
        "{chain}"
    );
    let trace = get(app, token, &format!("/runs/{rid}/trace/verify")).await;
    assert_eq!(trace["ok"], false);
    assert_eq!(trace["broken"]["seq"], 1, "{trace}");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_audit_chain_is_the_one_the_database_wrote_and_a_removed_tail_shows(pool: PgPool) {
    let f = finished_run(pool.clone()).await;
    let (app, token, wid) = (&f.app, f.token.as_str(), &f.wid);
    // A few more entries of other kinds.
    let (status, _) = call(
        app,
        Method::DELETE,
        &format!("/graphs/{}", f.gid),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let guardrails = get(app, token, &format!("/workspaces/{wid}/guardrails")).await;
    let (status, body) = call(
        app,
        Method::PUT,
        &format!("/workspaces/{wid}/guardrails"),
        Some(token),
        Some(guardrails),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let entries: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE workspace_id = $1::uuid")
            .bind(wid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(entries >= 3, "{entries}");
    let chain = get(app, token, &format!("/workspaces/{wid}/audit/verify")).await;
    assert_eq!(chain["ok"], true, "{chain}");
    assert_eq!(chain["entries"], entries);
    assert_eq!(chain["head_seq"], entries - 1);
    assert_eq!(chain["hash"], chain["head_hash"]);
    assert_eq!(chain["redacted"], 0);

    // The backend recomputes exactly what the database computed.
    let wid_uuid: Uuid = wid.parse().unwrap();
    let rows = nexc::repo::audit::chain_page(&pool, wid_uuid, None, 1000)
        .await
        .unwrap();
    let mut prev = nexc::domain::audit_chain::GENESIS;
    for (seq, entry) in rows.iter().enumerate() {
        assert_eq!(
            nexc::domain::audit_chain::misfit(entry, seq as i64, &prev),
            None
        );
        prev = entry.hash;
    }

    // The last entry removed (even by a purge path): the chain ends short of its head.
    let mut tx = pool.begin().await.unwrap();
    nexc::repo::evidence::allow_purge(&mut tx).await.unwrap();
    sqlx::query("DELETE FROM audit_log WHERE workspace_id = $1::uuid AND seq = $2")
        .bind(wid)
        .bind(entries - 1)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let chain = get(app, token, &format!("/workspaces/{wid}/audit/verify")).await;
    assert_eq!(chain["ok"], false, "{chain}");
    assert!(chain["broken"]["entry_id"].is_null());
    assert!(
        chain["broken"]["reason"]
            .as_str()
            .unwrap()
            .contains("removed"),
        "{chain}"
    );
}

/// Writes `value` to a file in `dir`.
fn write(dir: &std::path::Path, name: &str, value: &Value) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    path
}

/// Runs `nexc-verify` on a bundle with a key set; its exit code and output.
fn nexc_verify(bundle: &std::path::Path, keys: &std::path::Path) -> (i32, String) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_nexc-verify"))
        .arg(bundle)
        .arg("--keys")
        .arg(keys)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_evidence_bundle_verifies_offline_and_tampering_is_found(pool: PgPool) {
    let f = finished_run(pool).await;
    let (app, token, rid, gid) = (&f.app, f.token.as_str(), &f.rid, &f.gid);
    // The graph is deleted first: its runs' evidence still exports.
    let (status, _) = call(
        app,
        Method::DELETE,
        &format!("/graphs/{gid}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let exported = app
        .request(
            Method::GET,
            &format!("/api/v1/runs/{rid}/evidence"),
            Some(token),
            None,
        )
        .await;
    assert_eq!(exported.status, StatusCode::OK, "{}", exported.body);
    assert!(
        exported.headers["content-disposition"]
            .to_str()
            .unwrap()
            .contains(&format!("run-{rid}.evidence.json"))
    );
    let bundle = exported.body;
    assert_eq!(bundle["format"], "nexc-evidence-bundle/v1");
    assert!(bundle["run"]["graph_deleted_at"].is_string());
    let actions: Vec<&str> = bundle["audit"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    for action in ["trace_sealed", "graph_deleted", "evidence_exported"] {
        assert!(actions.contains(&action), "{actions:?}");
    }
    let jwks = app
        .request(Method::GET, "/.well-known/jwks.json", None, None)
        .await
        .body;

    // In process, with the published keys alone.
    let read: nexc::domain::bundle::EvidenceBundle =
        serde_json::from_value(bundle.clone()).unwrap();
    let keys: nexc::security::signing::Jwks = serde_json::from_value(jwks.clone()).unwrap();
    let report = nexc::domain::bundle::verify(&read, &keys);
    assert!(report.ok, "{report:#?}");

    // And with the binary, no database anywhere.
    let dir = tempfile_dir();
    let keys_path = write(&dir, "jwks.json", &jwks);
    let (code, out) = nexc_verify(&write(&dir, "bundle.json", &bundle), &keys_path);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("the bundle verifies"), "{out}");

    let tamper = |name: &str, edit: &dyn Fn(&mut Value)| {
        let mut changed = bundle.clone();
        edit(&mut changed);
        let (code, out) = nexc_verify(&write(&dir, name, &changed), &keys_path);
        assert_eq!(code, 1, "{name} must not verify: {out}");
        out
    };
    let out = tamper("span.json", &|b| {
        b["spans"][1]["attributes"]["tokens_out"] = json!(1);
    });
    assert!(out.contains("FAIL spans"), "{out}");
    let out = tamper("seal.json", &|b| {
        let statement = b["seal"]["statement"]
            .as_str()
            .unwrap()
            .replace("\"type\"", "\"type\" ");
        b["seal"]["statement"] = json!(statement);
    });
    assert!(out.contains("FAIL seal"), "{out}");
    let out = tamper("audit.json", &|b| {
        let entries = b["audit"].as_array_mut().unwrap();
        let sealed = entries
            .iter_mut()
            .find(|e| e["action"] == "trace_sealed")
            .unwrap();
        sealed["detail"] = json!("0 span(s)");
    });
    assert!(out.contains("FAIL audit"), "{out}");
    let out = tamper("dropped.json", &|b| {
        let entries = b["audit"].as_array_mut().unwrap();
        entries.retain(|e| e["action"] != "graph_deleted");
    });
    assert!(out.contains("FAIL attestation"), "{out}");

    // Another key: nothing nexc signed verifies with it.
    let other = nexc::security::signing::Signer::from_seed(&[7; 32]).unwrap();
    let wrong = write(
        &dir,
        "other.json",
        &json!({"keys": [nexc::security::signing::Jwk::of(&other.public_key(), nexc::security::signing::KeyStatus::Current)]}),
    );
    let (code, out) = nexc_verify(&write(&dir, "bundle.json", &bundle), &wrong);
    assert_eq!(code, 1, "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

fn tempfile_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("nexc-evidence-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_retention_purge_removes_a_deleted_graph_and_says_so(pool: PgPool) {
    let f = finished_run(pool.clone()).await;
    let (app, token, rid, gid, wid) = (&f.app, f.token.as_str(), &f.rid, &f.gid, &f.wid);
    let (status, _) = call(
        app,
        Method::DELETE,
        &format!("/graphs/{gid}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Within the retention (30 days by default), nothing goes.
    nexc::engine::evidence::purge_expired(&app.state)
        .await
        .unwrap();
    assert!(span_count(&pool, rid).await > 0);
    // Nor with the retention cut to one day: a deleted graph's evidence stays 30 days at least.
    let mut guardrails = get(app, token, &format!("/workspaces/{wid}/guardrails")).await;
    guardrails["recording"]["retention_days"] = json!(1);
    let (status, body) = call(
        app,
        Method::PUT,
        &format!("/workspaces/{wid}/guardrails"),
        Some(token),
        Some(guardrails),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for sql in [
        "UPDATE graphs SET deleted_at = now() - interval '2 days' WHERE id = $1::uuid",
        "UPDATE runs SET finished_at = now() - interval '2 days' WHERE graph_id = $1::uuid",
    ] {
        sqlx::query(sql).bind(gid).execute(&pool).await.unwrap();
    }
    nexc::engine::evidence::purge_expired(&app.state)
        .await
        .unwrap();
    assert!(
        span_count(&pool, rid).await > 0,
        "kept for the 30-day floor"
    );

    // Deleted 31 days ago, its last run ended 31 days ago: it goes, with everything in it.
    for sql in [
        "UPDATE graphs SET deleted_at = now() - interval '31 days' WHERE id = $1::uuid",
        "UPDATE runs SET finished_at = now() - interval '31 days' WHERE graph_id = $1::uuid",
    ] {
        sqlx::query(sql).bind(gid).execute(&pool).await.unwrap();
    }
    nexc::engine::evidence::purge_expired(&app.state)
        .await
        .unwrap();
    assert_eq!(span_count(&pool, rid).await, 0);
    let graphs: i64 = sqlx::query_scalar("SELECT count(*) FROM graphs WHERE id = $1::uuid")
        .bind(gid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(graphs, 0);
    let (status, _) = call(
        app,
        Method::GET,
        &format!("/runs/{rid}/trace/verify"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let audit = get(app, token, &format!("/workspaces/{wid}/audit")).await;
    let purged = audit
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["action"] == "evidence_purged")
        .unwrap_or_else(|| panic!("no evidence_purged entry: {audit}"));
    assert_eq!(purged["actor_name"], "system");
    assert_eq!(purged["subject"], "G");
    let detail = purged["detail"].as_str().unwrap();
    assert!(
        detail.contains(gid.as_str())
            && detail.contains("1 run(s)")
            && detail.contains("after 30 day(s)"),
        "{detail}"
    );
    let chain = get(app, token, &format!("/workspaces/{wid}/audit/verify")).await;
    assert_eq!(
        chain["ok"], true,
        "the purge leaves the chain whole: {chain}"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn erasing_a_workspace_is_a_purge_path_on_the_platform_log(pool: PgPool) {
    let f = finished_run(pool.clone()).await;
    let (app, token, wid) = (&f.app, f.token.as_str(), &f.wid);
    let (status, other) = call(
        app,
        Method::POST,
        "/workspaces",
        Some(token),
        Some(json!({"name": "Second"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{other}");
    let (status, body) = call(
        app,
        Method::DELETE,
        &format!("/workspaces/{wid}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(span_count(&pool, &f.rid).await, 0);
    let detail: String = sqlx::query_scalar(
        "SELECT detail FROM platform_events WHERE action = 'workspace_deleted' AND actor_id = $1::uuid",
    )
    .bind(&f.user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        detail.contains("deleted by an owner")
            && detail.contains("1 run(s)")
            && detail.contains("audit chain ended at entry"),
        "{detail}"
    );
}

/// An empty database next to the test's own, and its URL.
async fn empty_database(pool: &PgPool) -> String {
    let name = format!("nexc_evidence_{}", Uuid::now_v7().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
        .execute(pool)
        .await
        .unwrap();
    let base = std::env::var("DATABASE_URL").unwrap();
    let (server, _) = base.rsplit_once('/').unwrap();
    format!("{server}/{name}")
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_transfer_carries_the_same_chain_and_refuses_a_broken_one(pool: PgPool) {
    let f = finished_run(pool.clone()).await;
    let workspace: Uuid = f.wid.parse().unwrap();
    let source_head = nexc::repo::audit::head(&pool, workspace).await.unwrap();
    assert!(source_head.is_some());

    // Intact: the target's database computes the same chain, to the same head.
    let url = empty_database(&pool).await;
    nexc::engine::transfer::copy_workspace(&pool, &url, workspace, &[])
        .await
        .unwrap();
    let target = nexc::repo::connect(&url, 1).await.unwrap();
    assert_eq!(
        nexc::repo::audit::head(&target, workspace).await.unwrap(),
        source_head
    );
    target.close().await;

    // An entry changed here behind the guard's back cannot arrive as a clean chain there.
    let mut tx = pool.begin().await.unwrap();
    for sql in [
        "ALTER TABLE audit_log DISABLE TRIGGER audit_log_append_only",
        "UPDATE audit_log SET detail = detail || ' (edited)' WHERE seq = 0",
        "ALTER TABLE audit_log ENABLE TRIGGER audit_log_append_only",
    ] {
        sqlx::query(sql).execute(&mut *tx).await.unwrap();
    }
    tx.commit().await.unwrap();
    let url = empty_database(&pool).await;
    let refused = nexc::engine::transfer::copy_workspace(&pool, &url, workspace, &[])
        .await
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains("did not arrive as the same chain"),
        "{refused}"
    );
}

/// The server's pool cuts a request's statement off after its limit, but a
/// purge path lifts the limits for its own transaction (a retention purge or
/// an erasure is one long cascade, which would fail the same way on every
/// retry), and the onto-intelli export lifts them while it holds its claim;
/// the next transaction has them back.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_purge_outlasts_the_pools_statement_limit(pool: PgPool) {
    let options = (*pool.connect_options()).clone();
    let limited =
        nexc::repo::connect_limited_with(options, 2, std::time::Duration::from_millis(500))
            .await
            .unwrap();
    let cut = sqlx::query("SELECT pg_sleep(1)")
        .execute(&limited)
        .await
        .unwrap_err();
    assert_eq!(
        cut.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("57014"),
        "{cut}"
    );

    let mut tx = limited.begin().await.unwrap();
    nexc::repo::evidence::allow_purge(&mut tx).await.unwrap();
    sqlx::query("SELECT pg_sleep(1)")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let mut tx = limited.begin().await.unwrap();
    nexc::repo::onto_export::claim(&mut tx, Uuid::now_v7(), 10)
        .await
        .unwrap();
    let idle: String = sqlx::query_scalar("SHOW idle_in_transaction_session_timeout")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(idle, "0");
    tx.rollback().await.unwrap();

    for (setting, value) in [
        ("statement_timeout", "500ms"),
        ("idle_in_transaction_session_timeout", "5min"),
    ] {
        let now: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SHOW {setting}")))
            .fetch_one(&limited)
            .await
            .unwrap();
        assert_eq!(now, value, "{setting}");
    }
    nexc::repo::evidence::purge_payloads(&limited)
        .await
        .unwrap();
}
