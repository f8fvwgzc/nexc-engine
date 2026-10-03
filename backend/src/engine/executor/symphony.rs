//! texc-symphony bridge (contract §9).
//!
//! Symphony nodes become issues of a `tracker.kind: memory` workflow that
//! nexc fully manages: the backend rewrites `WORKFLOW.md` atomically, asks
//! Symphony to poll (`POST /api/v1/refresh`) and follows
//! `GET /api/v1/runs?issue=…` until the issue's run ends. The issue then
//! moves to `Done` (or `Cancelled`). Symphony owns retries and workspaces,
//! so failures reported by it are not retried by nexc.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Mutex;
use uuid::Uuid;

use super::{ExecContext, ExecError, ExecOutput, NodeExecutor};
use crate::config::Settings;
use crate::domain::graph::GraphNode;
use crate::realtime::events::LogLevel;

const POLL_INTERVAL: Duration = Duration::from_secs(3);
const CLOCK_SKEW: chrono::Duration = chrono::Duration::seconds(5);

/// An issue of the managed memory tracker.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Issue {
    pub id: String,
    pub identifier: String,
    pub title: String,
    pub description: String,
    pub state: String,
    pub labels: Vec<String>,
}

/// Run record from `GET /api/v1/runs`.
#[derive(Debug, Clone, Deserialize)]
struct RunRecord {
    id: i64,
    status: String,
    error: Option<String>,
    started_at: DateTime<Utc>,
    tokens: RunTokens,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RunTokens {
    input: i64,
    output: i64,
}

#[derive(Debug, Deserialize)]
struct RunList {
    runs: Vec<RunRecord>,
}

#[derive(Debug, Deserialize)]
struct RunEvent {
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RunEventList {
    events: Vec<RunEvent>,
}

/// Owns the managed `WORKFLOW.md` and the issues in it.
pub struct SymphonyBridge {
    enabled: bool,
    url: String,
    workflow: PathBuf,
    issues: Mutex<BTreeMap<String, Issue>>,
}

impl SymphonyBridge {
    /// Bridge configured from settings.
    pub fn new(settings: &Settings) -> Self {
        SymphonyBridge {
            enabled: settings.symphony_enabled,
            url: settings.symphony_url.trim_end_matches('/').to_owned(),
            workflow: settings.symphony_workflow.clone(),
            issues: Mutex::new(BTreeMap::new()),
        }
    }

    /// Whether the bridge is enabled.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Base URL of the Symphony API.
    pub fn url(&self) -> &str {
        &self.url
    }

    async fn set_issue(&self, issue: Issue) -> std::io::Result<()> {
        let mut issues = self.issues.lock().await;
        issues.insert(issue.identifier.clone(), issue);
        write_atomic(&self.workflow, &render_workflow(issues.values())).await
    }

    async fn set_state(&self, identifier: &str, state: &str) -> std::io::Result<()> {
        let mut issues = self.issues.lock().await;
        if let Some(issue) = issues.get_mut(identifier) {
            issue.state = state.to_owned();
        }
        write_atomic(&self.workflow, &render_workflow(issues.values())).await
    }
}

/// Issue identifier of a node: `NEXC-` + the last 8 hex digits of its id
/// (UUID v7 ids share their leading timestamp digits, so the tail is used
/// to keep identifiers of nodes created together distinct).
pub fn identifier(node_id: Uuid) -> String {
    let hex = node_id.simple().to_string();
    format!("NEXC-{}", &hex[hex.len() - 8..])
}

/// Renders the managed workflow. JSON front matter is valid YAML 1.2.
pub fn render_workflow<'a>(issues: impl Iterator<Item = &'a Issue>) -> String {
    let front = json!({
        "tracker": { "kind": "memory", "provider": { "issues": issues.collect::<Vec<_>>() } },
        "polling": { "interval_ms": 5000 },
    });
    format!(
        "---\n{}\n---\n\nYou are working on {{{{ issue.identifier }}}}: {{{{ issue.title }}}}\n\n{{{{ issue.description }}}}\n",
        serde_json::to_string_pretty(&front).expect("json serialises")
    )
}

async fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    tokio::fs::create_dir_all(dir).await?;
    let tmp = dir.join(format!(".WORKFLOW.md.{}.tmp", Uuid::now_v7()));
    tokio::fs::write(&tmp, contents).await?;
    tokio::fs::rename(&tmp, path).await
}

fn describe(ctx: &ExecContext) -> String {
    let mut text = format!("{}\n", ctx.node.content.trim());
    if !ctx.goal.trim().is_empty() {
        text.push_str(&format!("\nOverall goal: {}\n", ctx.goal.trim()));
    }
    for u in &ctx.upstream {
        let excerpt: String = u.output.chars().take(4_000).collect();
        text.push_str(&format!("\n## Upstream: {}\n{excerpt}\n", u.title));
    }
    text
}

/// Moves the issue to `Cancelled` if the attempt is dropped before it ends
/// (timeout or run cancellation).
struct IssueGuard {
    bridge: Arc<SymphonyBridge>,
    identifier: String,
    armed: bool,
}

impl Drop for IssueGuard {
    fn drop(&mut self) {
        if self.armed {
            let (bridge, identifier) = (self.bridge.clone(), self.identifier.clone());
            tokio::spawn(async move {
                if let Err(err) = bridge.set_state(&identifier, "Cancelled").await {
                    tracing::warn!(%identifier, error = %err, "cannot cancel symphony issue");
                }
            });
        }
    }
}

/// Executes nodes with `executor: "symphony"`.
pub struct SymphonyExecutor;

impl NodeExecutor for SymphonyExecutor {
    fn execute<'a>(&'a self, ctx: &'a ExecContext) -> BoxFuture<'a, Result<ExecOutput, ExecError>> {
        Box::pin(run(ctx))
    }
}

async fn run(ctx: &ExecContext) -> Result<ExecOutput, ExecError> {
    let bridge = ctx.state.symphony.clone();
    if !bridge.enabled() {
        return Err(ExecError::fatal(
            "the texc-symphony executor is disabled; set NEXC_SYMPHONY_ENABLED=true or change the node's executor",
        ));
    }
    let node: &GraphNode = &ctx.node;
    let identifier = identifier(node.id);
    let issue = Issue {
        id: node.id.to_string(),
        identifier: identifier.clone(),
        title: node.title.clone(),
        description: describe(ctx),
        state: "Todo".into(),
        labels: vec!["nexc".into(), node.kind.to_string()],
    };
    let dispatched_at = Utc::now() - CLOCK_SKEW;
    bridge
        .set_issue(issue)
        .await
        .map_err(|e| ExecError::fatal(format!("cannot write WORKFLOW.md: {e}")))?;
    let mut guard = IssueGuard {
        bridge: bridge.clone(),
        identifier: identifier.clone(),
        armed: true,
    };
    let http = &ctx.state.http;
    refresh(http, bridge.url()).await?;
    ctx.log(
        LogLevel::Info,
        format!("queued as symphony issue {identifier}"),
    );

    let mut announced = false;
    let record = loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        let Some(run) = latest_run(http, bridge.url(), &identifier, dispatched_at).await? else {
            continue;
        };
        if run.status == "running" {
            if !announced {
                ctx.log(LogLevel::Info, format!("symphony run {} started", run.id));
                announced = true;
            }
            continue;
        }
        break run;
    };
    let succeeded = record.status == "succeeded";
    guard.armed = false;
    let final_state = if succeeded { "Done" } else { "Cancelled" };
    bridge
        .set_state(&identifier, final_state)
        .await
        .map_err(|e| ExecError::fatal(format!("cannot write WORKFLOW.md: {e}")))?;
    refresh(http, bridge.url()).await?;
    ctx.tokens(record.tokens.input, record.tokens.output);
    if !succeeded {
        let reason = record.error.unwrap_or_else(|| record.status.clone());
        return Err(ExecError::fatal(format!(
            "symphony run {} {}: {reason}",
            record.id, record.status
        )));
    }
    let output = final_message(http, bridge.url(), record.id)
        .await
        .unwrap_or_else(|| "symphony run succeeded".into());
    ctx.output(&output);
    Ok(ExecOutput {
        output,
        tokens_in: record.tokens.input,
        tokens_out: record.tokens.output,
    })
}

async fn refresh(http: &reqwest::Client, base: &str) -> Result<(), ExecError> {
    let resp = http
        .post(format!("{base}/api/v1/refresh"))
        .json(&json!({}))
        .send()
        .await
        .map_err(|e| ExecError::transient(format!("symphony unreachable: {}", e.without_url())))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(ExecError::transient(format!(
            "symphony refresh failed: HTTP {}",
            resp.status()
        )))
    }
}

async fn latest_run(
    http: &reqwest::Client,
    base: &str,
    identifier: &str,
    since: DateTime<Utc>,
) -> Result<Option<RunRecord>, ExecError> {
    let resp = http
        .get(format!("{base}/api/v1/runs"))
        .query(&[("issue", identifier), ("limit", "5")])
        .send()
        .await
        .map_err(|e| ExecError::transient(format!("symphony unreachable: {}", e.without_url())))?;
    if !resp.status().is_success() {
        return Err(ExecError::transient(format!(
            "symphony runs API returned HTTP {}",
            resp.status()
        )));
    }
    let list: RunList = resp
        .json()
        .await
        .map_err(|e| ExecError::fatal(format!("invalid symphony response: {e}")))?;
    Ok(list
        .runs
        .into_iter()
        .filter(|r| r.started_at >= since)
        .max_by_key(|r| r.id))
}

async fn final_message(http: &reqwest::Client, base: &str, run_id: i64) -> Option<String> {
    let resp = http
        .get(format!("{base}/api/v1/runs/{run_id}/events"))
        .query(&[("limit", "1000")])
        .send()
        .await
        .ok()?;
    let list: RunEventList = resp.json().await.ok()?;
    list.events
        .into_iter()
        .rev()
        .find_map(|e| e.message.filter(|m| !m.trim().is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_differ_for_nodes_created_together() {
        let (a, b) = (Uuid::now_v7(), Uuid::now_v7());
        assert_ne!(identifier(a), identifier(b));
        assert_eq!(identifier(a).len(), "NEXC-".len() + 8);
    }

    #[test]
    fn renders_memory_tracker_workflow() {
        let issue = Issue {
            id: "1".into(),
            identifier: "NEXC-0000abcd".into(),
            title: "Add \"health\" check".into(),
            description: "line 1\nline 2".into(),
            state: "Todo".into(),
            labels: vec!["nexc".into()],
        };
        let text = render_workflow([issue].iter());
        assert!(text.starts_with("---\n{"));
        let front = text.split("---\n").nth(1).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(front.trim()).unwrap();
        assert_eq!(parsed["tracker"]["kind"], "memory");
        assert_eq!(parsed["tracker"]["provider"]["issues"][0]["state"], "Todo");
        assert!(text.contains("{{ issue.identifier }}"));
    }

    #[test]
    fn parses_run_records() {
        let body = r#"{"runs":[{"id":42,"issue_id":"x","issue_identifier":"NEXC-1","issue_title":null,"attempt":0,
            "worker_host":null,"workspace_path":null,"status":"succeeded","error":null,"turns":7,
            "started_at":"2026-02-24T20:10:12.004Z","finished_at":"2026-02-24T20:31:40.250Z","duration_ms":1,
            "tokens":{"input":18230,"output":2207,"total":20437}}],"next_before_id":null}"#;
        let list: RunList = serde_json::from_str(body).unwrap();
        assert_eq!((list.runs[0].id, list.runs[0].tokens.output), (42, 2207));
    }
}
