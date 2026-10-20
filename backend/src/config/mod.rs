//! Typed, validated settings loaded from the environment (and `.env`).
#![forbid(unsafe_code)]

pub mod oidc;
mod secret;
pub mod vars;

use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use base64::Engine as _;

pub use secret::Secret;

use crate::domain::ontology::OntoMode;
use crate::domain::settings::LlmProviderKind;
use crate::domain::user;
use crate::domain::validation::FieldErrors;
use crate::security::random::random_bytes;

/// Deployment environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Development,
    Production,
}

/// All runtime configuration. Secrets are wrapped in [`Secret`].
#[derive(Debug, Clone)]
pub struct Settings {
    pub env: Environment,
    pub host: IpAddr,
    pub port: u16,
    pub database_url: Secret<String>,
    pub db_max_connections: u32,
    pub data_dir: PathBuf,
    pub jwt_secret: Secret<Vec<u8>>,
    pub master_key: Secret<[u8; 32]>,
    pub cors_origins: Vec<String>,
    pub cookie_secure: bool,
    pub allow_signup: bool,
    pub admin_email: Option<String>,
    pub admin_password: Option<Secret<String>>,
    pub access_ttl: Duration,
    pub refresh_ttl: Duration,
    pub llm_provider: LlmProviderKind,
    pub llm_model: String,
    pub llm_base_url: Option<String>,
    pub llm_fallbacks: bool,
    pub anthropic_api_key: Option<Secret<String>>,
    /// OpenAI-compatible embeddings endpoint for the knowledge base (e.g.
    /// `https://api.openai.com/v1`); unset uses the built-in word-hashing embedding.
    pub embed_base_url: Option<String>,
    pub embed_model: Option<String>,
    pub embed_api_key: Option<Secret<String>>,
    /// Asks the endpoint for vectors of this size, when the model can shorten them.
    pub embed_dims: Option<u32>,
    /// Claude Code CLI binary used by the `claude_code` provider.
    pub claude_bin: String,
    pub runtime_url: String,
    /// How the backend speaks to the runtime: the gRPC stream, or the NDJSON stream of one release ago.
    pub runtime_protocol: RuntimeProtocol,
    /// The runtime's gRPC endpoint (`http://host:8095`).
    pub runtime_grpc_url: String,
    pub runtime_token: Secret<String>,
    /// Signs the per-run agent tokens the runtime verifies: `NEXC_AGENT_TOKEN_KEY`,
    /// or derived from the runtime token when it is unset.
    pub agent_token_key: Secret<Vec<u8>>,
    pub symphony_enabled: bool,
    pub symphony_url: String,
    pub symphony_workflow: PathBuf,
    /// Agent command for texc-symphony; Symphony's default (Codex) when unset.
    pub symphony_agent_command: Option<String>,
    /// How nexc reaches onto-intelli (`NEXC_ONTO_MODE`): over gRPC (the default when
    /// `NEXC_ONTO_URL` is set), embedded (not in this build) or not at all.
    pub onto_mode: OntoMode,
    /// gRPC and HTTP endpoint of `onto serve` (`http://host:50051`).
    pub onto_url: Option<String>,
    /// Bearer token onto-intelli knows the backend by (`impersonate = true` in its config).
    pub onto_token: Option<Secret<String>>,
    /// Prefix of the onto-intelli workspace that holds each nexc workspace's entities.
    pub onto_workspace_prefix: String,
    /// Where browsers reach `onto serve` (its UI at `/ui/`); `NEXC_ONTO_URL` when unset.
    pub onto_public_url: Option<String>,
    /// How long a graph's vocabulary is served from memory before onto-intelli is
    /// asked again (zero: every read asks); a change made through nexc drops it at once.
    pub onto_vocabulary_ttl: Duration,
    /// How long a browser session for onto-intelli's UI lasts.
    pub onto_session_ttl: Duration,
    /// Copy every graph's stored ontology into onto-intelli when the backend starts
    /// (`nexc onto migrate`, idempotent and resumable).
    pub onto_migrate_on_start: bool,
    pub max_concurrency: usize,
    pub max_attempts: u32,
    pub node_timeout: Duration,
    /// The lease an instance holds on a run it executes (renewed every third of it); a run
    /// whose lease ran out is taken over by another instance (docs/HA.md).
    pub run_lease: Duration,
    pub trust_proxy: bool,
    pub metrics_token: Option<Secret<String>>,
    /// Where the Agent Firewall API listens (`0.0.0.0:50052`); unset keeps it off.
    pub grpc_addr: Option<SocketAddr>,
    /// Ed25519 seed the firewall signs decisions with: `NEXC_FIREWALL_SIGNING_KEY`,
    /// or derived from the master key when unset.
    pub firewall_signing_key: Secret<[u8; 32]>,
    /// OTLP/gRPC endpoint the trace is exported to (`http://host:4317`); unset keeps it off.
    pub otlp_endpoint: Option<String>,
    /// Authenticated API requests one account may make per minute (burst a fifth of it).
    pub api_calls_per_minute: u32,
    /// How long a hot-read cache entry may live (zero turns the caches off); a write
    /// drops the entry on every replica before that.
    pub cache_ttl: Duration,
    /// HTTP/2 keepalive of every gRPC channel and server: how often a PING goes out.
    pub grpc_keepalive: Duration,
    /// How long a PING may go unanswered before the connection is dropped.
    pub grpc_keepalive_timeout: Duration,
    /// Connections per gRPC client channel (runtime, onto-intelli, OTLP collector).
    pub grpc_pool_size: usize,
    /// Consecutive failed provider calls that open the LLM circuit (0 turns the breaker off).
    pub llm_breaker_failures: u32,
    /// How long an open circuit refuses calls before one probe is let through.
    pub llm_breaker_open: Duration,
    /// Gateway calls one agent may make per minute.
    pub gateway_calls_per_minute: u32,
    /// Longest one gateway call may take, the tool included.
    pub gateway_timeout: Duration,
    /// Largest response a gateway call brings back.
    pub gateway_max_response_bytes: usize,
    /// Where the console is reached from outside (`https://nexc.example.com`):
    /// the links alert deliveries carry.
    pub public_url: String,
    /// Lets alert destinations name private and loopback hosts (development only).
    pub alert_allow_private_hosts: bool,
    /// Longest one outbound A2A task may take, the remote agent's work included.
    pub a2a_timeout: Duration,
    /// Lets A2A counterparts be registered on private and loopback hosts (development only).
    pub a2a_allow_private_hosts: bool,
    /// Single sign-on with an OpenID Connect provider; `None` when `NEXC_OIDC_ISSUER` is unset.
    pub oidc: Option<oidc::OidcSettings>,
}

/// The wire between the backend and the agent runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeProtocol {
    /// `AgentRuntime/Execute`, one bidirectional stream per node attempt.
    Grpc,
    /// `POST /v1/execute`, NDJSON; kept for one release.
    Ndjson,
}

impl std::str::FromStr for RuntimeProtocol {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "grpc" => Ok(RuntimeProtocol::Grpc),
            "ndjson" | "http" => Ok(RuntimeProtocol::Ndjson),
            other => Err(format!("unknown runtime protocol `{other}`")),
        }
    }
}

impl std::fmt::Display for RuntimeProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            RuntimeProtocol::Grpc => "grpc",
            RuntimeProtocol::Ndjson => "ndjson",
        })
    }
}

/// Every problem found while loading settings.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid configuration:\n  - {}", .0.join("\n  - "))]
pub struct ConfigError(pub Vec<String>);

/// Loads `.env` from the working directory or its parent (without
/// overriding variables that are already set). Returns the file used.
pub fn load_dotenv() -> Option<PathBuf> {
    ["./.env", "../.env"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .and_then(|p| dotenvy::from_path(&p).ok().map(|()| p))
}

impl Settings {
    /// Reads settings from the process environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Reads settings through `lookup` (the environment in production, a map in tests).
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let mut r = Reader {
            lookup: &lookup,
            errors: Vec::new(),
        };
        let env = match r.string("NEXC_ENV", "development").as_str() {
            "production" | "prod" => Environment::Production,
            "development" | "dev" | "test" => Environment::Development,
            other => {
                r.errors
                    .push(format!("NEXC_ENV: unknown environment `{other}`"));
                Environment::Development
            }
        };
        let prod = env == Environment::Production;
        let mut settings = Settings {
            env,
            host: r.parse("NEXC_HOST", "0.0.0.0".parse().expect("valid ip")),
            port: r.parse("NEXC_PORT", 8080),
            database_url: Secret::new(r.database_url()),
            db_max_connections: r.parse("NEXC_DB_MAX_CONNECTIONS", 20),
            data_dir: PathBuf::from(r.string("NEXC_DATA_DIR", "./data")),
            jwt_secret: Secret::new(r.jwt_secret(prod)),
            master_key: Secret::new(r.master_key(prod)),
            cors_origins: r.cors_origins(),
            // Production always sets `Secure` (browsers accept it on http://localhost too).
            cookie_secure: prod || r.parse("NEXC_COOKIE_SECURE", false),
            allow_signup: r.parse("NEXC_ALLOW_SIGNUP", true),
            admin_email: r
                .optional("NEXC_ADMIN_EMAIL")
                .map(|e| user::normalize_email(&e)),
            admin_password: r.optional("NEXC_ADMIN_PASSWORD").map(Secret::new),
            access_ttl: Duration::from_secs(r.parse("NEXC_ACCESS_TTL_SECS", 900)),
            refresh_ttl: Duration::from_secs(r.parse("NEXC_REFRESH_TTL_SECS", 1_209_600)),
            llm_provider: r.parse("NEXC_LLM_PROVIDER", LlmProviderKind::Anthropic),
            llm_model: r.string("NEXC_LLM_MODEL", "claude-opus-5"),
            llm_base_url: r.optional("NEXC_LLM_BASE_URL"),
            llm_fallbacks: r.parse("NEXC_LLM_FALLBACKS", true),
            anthropic_api_key: r.optional("ANTHROPIC_API_KEY").map(Secret::new),
            embed_base_url: r.optional("NEXC_EMBED_BASE_URL"),
            embed_model: r.optional("NEXC_EMBED_MODEL"),
            embed_api_key: r.optional("NEXC_EMBED_API_KEY").map(Secret::new),
            embed_dims: r.optional("NEXC_EMBED_DIMS").and_then(|d| d.parse().ok()),
            claude_bin: r.string("NEXC_CLAUDE_BIN", "claude"),
            runtime_url: r.string("NEXC_RUNTIME_URL", "http://localhost:8090"),
            runtime_protocol: r.parse("NEXC_RUNTIME_PROTOCOL", RuntimeProtocol::Grpc),
            runtime_grpc_url: r.string("NEXC_RUNTIME_GRPC_URL", "http://localhost:8095"),
            agent_token_key: Secret::new(r.agent_token_key(prod)),
            runtime_token: Secret::new(r.runtime_token(prod)),
            symphony_enabled: r.parse("NEXC_SYMPHONY_ENABLED", false),
            symphony_url: r.string("NEXC_SYMPHONY_URL", "http://localhost:4000"),
            symphony_workflow: PathBuf::from(
                r.string("NEXC_SYMPHONY_WORKFLOW", "./data/symphony/WORKFLOW.md"),
            ),
            symphony_agent_command: r.optional("NEXC_SYMPHONY_AGENT_COMMAND"),
            onto_mode: r.onto_mode(),
            onto_url: r.optional("NEXC_ONTO_URL"),
            onto_token: r.optional("NEXC_ONTO_TOKEN").map(Secret::new),
            onto_workspace_prefix: r.string("NEXC_ONTO_WORKSPACE_PREFIX", "nexc-"),
            onto_public_url: r
                .optional("NEXC_ONTO_PUBLIC_URL")
                .or_else(|| r.optional("NEXC_ONTO_URL"))
                .map(|u| u.trim_end_matches('/').to_owned()),
            onto_vocabulary_ttl: Duration::from_secs(r.parse("NEXC_ONTO_VOCABULARY_TTL_SECS", 30)),
            onto_session_ttl: Duration::from_secs(r.parse("NEXC_ONTO_SESSION_TTL_SECS", 3600)),
            onto_migrate_on_start: r.parse("NEXC_ONTO_MIGRATE_ON_START", true),
            max_concurrency: r.parse("NEXC_MAX_CONCURRENCY", 4),
            max_attempts: r.parse("NEXC_MAX_ATTEMPTS", 3),
            node_timeout: Duration::from_secs(r.parse("NEXC_NODE_TIMEOUT_SECS", 600)),
            run_lease: Duration::from_secs(r.parse("NEXC_RUN_LEASE_SECS", 30)),
            trust_proxy: r.parse("NEXC_TRUST_PROXY", false),
            metrics_token: r.optional("NEXC_METRICS_TOKEN").map(Secret::new),
            grpc_addr: r.grpc_addr(),
            firewall_signing_key: Secret::new(r.firewall_signing_key()),
            otlp_endpoint: r.optional("NEXC_OTLP_ENDPOINT"),
            api_calls_per_minute: r.parse("NEXC_API_CALLS_PER_MINUTE", 600),
            cache_ttl: Duration::from_secs(r.parse("NEXC_CACHE_TTL_SECS", 60)),
            grpc_keepalive: Duration::from_secs(r.parse("NEXC_GRPC_KEEPALIVE_SECS", 30)),
            grpc_keepalive_timeout: Duration::from_secs(
                r.parse("NEXC_GRPC_KEEPALIVE_TIMEOUT_SECS", 10),
            ),
            grpc_pool_size: r.parse("NEXC_GRPC_POOL_SIZE", 2),
            llm_breaker_failures: r.parse("NEXC_LLM_BREAKER_FAILURES", 5),
            llm_breaker_open: Duration::from_secs(r.parse("NEXC_LLM_BREAKER_OPEN_SECS", 30)),
            gateway_calls_per_minute: r.parse("NEXC_GATEWAY_CALLS_PER_MINUTE", 120),
            gateway_timeout: Duration::from_secs(r.parse("NEXC_GATEWAY_TIMEOUT_SECS", 30)),
            gateway_max_response_bytes: r.parse("NEXC_GATEWAY_MAX_RESPONSE_BYTES", 1024 * 1024),
            public_url: r
                .string("NEXC_PUBLIC_URL", "http://localhost:8080")
                .trim_end_matches('/')
                .to_owned(),
            alert_allow_private_hosts: r.parse("NEXC_ALERT_ALLOW_PRIVATE_HOSTS", false),
            a2a_timeout: Duration::from_secs(r.parse("NEXC_A2A_TIMEOUT_SECS", 120)),
            a2a_allow_private_hosts: r.parse("NEXC_A2A_ALLOW_PRIVATE_HOSTS", false),
            oidc: None,
        };
        settings.oidc = oidc::read(r.lookup, prod, &settings.public_url, &mut r.errors);
        let mut errors = r.errors;
        settings.check_invariants(&mut errors);
        if errors.is_empty() {
            Ok(settings)
        } else {
            Err(ConfigError(errors))
        }
    }

    /// True in production.
    pub fn is_production(&self) -> bool {
        self.env == Environment::Production
    }

    /// Where uploaded documents are kept, one file per document id.
    pub fn documents_dir(&self) -> PathBuf {
        self.data_dir.join("documents")
    }

    /// Directory holding run artifacts.
    pub fn artifacts_dir(&self) -> PathBuf {
        self.data_dir.join("artifacts")
    }

    /// How every gRPC client channel is kept (`NEXC_GRPC_*`).
    pub fn grpc_channel(&self) -> crate::rpc::ChannelSettings {
        crate::rpc::ChannelSettings {
            keepalive: self.grpc_keepalive,
            keepalive_timeout: self.grpc_keepalive_timeout,
            pool: self.grpc_pool_size,
            ..crate::rpc::ChannelSettings::default()
        }
    }

    fn check_invariants(&self, errors: &mut Vec<String>) {
        if !(1..=32).contains(&self.max_concurrency) {
            errors.push("NEXC_MAX_CONCURRENCY must be between 1 and 32".into());
        }
        if !(1..=10).contains(&self.max_attempts) {
            errors.push("NEXC_MAX_ATTEMPTS must be between 1 and 10".into());
        }
        if self.node_timeout.is_zero() {
            errors.push("NEXC_NODE_TIMEOUT_SECS must be positive".into());
        }
        if self.run_lease.is_zero() || self.run_lease > Duration::from_secs(3600) {
            errors.push("NEXC_RUN_LEASE_SECS must be between 1 and 3600".into());
        } else if self.is_production() && self.run_lease < MIN_PRODUCTION_LEASE {
            errors.push(format!(
                "NEXC_RUN_LEASE_SECS must be at least {} in production: a shorter lease is given up, and its run taken over, on any slow answer of the database",
                MIN_PRODUCTION_LEASE.as_secs()
            ));
        }
        if !(1..=1000).contains(&self.db_max_connections) {
            errors.push("NEXC_DB_MAX_CONNECTIONS must be between 1 and 1000".into());
        }
        if let Some(url) = &self.onto_url
            && !(url.starts_with("http://") || url.starts_with("https://"))
        {
            errors.push("NEXC_ONTO_URL must start with http:// or https://".into());
        }
        if let Some(url) = &self.onto_public_url
            && !(url.starts_with("http://") || url.starts_with("https://"))
        {
            errors.push("NEXC_ONTO_PUBLIC_URL must start with http:// or https://".into());
        }
        match self.onto_mode {
            OntoMode::Service if self.onto_url.is_none() => errors.push(
                "NEXC_ONTO_MODE=service needs NEXC_ONTO_URL (the address of `onto serve`)".into(),
            ),
            OntoMode::Embedded => errors.push(
                "NEXC_ONTO_MODE=embedded: this build has no embedded onto-intelli (it needs \
                 onto-intelli published as a crate; see docs/ONTOLOGY.md); use service or off"
                    .into(),
            ),
            _ => {}
        }
        if self.onto_vocabulary_ttl > Duration::from_secs(3600) {
            errors.push("NEXC_ONTO_VOCABULARY_TTL_SECS must be at most 3600".into());
        }
        if !(60..=43_200).contains(&self.onto_session_ttl.as_secs()) {
            errors.push("NEXC_ONTO_SESSION_TTL_SECS must be between 60 and 43200".into());
        }
        if !(self.runtime_grpc_url.starts_with("http://")
            || self.runtime_grpc_url.starts_with("https://"))
        {
            errors.push("NEXC_RUNTIME_GRPC_URL must start with http:// or https://".into());
        }
        if let Some(url) = &self.otlp_endpoint
            && !(url.starts_with("http://") || url.starts_with("https://"))
        {
            errors.push("NEXC_OTLP_ENDPOINT must start with http:// or https://".into());
        }
        if !(self.public_url.starts_with("http://") || self.public_url.starts_with("https://")) {
            errors.push("NEXC_PUBLIC_URL must start with http:// or https://".into());
        }
        if !(5..=10_000_000).contains(&self.api_calls_per_minute) {
            errors.push("NEXC_API_CALLS_PER_MINUTE must be between 5 and 10000000".into());
        }
        if self.cache_ttl > Duration::from_secs(3600) {
            errors.push("NEXC_CACHE_TTL_SECS must be at most 3600".into());
        }
        if self.grpc_keepalive.is_zero() || self.grpc_keepalive > Duration::from_secs(3600) {
            errors.push("NEXC_GRPC_KEEPALIVE_SECS must be between 1 and 3600".into());
        }
        if self.grpc_keepalive_timeout.is_zero()
            || self.grpc_keepalive_timeout > Duration::from_secs(600)
        {
            errors.push("NEXC_GRPC_KEEPALIVE_TIMEOUT_SECS must be between 1 and 600".into());
        }
        if !(1..=64).contains(&self.grpc_pool_size) {
            errors.push("NEXC_GRPC_POOL_SIZE must be between 1 and 64".into());
        }
        if self.llm_breaker_failures > 1000 {
            errors.push("NEXC_LLM_BREAKER_FAILURES must be at most 1000".into());
        }
        if self.llm_breaker_open.is_zero() || self.llm_breaker_open > Duration::from_secs(3600) {
            errors.push("NEXC_LLM_BREAKER_OPEN_SECS must be between 1 and 3600".into());
        }
        if !(1..=10_000).contains(&self.gateway_calls_per_minute) {
            errors.push("NEXC_GATEWAY_CALLS_PER_MINUTE must be between 1 and 10000".into());
        }
        if self.gateway_timeout.is_zero() || self.gateway_timeout > Duration::from_secs(600) {
            errors.push("NEXC_GATEWAY_TIMEOUT_SECS must be between 1 and 600".into());
        }
        if self.a2a_timeout.is_zero() || self.a2a_timeout > Duration::from_secs(600) {
            errors.push("NEXC_A2A_TIMEOUT_SECS must be between 1 and 600".into());
        }
        if !(1024..=64 * 1024 * 1024).contains(&self.gateway_max_response_bytes) {
            errors.push("NEXC_GATEWAY_MAX_RESPONSE_BYTES must be between 1 KiB and 64 MiB".into());
        }
        if self.onto_workspace_prefix.len() > 32
            || !self
                .onto_workspace_prefix
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        {
            errors.push(
                "NEXC_ONTO_WORKSPACE_PREFIX must be at most 32 letters, digits, '-', '_' or '.'"
                    .into(),
            );
        }
        if self.access_ttl.as_secs() < 60 || self.refresh_ttl <= self.access_ttl {
            errors.push("token lifetimes: access ≥ 60 s and refresh > access required".into());
        }
        if self.admin_email.is_some() != self.admin_password.is_some() {
            errors.push("NEXC_ADMIN_EMAIL and NEXC_ADMIN_PASSWORD must be set together".into());
        }
        if let Some(pw) = &self.admin_password {
            let mut fe = FieldErrors::default();
            user::check_password(&mut fe, pw.expose());
            if !fe.is_empty() {
                errors.push("NEXC_ADMIN_PASSWORD does not satisfy the password policy".into());
            }
        }
        if self.is_production()
            && self
                .cors_origins
                .iter()
                .any(|o| is_insecure_remote_origin(o))
        {
            errors.push(
                "NEXC_CORS_ORIGINS must use https in production (plain http only for localhost)"
                    .into(),
            );
        }
    }
}

struct Reader<'a> {
    lookup: &'a dyn Fn(&str) -> Option<String>,
    errors: Vec<String>,
}

impl Reader<'_> {
    fn optional(&self, key: &str) -> Option<String> {
        (self.lookup)(key)
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    }

    fn string(&self, key: &str, default: &str) -> String {
        self.optional(key).unwrap_or_else(|| default.to_owned())
    }

    fn required(&mut self, key: &str) -> Option<String> {
        let value = self.optional(key);
        if value.is_none() {
            self.errors.push(format!(
                "{key} is required (run `nexc init` to generate one)"
            ));
        }
        value
    }

    fn parse<T: std::str::FromStr>(&mut self, key: &str, default: T) -> T {
        match self.optional(key) {
            None => default,
            Some(raw) => raw.parse().unwrap_or_else(|_| {
                self.errors.push(format!("{key}: cannot parse `{raw}`"));
                default
            }),
        }
    }

    /// `NEXC_ONTO_MODE`; service when `NEXC_ONTO_URL` is set and nothing else is said.
    fn onto_mode(&mut self) -> OntoMode {
        match self.optional("NEXC_ONTO_MODE") {
            None if self.optional("NEXC_ONTO_URL").is_some() => OntoMode::Service,
            None => OntoMode::Off,
            Some(raw) => raw.to_ascii_lowercase().parse().unwrap_or_else(|_| {
                self.errors.push(format!(
                    "NEXC_ONTO_MODE: `{raw}` is not one of service, embedded, off"
                ));
                OntoMode::Off
            }),
        }
    }

    fn database_url(&mut self) -> String {
        if let Some(url) = self.optional("NEXC_DATABASE_URL") {
            if !url.starts_with("postgres://") && !url.starts_with("postgresql://") {
                self.errors
                    .push("NEXC_DATABASE_URL must be a postgres:// URL".into());
            }
            return url;
        }
        match self.optional("POSTGRES_PASSWORD") {
            Some(pw) => format!(
                "postgres://{}:{}@localhost:5432/{}",
                self.string("POSTGRES_USER", "nexc"),
                pw,
                self.string("POSTGRES_DB", "nexc")
            ),
            None => {
                self.errors
                    .push("NEXC_DATABASE_URL is required (or set POSTGRES_PASSWORD)".into());
                String::new()
            }
        }
    }

    fn jwt_secret(&mut self, prod: bool) -> Vec<u8> {
        let Some(secret) = self.required("NEXC_JWT_SECRET") else {
            return Vec::new();
        };
        if secret.len() < 32 {
            self.errors
                .push("NEXC_JWT_SECRET must be at least 32 bytes".into());
        } else if prod && is_weak(&secret) {
            self.errors
                .push("NEXC_JWT_SECRET looks weak; generate one with `nexc init`".into());
        }
        secret.into_bytes()
    }

    /// The AES-256-GCM master key. When it is missing or invalid an error is recorded (so
    /// startup fails) and a random throwaway key is returned: a predictable placeholder key
    /// never exists, even transiently.
    fn master_key(&mut self, prod: bool) -> [u8; 32] {
        let Some(raw) = self.required("NEXC_MASTER_KEY") else {
            return random_bytes::<32>();
        };
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(raw.as_bytes())
            .ok();
        match decoded.and_then(|bytes| <[u8; 32]>::try_from(bytes.as_slice()).ok()) {
            Some(key) => {
                if prod && key.iter().collect::<HashSet<_>>().len() < 16 {
                    self.errors
                        .push("NEXC_MASTER_KEY looks weak; generate one with `nexc init`".into());
                }
                key
            }
            None => {
                self.errors
                    .push("NEXC_MASTER_KEY must be base64 of exactly 32 bytes".into());
                random_bytes::<32>()
            }
        }
    }

    fn runtime_token(&mut self, prod: bool) -> String {
        let Some(token) = self.required("NEXC_RUNTIME_TOKEN") else {
            return String::new();
        };
        if token.chars().count() < 32 {
            self.errors
                .push("NEXC_RUNTIME_TOKEN must be at least 32 characters".into());
        } else if prod && is_weak(&token) {
            self.errors
                .push("NEXC_RUNTIME_TOKEN looks weak; generate one with `nexc init`".into());
        }
        token
    }

    /// The agent token key: `NEXC_AGENT_TOKEN_KEY` when set (≥ 32 characters),
    /// else derived from `NEXC_RUNTIME_TOKEN`, which the runtime also holds.
    fn agent_token_key(&mut self, prod: bool) -> Vec<u8> {
        match self.optional("NEXC_AGENT_TOKEN_KEY") {
            Some(key) => {
                if key.chars().count() < 32 {
                    self.errors
                        .push("NEXC_AGENT_TOKEN_KEY must be at least 32 characters".into());
                } else if prod && is_weak(&key) {
                    self.errors.push(
                        "NEXC_AGENT_TOKEN_KEY looks weak; generate one with `openssl rand -hex 32`"
                            .into(),
                    );
                }
                key.into_bytes()
            }
            None => {
                let runtime_token = (self.lookup)("NEXC_RUNTIME_TOKEN").unwrap_or_default();
                crate::security::agent_token::derive_key(&runtime_token).to_vec()
            }
        }
    }

    /// Where the Agent Firewall API listens, when it is on.
    fn grpc_addr(&mut self) -> Option<SocketAddr> {
        let raw = self.optional("NEXC_GRPC_ADDR")?;
        match raw.parse() {
            Ok(addr) => Some(addr),
            Err(_) => {
                self.errors.push(format!(
                    "NEXC_GRPC_ADDR: `{raw}` is not a host:port address"
                ));
                None
            }
        }
    }

    /// The firewall's Ed25519 seed: `NEXC_FIREWALL_SIGNING_KEY` (base64 of 32
    /// bytes) when set, else derived from the master key, which only the
    /// backend holds. The master key itself never signs anything.
    fn firewall_signing_key(&mut self) -> [u8; 32] {
        match self.optional("NEXC_FIREWALL_SIGNING_KEY") {
            Some(raw) => {
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(raw.as_bytes())
                    .ok()
                    .and_then(|bytes| <[u8; 32]>::try_from(bytes.as_slice()).ok());
                match decoded {
                    Some(seed) => seed,
                    None => {
                        self.errors.push(
                            "NEXC_FIREWALL_SIGNING_KEY must be base64 of exactly 32 bytes".into(),
                        );
                        random_bytes::<32>()
                    }
                }
            }
            None => {
                let master = (self.lookup)("NEXC_MASTER_KEY").unwrap_or_default();
                crate::security::signing::derive_seed(master.as_bytes())
            }
        }
    }

    fn cors_origins(&mut self) -> Vec<String> {
        let origins: Vec<String> = self
            .string("NEXC_CORS_ORIGINS", "http://localhost:5173")
            .split(',')
            .map(|o| o.trim().trim_end_matches('/').to_owned())
            .filter(|o| !o.is_empty())
            .collect();
        for o in &origins {
            let ok = (o.starts_with("http://") || o.starts_with("https://")) && !o.contains('*');
            if !ok {
                self.errors.push(format!(
                    "NEXC_CORS_ORIGINS: `{o}` must be an explicit http(s) origin"
                ));
            }
        }
        origins
    }
}

/// The shortest run lease production accepts (docs/HA.md): renewals every
/// third of it must get through, so a shorter one turns a slow database into
/// takeovers.
const MIN_PRODUCTION_LEASE: Duration = Duration::from_secs(10);

/// An `http://` origin that is not the local machine.
fn is_insecure_remote_origin(origin: &str) -> bool {
    reqwest::Url::parse(origin).is_ok_and(|url| {
        url.scheme() == "http"
            && !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
    })
}

/// Heuristic for secrets that are obviously not random.
fn is_weak(secret: &str) -> bool {
    let lower = secret.to_lowercase();
    let distinct = secret.chars().collect::<HashSet<_>>().len();
    distinct < 16
        || ["change", "secret", "example", "password", "replace"]
            .iter()
            .any(|w| lower.contains(w))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn base() -> HashMap<&'static str, String> {
        HashMap::from([
            (
                "NEXC_DATABASE_URL",
                "postgres://nexc:pw@localhost:5432/nexc".to_owned(),
            ),
            (
                "NEXC_JWT_SECRET",
                "q8Xv2LmZ7pR4tW9yB3nC6kD1fH5jS0aE".to_owned(),
            ),
            (
                "NEXC_MASTER_KEY",
                base64::engine::general_purpose::STANDARD.encode((0u8..32).collect::<Vec<_>>()),
            ),
            (
                "NEXC_RUNTIME_TOKEN",
                "Zx9Qw8Er7Ty6Ui5Op4As3Df2Gh1Jk0Lm".to_owned(),
            ),
        ])
    }

    fn load(map: &HashMap<&'static str, String>) -> Result<Settings, ConfigError> {
        Settings::from_lookup(|k| map.get(k).cloned())
    }

    #[test]
    fn loads_defaults() {
        let s = load(&base()).unwrap();
        assert_eq!(s.port, 8080);
        assert_eq!(s.max_concurrency, 4);
        assert_eq!(s.llm_provider, LlmProviderKind::Anthropic);
        assert_eq!(s.cors_origins, vec!["http://localhost:5173"]);
        assert!(!s.cookie_secure);
        assert!(
            !format!("{s:?}").contains("q8Xv2"),
            "secrets are redacted in Debug"
        );
    }

    #[test]
    fn reports_every_problem() {
        let mut m = base();
        m.insert("NEXC_JWT_SECRET", "short".into());
        m.insert("NEXC_MASTER_KEY", "not-base64".into());
        m.insert("NEXC_PORT", "http".into());
        m.insert("NEXC_CORS_ORIGINS", "*".into());
        let err = load(&m).unwrap_err();
        assert_eq!(err.0.len(), 4, "{err}");
    }

    #[test]
    fn production_refuses_weak_secrets() {
        let mut m = base();
        m.insert("NEXC_ENV", "production".into());
        m.insert(
            "NEXC_JWT_SECRET",
            "change-me-change-me-change-me-change-me".into(),
        );
        m.insert(
            "NEXC_CORS_ORIGINS",
            "https://nexc.example.org,http://localhost:8080".into(),
        );
        m.insert("NEXC_COOKIE_SECURE", "false".into());
        let err = load(&m).unwrap_err();
        assert_eq!(
            err.0,
            vec!["NEXC_JWT_SECRET looks weak; generate one with `nexc init`".to_owned()]
        );
        m.insert("NEXC_JWT_SECRET", "q8Xv2LmZ7pR4tW9yB3nC6kD1fH5jS0aE".into());
        let s = load(&m).unwrap();
        assert!(
            s.cookie_secure && s.is_production(),
            "production forces Secure cookies"
        );
        m.insert("NEXC_CORS_ORIGINS", "http://nexc.example.org".into());
        assert!(
            load(&m).is_err(),
            "remote plain-http origins are refused in production"
        );
    }

    #[test]
    fn production_refuses_a_lease_too_short_to_renew_in_time() {
        let mut m = base();
        m.insert("NEXC_RUN_LEASE_SECS", "2".into());
        assert_eq!(load(&m).unwrap().run_lease, Duration::from_secs(2));
        m.insert("NEXC_ENV", "production".into());
        m.insert("NEXC_CORS_ORIGINS", "https://nexc.example.org".into());
        let err = load(&m).unwrap_err();
        assert_eq!(err.0.len(), 1, "{err}");
        assert!(err.0[0].starts_with("NEXC_RUN_LEASE_SECS must be at least 10"));
        m.insert("NEXC_RUN_LEASE_SECS", "10".into());
        assert!(load(&m).is_ok());
    }

    #[test]
    fn database_url_from_postgres_password() {
        let mut m = base();
        m.remove("NEXC_DATABASE_URL");
        m.insert("POSTGRES_PASSWORD", "pw123".into());
        let s = load(&m).unwrap();
        assert_eq!(
            s.database_url.expose(),
            "postgres://nexc:pw123@localhost:5432/nexc"
        );
    }
}
