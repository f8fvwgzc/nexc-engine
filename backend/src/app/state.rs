//! Shared application state handed to every handler and engine task.

use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;

use crate::config::Settings;
use crate::engine::Engine;
use crate::engine::executor::symphony::SymphonyBridge;
use crate::http::middleware::rate_limit::RateLimiters;
use crate::llm::LlmProvider;
use crate::llm::claude_code::ClaudeCodeProvider;
use crate::llm::router::ProviderRouter;
use crate::llm::service::LlmService;
use crate::memory::index::MemoryIndex;
use crate::observability::metrics::Metrics;
use crate::orchestrator::health::HealthCache;
use crate::realtime::hub::Hub;
use crate::security::gate::SessionGate;
use crate::security::jwt::JwtKeys;
use crate::security::secret_box::SecretBox;

/// Cheap to clone: every field is reference counted.
#[derive(Clone)]
pub struct AppState {
    pub settings: Arc<Settings>,
    pub db: PgPool,
    pub http: reqwest::Client,
    pub jwt: Arc<JwtKeys>,
    pub secret_box: Arc<SecretBox>,
    pub hub: Arc<Hub>,
    pub llm: Arc<LlmService>,
    pub metrics: Arc<Metrics>,
    pub limiters: Arc<RateLimiters>,
    /// Accounts whose access tokens were ended early (suspension, platform role change).
    pub sessions: Arc<SessionGate>,
    pub engine: Arc<Engine>,
    pub symphony: Arc<SymphonyBridge>,
    pub health: Arc<HealthCache>,
    /// Decoded memories per owner, so retrieval does not hit the database every time.
    pub memories: Arc<MemoryIndex>,
    /// Whether the database can search document passages by vector.
    pub passage_vectors: Arc<crate::repo::knowledge_vectors::Support>,
}

impl AppState {
    /// Builds the state with the production LLM provider router.
    pub fn new(settings: Settings, db: PgPool) -> anyhow::Result<Self> {
        let http = http_client()?;
        let claude_code = ClaudeCodeProvider::new(
            settings.claude_bin.clone(),
            settings.data_dir.join("claude-code"),
        );
        let router = Arc::new(ProviderRouter::new(
            http.clone(),
            settings.llm_fallbacks,
            claude_code,
        ));
        Self::with_provider(settings, db, http, router)
    }

    /// Builds the state with a custom LLM provider (tests inject fakes here).
    pub fn with_provider(
        settings: Settings,
        db: PgPool,
        http: reqwest::Client,
        provider: Arc<dyn LlmProvider>,
    ) -> anyhow::Result<Self> {
        let metrics = Arc::new(Metrics::default());
        let hub = Arc::new(Hub::default());
        Ok(AppState {
            jwt: Arc::new(JwtKeys::new(
                settings.jwt_secret.expose(),
                settings.access_ttl,
            )),
            secret_box: Arc::new(SecretBox::new(settings.master_key.expose())),
            llm: Arc::new(LlmService::new(provider, metrics.clone())),
            limiters: Arc::new(RateLimiters::default()),
            sessions: Arc::default(),
            engine: Arc::new(Engine::new(hub.instance())),
            symphony: Arc::new(SymphonyBridge::new(&settings)),
            health: Arc::new(HealthCache::default()),
            memories: Arc::new(MemoryIndex::default()),
            passage_vectors: Arc::default(),
            hub,
            metrics,
            http,
            db,
            settings: Arc::new(settings),
        })
    }
}

/// The shared outbound HTTP client (rustls, connection pooling).
pub fn http_client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(300))
        .user_agent(concat!("nexc-engine/", env!("CARGO_PKG_VERSION")))
        .build()?)
}
