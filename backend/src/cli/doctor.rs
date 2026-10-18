//! `nexc doctor`: checks configuration, database and backends.

use std::process::ExitCode;
use std::time::Duration;

use crate::app::http_client;
use crate::config::Settings;
use crate::domain::settings::LlmProviderKind;
use crate::ontology::OntologyBackend;
use crate::ontology::grpc::GrpcOntology;
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
    check_runtime_grpc(&mut r, &settings).await;
    check_ontology(&mut r, &settings).await;
    check_firewall(&mut r, &settings);
    check_otlp(&mut r, &settings).await;
    check_sso(&mut r, &settings).await;
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

/// The runtime's gRPC endpoint (`AgentRuntime/Health`), the wire the backend
/// speaks unless `NEXC_RUNTIME_PROTOCOL=ndjson`.
async fn check_runtime_grpc(r: &mut Report, settings: &Settings) {
    use crate::config::RuntimeProtocol;
    if settings.runtime_protocol == RuntimeProtocol::Ndjson {
        return r.line(
            Level::Warn,
            "runtime grpc",
            "off: NEXC_RUNTIME_PROTOCOL=ndjson (the NDJSON wire is kept for one release)",
        );
    }
    let url = settings.runtime_grpc_url.trim_end_matches('/');
    let channel = match crate::engine::executor::runtime_grpc::channel(url) {
        Ok(channel) => channel,
        Err(err) => return r.line(Level::Fail, "runtime grpc", format!("{url}: {err}")),
    };
    let probe = async {
        let mut client =
            crate::rpc::runtime::agent_runtime_client::AgentRuntimeClient::new(channel);
        let mut request = tonic::Request::new(crate::rpc::runtime::HealthRequest {});
        if let Ok(bearer) = format!("Bearer {}", settings.runtime_token.expose()).parse() {
            request.metadata_mut().insert("authorization", bearer);
        }
        client.health(request).await
    };
    match tokio::time::timeout(Duration::from_secs(3), probe).await {
        Ok(Ok(reply)) => {
            let reply = reply.into_inner();
            r.line(
                Level::Ok,
                "runtime grpc",
                format!(
                    "{url}: runtime {} ({} run(s) active)",
                    reply.version, reply.runs_active
                ),
            );
        }
        Ok(Err(status)) => r.line(
            Level::Warn,
            "runtime grpc",
            format!("{url}: {} ({})", status.message(), status.code()),
        ),
        Err(_) => r.line(
            Level::Warn,
            "runtime grpc",
            format!("{url}: no answer within 3 seconds"),
        ),
    }
}

/// The Agent Firewall API: where it will listen and the key it signs with.
fn check_firewall(r: &mut Report, settings: &Settings) {
    match settings.grpc_addr {
        None => r.line(
            Level::Ok,
            "firewall api",
            "disabled (set NEXC_GRPC_ADDR to serve the Agent Firewall API)",
        ),
        Some(addr) => match crate::security::signing::Signer::from_seed(
            settings.firewall_signing_key.expose(),
        ) {
            Ok(signer) => r.line(
                Level::Ok,
                "firewall api",
                format!(
                    "{addr}: signs with Ed25519 key {}{}",
                    hex::encode(signer.public_key()),
                    if std::env::var("NEXC_FIREWALL_SIGNING_KEY").is_ok_and(|k| !k.is_empty()) {
                        ""
                    } else {
                        " (derived from NEXC_MASTER_KEY)"
                    }
                ),
            ),
            Err(err) => r.line(Level::Fail, "firewall api", err.to_string()),
        },
    }
}

/// The OTLP collector the trace is exported to: disabled, reachable, or not.
async fn check_otlp(r: &mut Report, settings: &Settings) {
    let Some(endpoint) = &settings.otlp_endpoint else {
        return r.line(
            Level::Ok,
            "otlp export",
            "disabled (set NEXC_OTLP_ENDPOINT to export the trace as OpenTelemetry spans)",
        );
    };
    let connect = async {
        tonic::transport::Endpoint::from_shared(endpoint.clone())?
            .connect_timeout(Duration::from_secs(3))
            .connect()
            .await
            .map_err(anyhow::Error::from)
    };
    match tokio::time::timeout(Duration::from_secs(3), connect).await {
        Ok(Ok(_)) => r.line(Level::Ok, "otlp export", format!("{endpoint}: reachable")),
        Ok(Err(err)) => r.line(Level::Warn, "otlp export", format!("{endpoint}: {err}")),
        Err(_) => r.line(
            Level::Warn,
            "otlp export",
            format!("{endpoint}: no answer within 3 seconds"),
        ),
    }
}

/// onto-intelli, the audit's query engine: disabled, reachable (with its
/// version and model), or not.
async fn check_ontology(r: &mut Report, settings: &Settings) {
    let Some(url) = &settings.onto_url else {
        return r.line(
            Level::Ok,
            "onto-intelli",
            "disabled (set NEXC_ONTO_URL to ask the audit)",
        );
    };
    let token = settings.onto_token.as_ref().map(|t| t.expose().as_str());
    let backend = match GrpcOntology::new(url, token) {
        Ok(backend) => backend,
        Err(err) => return r.line(Level::Fail, "onto-intelli", err.to_string()),
    };
    match tokio::time::timeout(Duration::from_secs(3), backend.health()).await {
        Ok(Ok(health)) => r.line(
            Level::Ok,
            "onto-intelli",
            format!(
                "{url}: onto-intelli {} (revision {}, explains with {}{})",
                health.version,
                health.revision,
                health.llm,
                if health.llm_available {
                    ""
                } else {
                    ", unavailable"
                }
            ),
        ),
        Ok(Err(err)) => r.line(Level::Warn, "onto-intelli", format!("{url}: {err}")),
        Err(_) => r.line(
            Level::Warn,
            "onto-intelli",
            format!("{url}: no answer within 3 seconds"),
        ),
    }
}

/// Single sign-on: off (and nothing of it half set), or the provider's
/// discovery document and signing keys fetched and checked, with what the
/// settings around it mean.
async fn check_sso(r: &mut Report, settings: &Settings) {
    use crate::config::oidc::redirect_concerns;
    use crate::security::oidc::OidcClient;

    let Some(oidc) = &settings.oidc else {
        let ignored: Vec<&str> = crate::config::vars::ENV_VARS
            .iter()
            .map(|var| var.name)
            .filter(|name| name.starts_with("NEXC_OIDC_"))
            .filter(|name| std::env::var(name).is_ok_and(|v| !v.trim().is_empty()))
            .filter(|name| {
                // `nexc init` writes these with their defaults.
                let var = crate::config::vars::ENV_VARS
                    .iter()
                    .find(|v| v.name == *name);
                var.is_none_or(|var| std::env::var(name).ok().as_deref() != Some(var.default))
            })
            .collect();
        return if ignored.is_empty() {
            r.line(
                Level::Ok,
                "sso",
                "disabled (set NEXC_OIDC_ISSUER to sign in with an OpenID Connect provider)",
            )
        } else {
            r.line(
                Level::Warn,
                "sso",
                format!(
                    "disabled: NEXC_OIDC_ISSUER is unset, so {} {} ignored",
                    ignored.join(", "),
                    if ignored.len() == 1 { "is" } else { "are" }
                ),
            )
        };
    };
    let client = match OidcClient::new(oidc.clone(), settings.is_production()) {
        Ok(client) => client,
        Err(err) => return r.line(Level::Fail, "sso", err.to_string()),
    };
    match client.discovery().await {
        Ok(doc) => {
            r.line(
                Level::Ok,
                "sso",
                format!(
                    "{}: discovery read (authorize at {}, tokens at {})",
                    oidc.issuer, doc.authorization_endpoint, doc.token_endpoint
                ),
            );
            if doc.advertises_s256() == Some(false) {
                r.line(
                    Level::Warn,
                    "sso",
                    "the provider does not list S256 among its PKCE methods; sign-in sends an S256 challenge anyway",
                );
            }
            match client.get_json(&doc.jwks_uri).await {
                Ok(jwks) => {
                    // Only what a sign-in would verify with counts.
                    let usable = crate::security::oidc::usable_signing_keys(&jwks);
                    let listed = jwks["keys"].as_array().map_or(0, Vec::len);
                    let others = match listed.saturating_sub(usable) {
                        0 => String::new(),
                        n => format!("; {n} other key(s) are not used"),
                    };
                    if usable == 0 {
                        r.line(
                            Level::Fail,
                            "sso",
                            format!(
                                "{}: no RS256 or ES256 (P-256) signing key{others}: every sign-in would fail",
                                doc.jwks_uri
                            ),
                        );
                    } else {
                        r.line(
                            Level::Ok,
                            "sso",
                            format!(
                                "{}: {usable} RS256/ES256 signing key(s){others}",
                                doc.jwks_uri
                            ),
                        );
                    }
                }
                Err(err) => r.line(Level::Fail, "sso", err.to_string()),
            }
        }
        // The error names the address it failed at.
        Err(err) => r.line(Level::Fail, "sso", err.to_string()),
    }
    let derived = !std::env::var("NEXC_OIDC_REDIRECT_URL").is_ok_and(|v| !v.trim().is_empty());
    r.line(
        Level::Ok,
        "sso",
        format!(
            "redirect URL {}{} (register it at the provider)",
            oidc.redirect_url,
            if derived {
                ", from NEXC_PUBLIC_URL"
            } else {
                ""
            }
        ),
    );
    let development = !settings.is_production();
    for concern in redirect_concerns(&oidc.redirect_url, &settings.public_url, development) {
        r.line(Level::Warn, "sso", concern);
    }
    let break_glass = oidc
        .break_glass_email
        .as_ref()
        .filter(|_| !oidc.allow_local_login);
    match (oidc.allow_local_login, break_glass) {
        (true, _) => r.line(Level::Ok, "sso", "passwords sign in too"),
        (false, Some(_)) => {}
        (false, None) => r.line(
            Level::Warn,
            "sso",
            "password sign-in is off and NEXC_OIDC_BREAK_GLASS_EMAIL is unset: while the provider is down nobody signs in",
        ),
    }
    if oidc.workspace_roles.is_empty() && break_glass.is_none() {
        return;
    }
    let connect = repo::connect(settings.database_url.expose(), 1);
    let Ok(Ok(pool)) = tokio::time::timeout(Duration::from_secs(10), connect).await else {
        return r.line(
            Level::Warn,
            "sso",
            "cannot check the break-glass administrator nor NEXC_OIDC_WORKSPACE_ROLES: no database",
        );
    };
    if let Some(email) = break_glass {
        match crate::engine::sso::break_glass_problem(&pool, email).await {
            Ok(None) => r.line(
                Level::Ok,
                "sso",
                format!(
                    "password sign-in is off, except for the break-glass administrator {email}"
                ),
            ),
            Ok(Some(problem)) => r.line(Level::Fail, "sso", problem),
            Err(err) => r.line(
                Level::Warn,
                "sso",
                format!("cannot check the break-glass administrator: {err}"),
            ),
        }
    }
    for mapping in &oidc.workspace_roles {
        match repo::workspaces::name_of(&pool, mapping.workspace).await {
            Ok(Some(name)) => r.line(
                Level::Ok,
                "sso",
                format!("group {} is {} of {name}", mapping.group, mapping.role),
            ),
            Ok(None) => r.line(
                Level::Warn,
                "sso",
                format!(
                    "group {}: no workspace has the id {}; the mapping is skipped",
                    mapping.group, mapping.workspace
                ),
            ),
            Err(err) => r.line(
                Level::Warn,
                "sso",
                format!("cannot check {}: {err}", mapping.workspace),
            ),
        }
    }
    pool.close().await;
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
