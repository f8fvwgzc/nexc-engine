//! Typed, validated settings loaded from the environment (and `.env`).
#![forbid(unsafe_code)]

mod secret;
pub mod vars;

use std::collections::HashSet;
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;

use base64::Engine as _;

pub use secret::Secret;

use crate::domain::settings::LlmProviderKind;
use crate::domain::user;
use crate::domain::validation::FieldErrors;

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
    /// Claude Code CLI binary used by the `claude_code` provider.
    pub claude_bin: String,
    pub runtime_url: String,
    pub runtime_token: Secret<String>,
    pub symphony_enabled: bool,
    pub symphony_url: String,
    pub symphony_workflow: PathBuf,
    pub max_concurrency: usize,
    pub max_attempts: u32,
    pub node_timeout: Duration,
    pub trust_proxy: bool,
    pub metrics_token: Option<Secret<String>>,
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
        let settings = Settings {
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
            claude_bin: r.string("NEXC_CLAUDE_BIN", "claude"),
            runtime_url: r.string("NEXC_RUNTIME_URL", "http://localhost:8090"),
            runtime_token: Secret::new(r.runtime_token(prod)),
            symphony_enabled: r.parse("NEXC_SYMPHONY_ENABLED", false),
            symphony_url: r.string("NEXC_SYMPHONY_URL", "http://localhost:4000"),
            symphony_workflow: PathBuf::from(
                r.string("NEXC_SYMPHONY_WORKFLOW", "./data/symphony/WORKFLOW.md"),
            ),
            max_concurrency: r.parse("NEXC_MAX_CONCURRENCY", 4),
            max_attempts: r.parse("NEXC_MAX_ATTEMPTS", 3),
            node_timeout: Duration::from_secs(r.parse("NEXC_NODE_TIMEOUT_SECS", 600)),
            trust_proxy: r.parse("NEXC_TRUST_PROXY", false),
            metrics_token: r.optional("NEXC_METRICS_TOKEN").map(Secret::new),
        };
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

    /// Directory holding run artifacts.
    pub fn artifacts_dir(&self) -> PathBuf {
        self.data_dir.join("artifacts")
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
        if !(1..=1000).contains(&self.db_max_connections) {
            errors.push("NEXC_DB_MAX_CONNECTIONS must be between 1 and 1000".into());
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

    fn master_key(&mut self, prod: bool) -> [u8; 32] {
        let Some(raw) = self.required("NEXC_MASTER_KEY") else {
            return [0; 32];
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
                [0; 32]
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
