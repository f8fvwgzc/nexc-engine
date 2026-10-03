//! `nexc doctor`: checks configuration, database and backends.

use std::process::ExitCode;
use std::time::Duration;

use crate::app::http_client;
use crate::config::Settings;
use crate::domain::settings::LlmProviderKind;
use crate::orchestrator::health::probe;
use crate::repo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Ok,
    Warn,
    Fail,
}

struct Report {
    failed: bool,
}

impl Report {
    fn line(&mut self, level: Level, check: &str, detail: impl AsRef<str>) {
        let tag = match level {
            Level::Ok => "ok  ",
            Level::Warn => "warn",
            Level::Fail => "FAIL",
        };
        self.failed |= level == Level::Fail;
        println!("[{tag}] {check:<14} {}", detail.as_ref());
    }
}

/// Runs every check and prints a report; exit code 1 when any check failed.
pub async fn run() -> ExitCode {
    let mut r = Report { failed: false };
    let settings = match Settings::from_env() {
        Ok(s) => {
            r.line(Level::Ok, "config", format!("valid ({:?})", s.env));
            s
        }
        Err(err) => {
            r.line(Level::Fail, "config", err.to_string());
            return ExitCode::FAILURE;
        }
    };
    check_database(&mut r, &settings).await;
    check_backends(&mut r, &settings).await;
    check_llm(&mut r, &settings);
    if r.failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

async fn check_database(r: &mut Report, settings: &Settings) {
    let connect = repo::connect(settings.database_url.expose(), 1);
    let pool = match tokio::time::timeout(Duration::from_secs(10), connect).await {
        Ok(Ok(pool)) => pool,
        Ok(Err(err)) => return r.line(Level::Fail, "database", format!("cannot connect: {err}")),
        Err(_) => return r.line(Level::Fail, "database", "connection timed out"),
    };
    match repo::server_version(&pool).await {
        Ok(v) if v >= 170_000 => r.line(
            Level::Ok,
            "database",
            format!("PostgreSQL {}.{}", v / 10_000, v % 10_000),
        ),
        Ok(v) => r.line(
            Level::Warn,
            "database",
            format!("PostgreSQL {} is older than 17", v / 10_000),
        ),
        Err(err) => r.line(
            Level::Fail,
            "database",
            format!("cannot read server version: {err}"),
        ),
    }
    let applied: Result<i64, _> =
        sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success")
            .fetch_one(&pool)
            .await;
    let known = repo::MIGRATOR.iter().count() as i64;
    match applied {
        Ok(n) if n >= known => r.line(Level::Ok, "migrations", format!("{n} applied")),
        Ok(n) => r.line(
            Level::Warn,
            "migrations",
            format!("{} pending (run `nexc migrate`)", known - n),
        ),
        Err(_) => r.line(
            Level::Warn,
            "migrations",
            "not initialised (run `nexc migrate`)",
        ),
    }
    pool.close().await;
}

async fn check_backends(r: &mut Report, settings: &Settings) {
    let Ok(http) = http_client() else {
        return r.line(Level::Fail, "http client", "cannot build HTTP client");
    };
    let timeout = Duration::from_secs(3);
    let runtime = settings.runtime_url.trim_end_matches('/');
    let health = probe(
        http.get(format!("{runtime}/healthz"))
            .bearer_auth(settings.runtime_token.expose())
            .timeout(timeout),
        true,
        runtime.to_owned(),
    )
    .await;
    let detail = health.detail.unwrap_or_else(|| "reachable".into());
    let level = if health.ok { Level::Ok } else { Level::Warn };
    r.line(level, "agent runtime", format!("{runtime}: {detail}"));

    if !settings.symphony_enabled {
        return r.line(Level::Ok, "symphony", "disabled");
    }
    let url = settings.symphony_url.trim_end_matches('/');
    let health = probe(
        http.get(format!("{url}/api/v1/health")).timeout(timeout),
        true,
        url.to_owned(),
    )
    .await;
    let level = if health.ok { Level::Ok } else { Level::Warn };
    r.line(
        level,
        "symphony",
        format!(
            "{url}: {}",
            health.detail.unwrap_or_else(|| "reachable".into())
        ),
    );
}

fn check_llm(r: &mut Report, settings: &Settings) {
    match settings.llm_provider {
        LlmProviderKind::Demo => r.line(Level::Ok, "llm", "demo provider (offline, no key needed)"),
        LlmProviderKind::ClaudeCode => {
            let version = std::process::Command::new(&settings.claude_bin)
                .arg("--version")
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
            match version {
                Some(v) => r.line(
                    Level::Ok,
                    "llm",
                    format!("claude_code / {} via `{}` ({v})", settings.llm_model, settings.claude_bin),
                ),
                None => r.line(
                    Level::Fail,
                    "llm",
                    format!(
                        "claude_code: `{}` not found; install Claude Code and log in, or set NEXC_CLAUDE_BIN",
                        settings.claude_bin
                    ),
                ),
            }
        }
        LlmProviderKind::OpenaiCompatible => r.line(
            Level::Ok,
            "llm",
            format!("openai_compatible at {}", settings.llm_base_url.as_deref().unwrap_or("http://localhost:11434/v1")),
        ),
        LlmProviderKind::Anthropic if settings.anthropic_api_key.is_some() => {
            r.line(Level::Ok, "llm", format!("anthropic / {} (server key set)", settings.llm_model));
        }
        LlmProviderKind::Anthropic => r.line(
            Level::Warn,
            "llm",
            "no ANTHROPIC_API_KEY: users must add their own key in Settings (or use NEXC_LLM_PROVIDER=demo)",
        ),
    }
}
