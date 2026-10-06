//! texc-symphony bridge (contract §9).
//!
//! Symphony nodes become issues of a `tracker.kind: memory` workflow that
//! nexc fully manages: the backend rewrites `WORKFLOW.md` atomically, asks
//! Symphony to poll (`POST /api/v1/refresh`) and follows
//! `GET /api/v1/runs?issue=…` until the issue's run ends. The issue then
//! moves to `Done` (or `Cancelled`).
//!
//! Every graph has one git repository under `<workflow dir>/repos`. An
//! issue's workspace is a clone of it, and when the run succeeds the bridge
//! commits the workspace, rebases it on the repository's `main` and pushes,
//! so nodes that run later build on the code of the nodes before them. A
//! change that cannot be rebased is a retryable failure: the next attempt
//! starts from the newer `main`.
//!
//! The bridge and Symphony must see the workflow directory at the same path.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
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

const POLL_INTERVAL: Duration = Duration::from_millis(500);
const CLOCK_SKEW: chrono::Duration = chrono::Duration::seconds(5);
/// How long Symphony waits after a run before it runs a still-open issue
/// again. The bridge closes the issue well inside this window.
const CONTINUATION_DELAY_MS: u64 = 15_000;
const PUSH_ATTEMPTS: u32 = 8;
const GITIGNORE: &str = "node_modules/\ntarget/\ndist/\n__pycache__/\n.venv/\n.DS_Store\n";

/// Clones the graph repository into a new issue workspace (`sh`, cwd = the
/// workspace, whose directory name is the issue identifier).
const AFTER_CREATE_HOOK: &str = r#"set -e
id="${PWD##*/}"
git clone -q "$(cat __DIR__/issues/"$id")" .
"#;

const PROMPT: &str = "You are working on {{ issue.identifier }}: {{ issue.title }}

{{ issue.description }}

The current directory is a clone of the project's shared repository. Other tasks of the same
project have already added files to it and more will follow, so build on what is there and keep
to the files this task needs. Do not use git: your changes are committed and merged for you when
you finish. End with a short summary of what you changed.
";

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
    #[serde(default)]
    payload: serde_json::Value,
}

/// What became of a workspace's changes in the graph repository.
#[derive(Debug, PartialEq, Eq)]
pub enum Integration {
    /// The agent changed nothing.
    Unchanged,
    /// The changes are on `main`.
    Merged { commit: String, files: Vec<String> },
    /// The changes do not apply on top of the current `main`.
    Conflict,
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
    layout: Layout,
    issues: Mutex<BTreeMap<String, Issue>>,
    repos: Mutex<()>,
}

/// Where the bridge keeps its files and how the workflow runs agents.
#[derive(Debug, Clone)]
pub struct Layout {
    /// Absolute directory of the managed workflow.
    pub dir: PathBuf,
    /// Agent command for Symphony (`codex.command`); Symphony's default when absent.
    pub agent_command: Option<String>,
    /// Symphony's concurrency limit.
    pub max_agents: usize,
}

impl Layout {
    /// Git repository of a graph.
    pub fn repo(&self, graph_id: Uuid) -> PathBuf {
        self.dir.join("repos").join(format!("{graph_id}.git"))
    }

    /// Workspace of an issue (Symphony names it after the identifier).
    pub fn workspace(&self, identifier: &str) -> PathBuf {
        self.dir.join("workspaces").join(identifier)
    }

    fn issue_file(&self, identifier: &str) -> PathBuf {
        self.dir.join("issues").join(identifier)
    }
}

impl SymphonyBridge {
    /// Bridge configured from settings.
    pub fn new(settings: &Settings) -> Self {
        let workflow = std::path::absolute(&settings.symphony_workflow)
            .unwrap_or_else(|_| settings.symphony_workflow.clone());
        let dir = workflow
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        SymphonyBridge {
            enabled: settings.symphony_enabled,
            url: settings.symphony_url.trim_end_matches('/').to_owned(),
            workflow,
            layout: Layout {
                dir,
                agent_command: settings.symphony_agent_command.clone(),
                max_agents: settings.max_concurrency,
            },
            issues: Mutex::new(BTreeMap::new()),
            repos: Mutex::new(()),
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

    /// Files and agent settings of the bridge.
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Queues an issue whose workspace will be a clone of `repo`.
    async fn set_issue(&self, issue: Issue, repo: &Path) -> std::io::Result<()> {
        let pointer = self.layout.issue_file(&issue.identifier);
        if let Some(dir) = pointer.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        tokio::fs::write(&pointer, repo.to_string_lossy().as_bytes()).await?;
        let mut issues = self.issues.lock().await;
        issues.insert(issue.identifier.clone(), issue);
        write_atomic(
            &self.workflow,
            &render_workflow(issues.values(), &self.layout),
        )
        .await
    }

    /// The graph's repository, created with an initial commit on first use.
    async fn ensure_repo(&self, graph_id: Uuid) -> std::io::Result<PathBuf> {
        let _guard = self.repos.lock().await;
        let repo = self.layout.repo(graph_id);
        if !tokio::fs::try_exists(&repo).await? {
            create_repo(&repo).await?;
        }
        Ok(repo)
    }

    async fn set_state(&self, identifier: &str, state: &str) -> std::io::Result<()> {
        let mut issues = self.issues.lock().await;
        if let Some(issue) = issues.get_mut(identifier) {
            issue.state = state.to_owned();
        }
        write_atomic(
            &self.workflow,
            &render_workflow(issues.values(), &self.layout),
        )
        .await
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
///
/// Each issue runs for one turn; the bridge then merges its work and closes
/// it, which is why Symphony is told to wait before running it again.
pub fn render_workflow<'a>(issues: impl Iterator<Item = &'a Issue>, layout: &Layout) -> String {
    let dir = sh_quote(&layout.dir.to_string_lossy());
    let mut front = json!({
        "tracker": { "kind": "memory", "provider": { "issues": issues.collect::<Vec<_>>() } },
        "polling": { "interval_ms": 5000 },
        "workspace": { "root": layout.dir.join("workspaces") },
        "agent": {
            "max_turns": 1,
            "max_concurrent_agents": layout.max_agents,
            "continuation_delay_ms": CONTINUATION_DELAY_MS,
        },
        "hooks": { "after_create": AFTER_CREATE_HOOK.replace("__DIR__", &dir) },
    });
    if let Some(command) = &layout.agent_command {
        front["codex"] = json!({ "command": command, "approval_policy": "never" });
    }
    format!(
        "---\n{}\n---\n\n{PROMPT}",
        serde_json::to_string_pretty(&front).expect("json serialises")
    )
}

/// Single-quotes `value` for `sh`.
fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Output of a git command that exited successfully, or `None`.
async fn git(dir: &Path, args: &[&str], stdin: Option<&str>) -> std::io::Result<Option<String>> {
    use tokio::io::AsyncWriteExt;

    let mut child = tokio::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        // The workspace was written by an agent: never run hooks it may have left behind.
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .env("GIT_AUTHOR_NAME", "nexc symphony")
        .env("GIT_AUTHOR_EMAIL", "symphony@nexc.invalid")
        .env("GIT_COMMITTER_NAME", "nexc symphony")
        .env("GIT_COMMITTER_EMAIL", "symphony@nexc.invalid")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(text.as_bytes()).await?;
    }
    let output = child.wait_with_output().await?;
    Ok(output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned()))
}

/// Like [`git`], but a failing command is an error.
async fn git_ok(dir: &Path, args: &[&str], stdin: Option<&str>) -> std::io::Result<String> {
    git(dir, args, stdin)
        .await?
        .ok_or_else(|| std::io::Error::other(format!("git {} failed", args.join(" "))))
}

/// Creates a bare repository whose `main` holds one commit with a `.gitignore`.
async fn create_repo(repo: &Path) -> std::io::Result<()> {
    let parent = repo.parent().unwrap_or(Path::new("."));
    tokio::fs::create_dir_all(parent).await?;
    let tmp = parent.join(format!(".{}.tmp", Uuid::now_v7()));
    tokio::fs::create_dir_all(&tmp).await?;
    git_ok(
        &tmp,
        &["init", "-q", "--bare", "--initial-branch=main"],
        None,
    )
    .await?;
    let blob = git_ok(&tmp, &["hash-object", "-w", "--stdin"], Some(GITIGNORE)).await?;
    let entry = format!("100644 blob {blob}\t.gitignore\n");
    let tree = git_ok(&tmp, &["mktree"], Some(&entry)).await?;
    let commit = git_ok(
        &tmp,
        &["commit-tree", &tree, "-m", "Start the project"],
        None,
    )
    .await?;
    git_ok(&tmp, &["update-ref", "refs/heads/main", &commit], None).await?;
    tokio::fs::rename(&tmp, repo).await
}

/// Commits the workspace and lands it on the repository's `main`.
pub async fn integrate(workspace: &Path, message: &str) -> std::io::Result<Integration> {
    git_ok(workspace, &["add", "-A"], None).await?;
    if git(workspace, &["diff", "--cached", "--quiet"], None)
        .await?
        .is_none()
    {
        git_ok(workspace, &["commit", "-q", "-m", message], None).await?;
    }
    for _ in 0..PUSH_ATTEMPTS {
        if git(workspace, &["fetch", "-q", "origin", "main"], None)
            .await?
            .is_none()
        {
            tokio::time::sleep(Duration::from_millis(500)).await;
            continue;
        }
        let ahead = git_ok(
            workspace,
            &["rev-list", "--count", "origin/main..HEAD"],
            None,
        )
        .await?;
        if ahead == "0" {
            return Ok(Integration::Unchanged);
        }
        let base = git_ok(workspace, &["rev-parse", "origin/main"], None).await?;
        if git(workspace, &["rebase", "-q", "origin/main"], None)
            .await?
            .is_none()
        {
            git(workspace, &["rebase", "--abort"], None).await?;
            // The next attempt redoes the work on top of the newer main.
            git_ok(workspace, &["reset", "-q", "--hard", "origin/main"], None).await?;
            return Ok(Integration::Conflict);
        }
        // A concurrent push makes this one fail; fetch and rebase again.
        if git(workspace, &["push", "-q", "origin", "HEAD:main"], None)
            .await?
            .is_some()
        {
            let commit = git_ok(workspace, &["rev-parse", "--short", "HEAD"], None).await?;
            let files = git_ok(workspace, &["diff", "--name-only", &base, "HEAD"], None).await?;
            return Ok(Integration::Merged {
                commit,
                files: files.lines().map(str::to_owned).collect(),
            });
        }
    }
    Err(std::io::Error::other(
        "could not push to the project repository",
    ))
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
    let repo = bridge
        .ensure_repo(ctx.graph_id)
        .await
        .map_err(|e| ExecError::fatal(format!("cannot create the project repository: {e}")))?;
    let dispatched_at = Utc::now() - CLOCK_SKEW;
    bridge
        .set_issue(issue, &repo)
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
    let mut succeeded = record.status == "succeeded";
    // Merge before the issue closes: closing it makes Symphony delete the workspace.
    let integration = if succeeded {
        let message = format!("{identifier}: {}", node.title);
        let merged = integrate(&bridge.layout().workspace(&identifier), &message).await;
        succeeded = matches!(
            merged,
            Ok(Integration::Unchanged | Integration::Merged { .. })
        );
        Some(merged)
    } else {
        None
    };
    guard.armed = false;
    let final_state = if succeeded { "Done" } else { "Cancelled" };
    bridge
        .set_state(&identifier, final_state)
        .await
        .map_err(|e| ExecError::fatal(format!("cannot write WORKFLOW.md: {e}")))?;
    refresh(http, bridge.url()).await?;
    ctx.tokens(record.tokens.input, record.tokens.output);
    let landed = match integration {
        None => {
            let reason = record.error.unwrap_or_else(|| record.status.clone());
            return Err(ExecError::fatal(format!(
                "symphony run {} {}: {reason}",
                record.id, record.status
            )));
        }
        Some(Err(err)) => {
            return Err(ExecError::fatal(format!(
                "cannot merge the work of symphony run {}: {err}",
                record.id
            )));
        }
        Some(Ok(Integration::Conflict)) => {
            return Err(ExecError::transient(format!(
                "the work of symphony run {} conflicts with changes merged meanwhile",
                record.id
            )));
        }
        Some(Ok(Integration::Unchanged)) => "No files were changed.".to_owned(),
        Some(Ok(Integration::Merged { commit, files })) => format!(
            "Merged commit {commit} into {} ({}).",
            repo.display(),
            files.join(", ")
        ),
    };
    ctx.log(LogLevel::Info, landed.clone());
    let message = final_message(http, bridge.url(), record.id)
        .await
        .unwrap_or_else(|| "symphony run succeeded".into());
    let output = format!("{message}\n\n{landed}");
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
    last_agent_message(&list.events)
}

/// The agent's last message of a run, or failing that the last event message.
fn last_agent_message(events: &[RunEvent]) -> Option<String> {
    let agent_text = |e: &RunEvent| {
        let item = e.payload.pointer("/payload/params/item")?;
        (item.get("type")?.as_str()? == "agentMessage")
            .then(|| item.get("text")?.as_str().map(str::to_owned))
            .flatten()
    };
    let present = |text: &String| !text.trim().is_empty();
    events
        .iter()
        .rev()
        .find_map(|e| agent_text(e).filter(present))
        .or_else(|| {
            events
                .iter()
                .rev()
                .find_map(|e| e.message.clone().filter(present))
        })
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
        let layout = Layout {
            dir: PathBuf::from("/data/it's here"),
            agent_command: Some("/opt/agent.py".into()),
            max_agents: 4,
        };
        let text = render_workflow([issue.clone()].iter(), &layout);
        assert!(text.starts_with("---\n{"));
        let front = text.split("---\n").nth(1).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(front.trim()).unwrap();
        assert_eq!(parsed["tracker"]["kind"], "memory");
        assert_eq!(parsed["tracker"]["provider"]["issues"][0]["state"], "Todo");
        assert_eq!(parsed["agent"]["max_turns"], 1);
        assert_eq!(parsed["agent"]["max_concurrent_agents"], 4);
        assert_eq!(parsed["agent"]["continuation_delay_ms"], 15_000);
        assert_eq!(parsed["workspace"]["root"], "/data/it's here/workspaces");
        assert_eq!(parsed["codex"]["command"], "/opt/agent.py");
        let hook = parsed["hooks"]["after_create"].as_str().unwrap();
        assert!(hook.contains(r#"cat '/data/it'\''s here'/issues/"$id""#));
        assert!(text.contains("{{ issue.identifier }}"));

        let default_agent = Layout {
            agent_command: None,
            ..layout
        };
        let text = render_workflow([issue].iter(), &default_agent);
        let front = text.split("---\n").nth(1).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(front.trim()).unwrap();
        assert!(parsed.get("codex").is_none());
    }

    #[test]
    fn output_is_the_agents_last_message() {
        let events: RunEventList = serde_json::from_value(json!({"events": [
            {"message": "session started (x)", "payload": {}},
            {"message": "item/completed", "payload": {"payload": {"method": "item/completed",
                "params": {"item": {"type": "agentMessage", "text": "Added hello.py"}}}}},
            {"message": "turn/completed", "payload": {"details": {}}}
        ]}))
        .unwrap();
        assert_eq!(
            last_agent_message(&events.events).as_deref(),
            Some("Added hello.py")
        );

        let plain: RunEventList =
            serde_json::from_value(json!({"events": [{"message": "turn/completed"}]})).unwrap();
        assert_eq!(
            last_agent_message(&plain.events).as_deref(),
            Some("turn/completed")
        );
    }

    /// A clone of `repo`, as the `after_create` hook makes it.
    async fn clone(repo: &Path, into: &Path) {
        tokio::fs::create_dir_all(into).await.unwrap();
        git_ok(into, &["clone", "-q", &repo.to_string_lossy(), "."], None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn work_of_parallel_nodes_lands_on_main_and_conflicts_are_reported() {
        let root = std::env::temp_dir().join(format!("nexc-symphony-{}", Uuid::now_v7()));
        let repo = root.join("repos/graph.git");
        create_repo(&repo).await.unwrap();
        let (a, b, c) = (root.join("ws/a"), root.join("ws/b"), root.join("ws/c"));
        for workspace in [&a, &b, &c] {
            clone(&repo, workspace).await;
        }

        // Nothing changed.
        assert_eq!(integrate(&a, "a").await.unwrap(), Integration::Unchanged);

        // Two nodes that started from the same commit and touch different files both land.
        tokio::fs::write(a.join("api.py"), "api = 1\n")
            .await
            .unwrap();
        tokio::fs::write(b.join("ui.js"), "ui = 1\n").await.unwrap();
        tokio::fs::write(c.join("api.py"), "api = 2\n")
            .await
            .unwrap();
        let Integration::Merged { files, .. } = integrate(&a, "a").await.unwrap() else {
            panic!("a should merge");
        };
        assert_eq!(files, ["api.py"]);
        let Integration::Merged { files, .. } = integrate(&b, "b").await.unwrap() else {
            panic!("b should merge after a rebase");
        };
        assert_eq!(files, ["ui.js"]);

        // The same file changed differently: reported, and the workspace is back on main.
        assert_eq!(integrate(&c, "c").await.unwrap(), Integration::Conflict);
        assert_eq!(
            tokio::fs::read_to_string(c.join("api.py")).await.unwrap(),
            "api = 1\n"
        );
        assert!(tokio::fs::try_exists(c.join("ui.js")).await.unwrap());

        // A node that starts later sees the work of both.
        let later = root.join("ws/later");
        clone(&repo, &later).await;
        assert!(tokio::fs::try_exists(later.join("api.py")).await.unwrap());
        assert!(tokio::fs::try_exists(later.join("ui.js")).await.unwrap());
        assert!(
            tokio::fs::try_exists(later.join(".gitignore"))
                .await
                .unwrap()
        );
        tokio::fs::remove_dir_all(&root).await.unwrap();
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
