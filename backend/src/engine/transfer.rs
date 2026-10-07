//! Transfers a workspace's data to a PostgreSQL its owner names, so that an
//! organisation can hold its own data (and then have it removed from here).
//!
//! The target is prepared with this build's migrations and the workspace's
//! rows are copied table by table, parents before children, inside one
//! transaction on the target: a transfer is there completely or not at all.
//! Nothing is changed on this server; removing the workspace afterwards is a
//! separate, deliberate act.
//!
//! What is deliberately not copied:
//! * password hashes — the accounts the data refers to are created on the
//!   target with a hash nothing matches, so nobody's credentials leave;
//! * encrypted API keys — they can only be read with this server's master key;
//! * sessions, realtime tickets and other members' personal LLM settings;
//! * files on disk (uploaded documents, run artifacts); their passages and
//!   records are copied, the files are not.

use std::net::IpAddr;

use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{AssertSqlSafe, PgPool};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::app::AppState;
use crate::repo;

/// Rows copied per statement for the tables that can be paged by `id`.
const BATCH: i64 = 1_000;

/// One table of the transfer: which of its rows belong to workspace `$1`.
struct Table {
    name: &'static str,
    /// A condition on the table aliased `t`.
    belongs: &'static str,
    /// How its rows are read in batches.
    page: Page,
    /// Fields overwritten in every copied row (as a JSON object).
    overlay: Option<&'static str>,
}

/// How a table's rows are read.
#[derive(Clone, Copy)]
enum Page {
    /// All at once: small tables, and tables whose rows refer to each other.
    Whole,
    /// By its uuid `id`, each batch starting after the last one.
    ById,
    /// By position in this order, for a large table without an `id`.
    ByOrder(&'static str),
}

const fn table(name: &'static str, belongs: &'static str, paged: bool) -> Table {
    Table {
        name,
        belongs,
        page: if paged { Page::ById } else { Page::Whole },
        overlay: None,
    }
}

const IN_WORKSPACE: &str = "t.workspace_id = $1";
const OF_ITS_TEAMS: &str = "t.team_id IN (SELECT id FROM teams WHERE workspace_id = $1)";
const OF_ITS_GRAPHS: &str = "t.graph_id IN (SELECT id FROM graphs WHERE workspace_id = $1)";
const OF_ITS_RUNS: &str = "t.run_id IN (SELECT r.id FROM runs r JOIN graphs g ON g.id = r.graph_id
                                        WHERE g.workspace_id = $1)";
const OF_ITS_ISSUES: &str = "t.issue_id IN (SELECT id FROM issues WHERE workspace_id = $1)";
const OF_ITS_CONVERSATIONS: &str =
    "t.conversation_id IN (SELECT id FROM assistant_conversations WHERE workspace_id = $1)";

/// Every account the workspace's rows refer to, members or not: a row may
/// name someone who has since left.
const REFERENCED_USERS: &str = "t.id IN (
    SELECT user_id FROM workspace_members WHERE workspace_id = $1
    UNION SELECT created_by FROM workspaces WHERE id = $1
    UNION SELECT invited_by FROM workspace_invites WHERE workspace_id = $1
    UNION SELECT owner_id FROM graphs WHERE workspace_id = $1
    UNION SELECT owner_id FROM agents WHERE workspace_id = $1
    UNION SELECT owner_id FROM memories WHERE workspace_id = $1
    UNION SELECT uploaded_by FROM documents WHERE workspace_id = $1
    UNION SELECT actor_id FROM audit_log WHERE workspace_id = $1
    UNION SELECT user_id FROM llm_usage WHERE workspace_id = $1
    UNION SELECT user_id FROM notifications WHERE workspace_id = $1
    UNION SELECT actor_id FROM notifications WHERE workspace_id = $1
    UNION SELECT created_by FROM projects WHERE workspace_id = $1
    UNION SELECT lead_id FROM projects WHERE workspace_id = $1
    UNION SELECT assignee_id FROM issues WHERE workspace_id = $1
    UNION SELECT creator_id FROM issues WHERE workspace_id = $1
    UNION SELECT e.actor_id FROM issue_events e JOIN issues i ON i.id = e.issue_id
          WHERE i.workspace_id = $1
    UNION SELECT tm.user_id FROM team_members tm JOIN teams tt ON tt.id = tm.team_id
          WHERE tt.workspace_id = $1
    UNION SELECT r.owner_id FROM runs r JOIN graphs g ON g.id = r.graph_id
          WHERE g.workspace_id = $1
    UNION SELECT p.owner_id FROM plans p JOIN graphs g ON g.id = p.graph_id
          WHERE g.workspace_id = $1
    UNION SELECT updated_by FROM workspace_guardrails WHERE workspace_id = $1
    UNION SELECT created_by FROM day_summaries WHERE workspace_id = $1
    UNION SELECT updated_by FROM workspace_knowledge_settings WHERE workspace_id = $1
    UNION SELECT user_id FROM assistant_conversations WHERE workspace_id = $1)";

/// Names of the tables a transfer copies, in the order it copies them.
pub fn tables() -> impl Iterator<Item = &'static str> {
    TABLES.iter().map(|t| t.name)
}

/// The tables of a workspace, parents before the tables that refer to them.
/// A new table that hangs off a workspace belongs here, or among the
/// exceptions `tests/transfer.rs` lists with their reasons.
const TABLES: &[Table] = &[
    Table {
        name: "users",
        belongs: REFERENCED_USERS,
        page: Page::ById,
        // A hash nothing matches: the account exists for the data to point at. Nobody
        // administers, or is suspended on, another installation because they were here.
        overlay: Some(
            r#"{"password_hash": "!", "failed_logins": 0, "locked_until": null, "role": "user",
                "suspended_at": null, "suspended_reason": "", "session_epoch": 0,
                "session_epoch_at": null, "totp_secret_enc": null, "totp_enabled_at": null,
                "totp_last_step": null}"#,
        ),
    },
    table("workspaces", "t.id = $1", false),
    table("workspace_members", IN_WORKSPACE, false),
    table("workspace_invites", IN_WORKSPACE, false),
    table("workspace_guardrails", IN_WORKSPACE, false),
    Table {
        name: "workspace_knowledge_settings",
        belongs: IN_WORKSPACE,
        page: Page::Whole,
        // The key is sealed with this server's master key; it is entered again on the target.
        overlay: Some(r#"{"api_key_enc": null, "key_hint": null}"#),
    },
    table("teams", IN_WORKSPACE, false),
    table("team_members", OF_ITS_TEAMS, false),
    table("issue_states", OF_ITS_TEAMS, false),
    table("cycles", OF_ITS_TEAMS, false),
    table("labels", IN_WORKSPACE, false),
    table("projects", IN_WORKSPACE, false),
    table("agents", IN_WORKSPACE, false),
    table("graphs", IN_WORKSPACE, false),
    table("nodes", OF_ITS_GRAPHS, true),
    table("edges", OF_ITS_GRAPHS, true),
    table("plans", OF_ITS_GRAPHS, true),
    table("runs", OF_ITS_GRAPHS, true),
    Table {
        name: "node_runs",
        belongs: OF_ITS_RUNS,
        page: Page::ByOrder("t.run_id, t.node_id"),
        overlay: None,
    },
    table("artifacts", OF_ITS_RUNS, true),
    // One statement for all issues: a sub-issue and its parent arrive together.
    table("issues", IN_WORKSPACE, false),
    table("issue_labels", OF_ITS_ISSUES, false),
    table("issue_events", OF_ITS_ISSUES, true),
    table("notifications", IN_WORKSPACE, true),
    table("knowledge_topics", IN_WORKSPACE, false),
    table("documents", IN_WORKSPACE, true),
    table("document_chunks", IN_WORKSPACE, true),
    table("memory_topics", IN_WORKSPACE, false),
    table("memories", IN_WORKSPACE, true),
    table("audit_log", IN_WORKSPACE, true),
    table("day_summaries", IN_WORKSPACE, false),
    table("llm_usage", IN_WORKSPACE, true),
    table("assistant_conversations", IN_WORKSPACE, true),
    table("assistant_messages", OF_ITS_CONVERSATIONS, true),
];

/// What a transfer copied for one table.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TableReport {
    pub table: String,
    /// Rows of the workspace on this server.
    pub read: i64,
    /// Rows written to the target (fewer when a row was already there).
    pub written: i64,
}

/// A connection URL without its credentials or options.
pub fn location(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let rest = rest.rsplit_once('@').map_or(rest, |(_, host)| host);
    let rest = rest.split('?').next().unwrap_or(rest);
    format!("{scheme}://{rest}")
}

fn is_internal(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
        }
        IpAddr::V6(v6) => {
            v6.is_loopback() || v6.is_unspecified() || (v6.segments()[0] & 0xfe00) == 0xfc00
        }
    }
}

/// In production a workspace owner must not be able to make the server
/// connect to its own network: targets that resolve to loopback, private or
/// link-local addresses are refused.
pub async fn check_target(url: &str, production: bool) -> Result<(), String> {
    if !(url.starts_with("postgres://") || url.starts_with("postgresql://")) {
        return Err("the target must be a postgres:// URL".into());
    }
    if !production {
        return Ok(());
    }
    let place = location(url);
    let host_port = place
        .split_once("://")
        .map_or("", |(_, rest)| rest.split('/').next().unwrap_or(""));
    let lookup = if host_port.contains(':') {
        host_port.to_owned()
    } else {
        format!("{host_port}:5432")
    };
    let addresses: Vec<_> = tokio::net::lookup_host(&lookup)
        .await
        .map_err(|_| "the target's host name does not resolve".to_owned())?
        .collect();
    if addresses.is_empty() || addresses.iter().any(|a| is_internal(a.ip())) {
        return Err("the target must be a public address".into());
    }
    Ok(())
}

/// The columns of `table` on the target that a row can be written to:
/// generated columns compute themselves, and the pgvector form of an
/// embedding is rebuilt there from the stored bytes.
async fn writable_columns(target: &PgPool, table: &str) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT column_name::text FROM information_schema.columns
         WHERE table_schema = current_schema() AND table_name = $1
           AND is_generated = 'NEVER' AND column_name <> 'embedding_vec'
         ORDER BY ordinal_position",
    )
    .bind(table)
    .fetch_all(target)
    .await
}

/// Copies one table's rows of the workspace. Table and column names come
/// from the fixed list above and from the target's own catalogue.
async fn copy_table(
    source: &PgPool,
    target: &mut sqlx::PgConnection,
    columns: &[String],
    t: &Table,
    workspace: Uuid,
) -> anyhow::Result<TableReport> {
    let overlay = t.overlay.unwrap_or("{}");
    let quoted: Vec<String> = columns.iter().map(|c| format!("\"{c}\"")).collect();
    let insert = format!(
        "INSERT INTO {name} ({cols}) SELECT {cols} FROM jsonb_populate_recordset(NULL::{name}, $1)
         ON CONFLICT DO NOTHING",
        name = t.name,
        cols = quoted.join(", ")
    );
    // $2 is the last id of the previous batch, $3 the batch size, $4 the rows already read.
    let (id, page) = match t.page {
        Page::Whole => (
            "NULL::uuid",
            " AND $2::uuid IS NULL AND $3::bigint IS NOT NULL AND $4::bigint IS NOT NULL"
                .to_owned(),
        ),
        Page::ById => (
            "t.id",
            " AND ($2::uuid IS NULL OR t.id > $2) AND $4::bigint IS NOT NULL
              ORDER BY t.id LIMIT $3"
                .to_owned(),
        ),
        Page::ByOrder(order) => (
            "NULL::uuid",
            format!(" AND $2::uuid IS NULL ORDER BY {order} LIMIT $3 OFFSET $4"),
        ),
    };
    let select = format!(
        "SELECT COALESCE(jsonb_agg(row), '[]'::jsonb), count(*), (max(id::text))::uuid FROM (
            SELECT to_jsonb(t) || '{overlay}'::jsonb AS row, {id} AS id
            FROM {name} t WHERE {belongs}{page}) rows",
        name = t.name,
        belongs = t.belongs,
    );
    let mut report = TableReport {
        table: t.name.to_owned(),
        read: 0,
        written: 0,
    };
    let mut after: Option<Uuid> = None;
    loop {
        let (rows, read, last): (Value, i64, Option<Uuid>) =
            sqlx::query_as(AssertSqlSafe(select.clone()))
                .bind(workspace)
                .bind(after)
                .bind(BATCH)
                .bind(report.read)
                .fetch_one(source)
                .await?;
        if read == 0 {
            break;
        }
        let written = sqlx::query(AssertSqlSafe(insert.clone()))
            .bind(&rows)
            .execute(&mut *target)
            .await?
            .rows_affected();
        report.read += read;
        report.written += i64::try_from(written).unwrap_or(i64::MAX);
        after = last;
        if matches!(t.page, Page::Whole) || read < BATCH {
            break;
        }
    }
    Ok(report)
}

/// Copies a workspace to `url`: prepares the schema there, then writes every
/// table in one transaction. Returns what was copied per table.
/// Connects to the target, and says what it is: used by the owner's
/// connection check. Refuses a target that already holds the workspace.
pub async fn probe_target(url: &str, workspace: Uuid) -> anyhow::Result<String> {
    let target = repo::connect(url, 1)
        .await
        .map_err(|err| anyhow::anyhow!("cannot connect: {err}"))?;
    let version: String = sqlx::query_scalar("SELECT version()")
        .fetch_one(&target)
        .await?;
    let has_tables: bool =
        sqlx::query_scalar("SELECT to_regclass('public.workspaces') IS NOT NULL")
            .fetch_one(&target)
            .await?;
    if has_tables {
        let already: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM workspaces WHERE id = $1)")
                .bind(workspace)
                .fetch_one(&target)
                .await?;
        anyhow::ensure!(!already, "that database already holds this workspace");
    }
    target.close().await;
    let short = version.split(" on ").next().unwrap_or(&version).to_owned();
    Ok(if has_tables {
        format!("{short}; already a Nexc database, the workspace will be added")
    } else {
        format!("{short}; empty, the tables will be created")
    })
}

pub async fn copy_workspace(
    source: &PgPool,
    url: &str,
    workspace: Uuid,
) -> anyhow::Result<Vec<TableReport>> {
    let target = repo::connect(url, 2)
        .await
        .map_err(|err| anyhow::anyhow!("cannot connect to the target database: {err}"))?;
    repo::migrate(&target).await.map_err(|err| {
        anyhow::anyhow!(
            "cannot prepare the target database (it needs the right to create tables): {err}"
        )
    })?;
    let already: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM workspaces WHERE id = $1)")
            .bind(workspace)
            .fetch_one(&target)
            .await?;
    anyhow::ensure!(
        !already,
        "the target already holds this workspace; remove it there before transferring again"
    );
    let mut columns = Vec::with_capacity(TABLES.len());
    for t in TABLES {
        columns.push(writable_columns(&target, t.name).await?);
    }
    let mut tx = target.begin().await?;
    let mut reports = Vec::with_capacity(TABLES.len());
    for (t, columns) in TABLES.iter().zip(&columns) {
        let report = copy_table(source, &mut tx, columns, t, workspace)
            .await
            .map_err(|err| anyhow::anyhow!("copying {} failed: {err}", t.name))?;
        reports.push(report);
    }
    tx.commit().await?;
    target.close().await;
    Ok(reports)
}

/// Runs a transfer that was recorded as `transfer_id` and records how it ended.
/// Copies the workspace's files on disk into the target's `workspace_files`,
/// one file per statement so a large workspace never sits in memory at once.
/// A file that is gone from disk is left out: its record was copied anyway.
pub async fn copy_files(
    state: &AppState,
    target: &PgPool,
    workspace: Uuid,
) -> anyhow::Result<TableReport> {
    let documents: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM documents WHERE workspace_id = $1 ORDER BY id")
            .bind(workspace)
            .fetch_all(&state.db)
            .await?;
    let artifacts: Vec<String> = sqlx::query_scalar(
        "SELECT a.storage_path FROM artifacts a
         JOIN runs r ON r.id = a.run_id JOIN graphs g ON g.id = r.graph_id
         WHERE g.workspace_id = $1 ORDER BY a.storage_path",
    )
    .bind(workspace)
    .fetch_all(&state.db)
    .await?;
    let mut entries: Vec<(String, std::path::PathBuf)> = documents
        .iter()
        .map(|id| {
            (
                format!("documents/{id}"),
                state.settings.documents_dir().join(id.to_string()),
            )
        })
        .collect();
    for storage_path in &artifacts {
        if let Ok(on_disk) = super::artifacts::resolve(state, storage_path) {
            entries.push((format!("artifacts/{storage_path}"), on_disk));
        }
    }
    let read = i64::try_from(entries.len()).unwrap_or(i64::MAX);
    let mut written = 0i64;
    for (path, on_disk) in entries {
        let Ok(content) = tokio::fs::read(&on_disk).await else {
            continue;
        };
        sqlx::query(
            "INSERT INTO workspace_files (id, workspace_id, path, content) VALUES ($1, $2, $3, $4)
             ON CONFLICT (workspace_id, path) DO NOTHING",
        )
        .bind(Uuid::now_v7())
        .bind(workspace)
        .bind(&path)
        .bind(content)
        .execute(target)
        .await
        .map_err(|err| anyhow::anyhow!("copying file {path} failed: {err}"))?;
        written += 1;
    }
    Ok(TableReport {
        table: "files".into(),
        read,
        written,
    })
}

/// Tells the Redis at `url` that the workspace now lives there: a small key
/// the owner's own systems can look for. The server keeps nothing durable
/// in Redis, so there is nothing more to move; the check is what matters.
pub async fn hand_over_redis(url: &str, workspace: Uuid, target: &str) -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    crate::http::handlers::infrastructure::check_redis(url).await?;
    let rest = url.trim_start_matches("redis://");
    let (credentials, address) = match rest.rsplit_once('@') {
        Some((credentials, address)) => (Some(credentials), address),
        None => (None, rest),
    };
    let address = address.split('/').next().unwrap_or(address);
    let address = if address.contains(':') {
        address.to_owned()
    } else {
        format!("{address}:6379")
    };
    let mut stream = tokio::net::TcpStream::connect(&address).await?;
    let mut reply = [0u8; 256];
    if let Some(password) = credentials.map(|c| c.rsplit_once(':').map_or(c, |(_, p)| p)) {
        let auth = format!("*2\r\n$4\r\nAUTH\r\n${}\r\n{password}\r\n", password.len());
        stream.write_all(auth.as_bytes()).await?;
        // Only +OK or an error comes back; the check above already proved the password.
        let _ = stream.read(&mut reply).await?;
    }
    let key = format!("nexc:workspace:{workspace}");
    let value =
        json!({"workspace_id": workspace, "database": target, "moved_at": chrono::Utc::now()})
            .to_string();
    let set = format!(
        "*3\r\n$3\r\nSET\r\n${}\r\n{key}\r\n${}\r\n{value}\r\n",
        key.len(),
        value.len()
    );
    stream.write_all(set.as_bytes()).await?;
    let read = stream.read(&mut reply).await?;
    anyhow::ensure!(reply[..read].starts_with(b"+OK"), "Redis refused the write");
    Ok(())
}

/// On a server that received workspaces: writes every file that travelled
/// in `workspace_files` to the data folder, where the server expects it,
/// then marks it restored. Called at start; safe to call again.
pub async fn restore_files(state: &AppState) -> anyhow::Result<usize> {
    let mut restored = 0;
    loop {
        let batch: Vec<(Uuid, String, Vec<u8>)> = sqlx::query_as(
            "SELECT id, path, content FROM workspace_files WHERE restored_at IS NULL
             ORDER BY id LIMIT 50",
        )
        .fetch_all(&state.db)
        .await?;
        if batch.is_empty() {
            return Ok(restored);
        }
        for (id, path, content) in batch {
            // Only the two layouts a transfer writes; anything else is left alone.
            let on_disk = match path.split_once('/') {
                Some(("documents", name)) if !name.contains('/') && !name.contains("..") => {
                    state.settings.documents_dir().join(name)
                }
                Some(("artifacts", rest)) if !rest.contains("..") => {
                    state.settings.artifacts_dir().join(rest)
                }
                _ => continue,
            };
            if let Some(parent) = on_disk.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&on_disk, &content).await?;
            sqlx::query(
                "UPDATE workspace_files SET restored_at = now(), content = '' WHERE id = $1",
            )
            .bind(id)
            .execute(&state.db)
            .await?;
            restored += 1;
        }
    }
}

/// Runs a transfer to completion and records how it ended: the rows, then
/// the files, then the Redis handover when one was named.
pub async fn run(
    state: &AppState,
    transfer_id: Uuid,
    workspace: Uuid,
    url: String,
    redis_url: Option<String>,
) {
    let outcome = async {
        let mut report = copy_workspace(&state.db, &url, workspace).await?;
        let target = repo::connect(&url, 1)
            .await
            .map_err(|err| anyhow::anyhow!("cannot connect to the target database: {err}"))?;
        report.push(copy_files(state, &target, workspace).await?);
        target.close().await;
        if let Some(redis) = redis_url.as_deref() {
            hand_over_redis(redis, workspace, &location(&url))
                .await
                .map_err(|err| anyhow::anyhow!("the rows and files are there, but Redis: {err}"))?;
        }
        Ok::<_, anyhow::Error>(report)
    }
    .await;
    let (status, report, error) = match outcome {
        Ok(reports) => ("done", json!(reports), String::new()),
        Err(err) => {
            tracing::warn!(%workspace, error = %err, "workspace transfer failed");
            // The driver's messages name hosts and reasons, not passwords.
            (
                "failed",
                json!([]),
                err.to_string().chars().take(500).collect(),
            )
        }
    };
    let saved = sqlx::query(
        "UPDATE workspace_transfers SET status = $2, report = $3, error = $4, finished_at = now()
         WHERE id = $1",
    )
    .bind(transfer_id)
    .bind(status)
    .bind(report)
    .bind(error)
    .execute(&state.db)
    .await;
    if let Err(err) = saved {
        tracing::error!(%workspace, error = %err, "cannot record how the transfer ended");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_target_is_shown_without_its_credentials() {
        assert_eq!(
            location("postgres://u:p@ss@db.example.com:5432/acme?sslmode=require"),
            "postgres://db.example.com:5432/acme"
        );
    }

    #[tokio::test]
    async fn production_refuses_targets_inside_the_servers_network() {
        for url in [
            "postgres://u:p@127.0.0.1:5432/db",
            "postgres://u:p@localhost/db",
            "postgres://u:p@10.0.0.8/db",
            "postgres://u:p@192.168.1.4:5432/db",
            "postgres://u:p@169.254.169.254/db",
        ] {
            assert!(check_target(url, true).await.is_err(), "{url}");
            assert!(
                check_target(url, false).await.is_ok(),
                "{url} is fine in development"
            );
        }
        assert!(
            check_target("mysql://u:p@db.example.com/db", false)
                .await
                .is_err()
        );
    }

    #[test]
    fn every_table_is_listed_once_and_after_what_it_refers_to() {
        let position = |name: &str| TABLES.iter().position(|t| t.name == name).unwrap();
        let mut names: Vec<&str> = TABLES.iter().map(|t| t.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), TABLES.len());
        for (child, parent) in [
            ("workspaces", "users"),
            ("teams", "workspaces"),
            ("issues", "issue_states"),
            ("issues", "cycles"),
            ("issues", "projects"),
            ("issues", "graphs"),
            ("issues", "agents"),
            ("edges", "nodes"),
            ("node_runs", "runs"),
            ("document_chunks", "documents"),
            ("document_chunks", "knowledge_topics"),
            ("memories", "memory_topics"),
            ("memories", "graphs"),
            ("llm_usage", "runs"),
            ("notifications", "issues"),
        ] {
            assert!(position(child) > position(parent), "{child} after {parent}");
        }
    }
}
