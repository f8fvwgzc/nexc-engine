//! `nexc doctor`: checks configuration, database and backends.

use std::process::ExitCode;
use std::time::Duration;

use crate::app::http_client;
use crate::config::Settings;
use crate::domain::ontology::OntoMode;
use crate::domain::settings::LlmProviderKind;
use crate::ontology::grpc::GrpcOntology;
use crate::ontology::{OntoError, OntologyBackend, Principal, role};
use crate::planner::health::probe;
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
    check_agent_tokens(&mut r, &settings);
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
                    "{addr}: signs with Ed25519 key {} (kid {}){}",
                    hex::encode(signer.public_key()),
                    signer.kid(),
                    if settings.signing_key_derived {
                        " (derived from NEXC_MASTER_KEY: set NEXC_SIGNING_KEY to keep them apart)"
                    } else {
                        ""
                    }
                ),
            ),
            Err(err) => r.line(Level::Fail, "firewall api", err.to_string()),
        },
    }
}

/// The key per-run agent tokens are signed with and the public keys that
/// verify them: what an operator copies into the runtime's
/// `NEXC_AGENT_TOKEN_VERIFY_KEYS`, or checks against its JWKS URL.
fn check_agent_tokens(r: &mut Report, settings: &Settings) {
    match crate::security::agent_token::AgentTokenKeys::new(
        settings.agent_token_signing_key.expose(),
        &settings.agent_token_verify_keys,
    ) {
        Ok(keys) => {
            let jwks = keys.jwks();
            r.line(
                Level::Ok,
                "agent tokens",
                format!(
                    "EdDSA, kid {} (public key {}){}; {} key(s) published at /api/v1/agent-tokens/jwks.json",
                    keys.kid(),
                    jwks.keys[0].x,
                    if std::env::var("NEXC_AGENT_TOKEN_SIGNING_KEY").is_ok_and(|k| !k.is_empty()) {
                        ""
                    } else {
                        ", derived from NEXC_MASTER_KEY"
                    },
                    jwks.keys.len()
                ),
            );
        }
        Err(err) => r.line(Level::Fail, "agent tokens", err.to_string()),
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

/// onto-intelli, where the ontology lives: off (graphs keep the read-only
/// starter vocabulary), or reachable (with its version and model), able to
/// act for members, and reachable by browsers at `NEXC_ONTO_PUBLIC_URL`.
async fn check_ontology(r: &mut Report, settings: &Settings) {
    let url = match (&settings.onto_mode, &settings.onto_url) {
        (OntoMode::Service, Some(url)) => url,
        (OntoMode::Off, Some(url)) => {
            return r.line(
                Level::Ok,
                "onto-intelli",
                format!(
                    "NEXC_ONTO_MODE=off: graphs use the read-only starter vocabulary, no \
                     workspace map (NEXC_ONTO_URL={url} is ignored)"
                ),
            );
        }
        _ => {
            return r.line(
                Level::Ok,
                "onto-intelli",
                "off: graphs use the read-only starter vocabulary, no workspace map \
                 (set NEXC_ONTO_URL; docs/ONTOLOGY.md)",
            );
        }
    };
    let token = settings.onto_token.as_ref().map(|t| t.expose().as_str());
    let backend = match GrpcOntology::new(url, token) {
        Ok(backend) => backend,
        Err(err) => return r.line(Level::Fail, "onto-intelli", err.to_string()),
    };
    let answers = match tokio::time::timeout(Duration::from_secs(3), backend.health()).await {
        Ok(Ok(health)) => {
            r.line(
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
            );
            true
        }
        Ok(Err(err)) => {
            r.line(Level::Warn, "onto-intelli", format!("{url}: {err}"));
            false
        }
        Err(_) => {
            r.line(
                Level::Warn,
                "onto-intelli",
                format!("{url}: no answer within 3 seconds"),
            );
            false
        }
    };
    if answers {
        check_onto_impersonation(r, &backend, settings).await;
    }
    check_onto_public_url(r, settings).await;
}

/// The roles nexc asserts for someone (`docs/ONTOLOGY.md` §2): the token's
/// `impersonate_roles` must allow every one of them.
const ASSERTED_ROLES: [&str; 5] = [
    role::VIEWER,
    role::MEMBER,
    role::AGENT,
    role::APPROVER,
    role::SCHEMA_ADMIN,
];

/// Whether `NEXC_ONTO_TOKEN` may act for members: a short session minted
/// for a throwaway principal with every role nexc asserts, in the default
/// tenant (nothing is created). A token without `impersonate = true` passes
/// the health check, yet every member, assistant and run engine call is
/// refused.
async fn check_onto_impersonation(r: &mut Report, backend: &GrpcOntology, settings: &Settings) {
    let probe = Principal {
        id: "user:nexc-doctor".to_owned(),
        roles: ASSERTED_ROLES.iter().map(|r| (*r).to_owned()).collect(),
        entity: None,
        tenant: String::new(),
        workspace: format!("{}doctor", settings.onto_workspace_prefix),
    };
    let check = "onto-intelli";
    match tokio::time::timeout(Duration::from_secs(3), backend.mint_session(&probe, 60)).await {
        Ok(Ok(_)) => r.line(
            Level::Ok,
            check,
            "NEXC_ONTO_TOKEN acts for members, with every role nexc asserts",
        ),
        Ok(Err(OntoError::Denied(detail))) => r.line(
            Level::Fail,
            check,
            format!(
                "NEXC_ONTO_TOKEN may not act for members ({detail}): every member, assistant \
                 and run engine call would be refused. Give its [[auth.tokens]] impersonate = true \
                 and impersonate_roles = [{}] (deploy/onto/onto-intelli.toml)",
                ASSERTED_ROLES
                    .iter()
                    .map(|r| format!("\"{r}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
        Ok(Err(err)) => r.line(
            Level::Warn,
            check,
            format!("cannot mint a browser session: {err}"),
        ),
        Err(_) => r.line(
            Level::Warn,
            check,
            "cannot mint a browser session: no answer within 3 seconds",
        ),
    }
}

/// A host only the cluster's DNS resolves (a Service name, `*.svc`,
/// `*.cluster.local`): a browser cannot reach it.
fn internal_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.parse::<std::net::IpAddr>().is_ok() || host.starts_with('[') {
        return false;
    }
    (!host.contains('.') && host != "localhost")
        || host.ends_with(".svc")
        || host.contains(".svc.")
        || host.ends_with(".cluster.local")
}

/// Where browsers load onto-intelli's UI from (`NEXC_ONTO_PUBLIC_URL`, else
/// `NEXC_ONTO_URL`): a host a browser can resolve, and `/ui/` answering
/// (from here; a browser may reach it another way).
async fn check_onto_public_url(r: &mut Report, settings: &Settings) {
    let check = "onto-intelli ui";
    let Some(public) = &settings.onto_public_url else {
        return;
    };
    let explicit = std::env::var("NEXC_ONTO_PUBLIC_URL").is_ok_and(|v| !v.trim().is_empty());
    let host = reqwest::Url::parse(public)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default();
    if internal_host(&host) {
        let setting = if explicit {
            "NEXC_ONTO_PUBLIC_URL"
        } else {
            "NEXC_ONTO_URL (NEXC_ONTO_PUBLIC_URL is not set)"
        };
        return r.line(
            Level::Warn,
            check,
            format!(
                "browsers are sent to {public}, from {setting}, whose host {host} only the \
                 cluster resolves: the workspace map will not load. Set NEXC_ONTO_PUBLIC_URL to \
                 where browsers reach onto-intelli"
            ),
        );
    }
    let Ok(http) = http_client() else {
        return r.line(Level::Warn, check, "no HTTP client");
    };
    let ui = format!("{public}/ui/");
    match tokio::time::timeout(Duration::from_secs(3), http.get(&ui).send()).await {
        Ok(Ok(response)) if response.status().is_success() => {
            r.line(Level::Ok, check, format!("{ui} answers"));
        }
        Ok(Ok(response)) => r.line(
            Level::Warn,
            check,
            format!("{ui} answers {}", response.status()),
        ),
        Ok(Err(err)) => r.line(
            Level::Warn,
            check,
            format!("{ui} does not answer from here: {err}"),
        ),
        Err(_) => r.line(
            Level::Warn,
            check,
            format!("{ui}: no answer within 3 seconds"),
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

#[cfg(test)]
mod tests {
    use super::internal_host;

    #[test]
    fn a_cluster_only_host_is_told_from_one_browsers_reach() {
        for host in [
            "onto-intelli",
            "onto-intelli.nexc.svc",
            "onto-intelli.nexc.svc.cluster.local",
        ] {
            assert!(internal_host(host), "{host}");
        }
        for host in [
            "localhost",
            "127.0.0.1",
            "[::1]",
            "onto.example.com",
            "onto.nexc.local",
        ] {
            assert!(!internal_host(host), "{host}");
        }
    }
}
