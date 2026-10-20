//! Shared application state handed to every handler and engine task.

use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;

use tonic::transport::Channel;

use super::caches::Caches;
use crate::config::{RuntimeProtocol, Settings};
use crate::domain::ontology::OntoMode;
use crate::engine::Engine;
use crate::engine::executor::symphony::SymphonyBridge;
use crate::http::middleware::rate_limit::RateLimiters;
use crate::llm::LlmProvider;
use crate::llm::breaker::CircuitBreaker;
use crate::llm::claude_code::ClaudeCodeProvider;
use crate::llm::router::ProviderRouter;
use crate::llm::service::LlmService;
use crate::memory::index::MemoryIndex;
use crate::observability::metrics::Metrics;
use crate::ontology::OntologyBackend;
use crate::ontology::grpc::GrpcOntology;
use crate::orchestrator::health::HealthCache;
use crate::realtime::hub::Hub;
use crate::security::agent_token::AgentTokenKeys;
use crate::security::gate::SessionGate;
use crate::security::jwt::JwtKeys;
use crate::security::oidc::OidcClient;
use crate::security::secret_box::SecretBox;
use crate::security::signing::{Keyring, Signer};

/// Cheap to clone: every field is reference counted.
#[derive(Clone)]
pub struct AppState {
    pub settings: Arc<Settings>,
    pub db: PgPool,
    pub http: reqwest::Client,
    pub jwt: Arc<JwtKeys>,
    /// Signs the per-run tokens the agent runtime verifies.
    pub agent_tokens: Arc<AgentTokenKeys>,
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
    /// The onto-intelli that holds the workspaces' ontology: graph
    /// vocabularies, the projected workspace, the audit's facts, schema
    /// proposals and the UI's sessions. `None` when `NEXC_ONTO_MODE=off`:
    /// graphs use the read-only starter vocabulary and the rest is off.
    pub ontology: Option<Arc<dyn OntologyBackend>>,
    /// The channel to the runtime's gRPC endpoint (`NEXC_RUNTIME_PROTOCOL=grpc`).
    pub runtime_grpc: Option<Channel>,
    /// The channel to the OTLP collector (`NEXC_OTLP_ENDPOINT`), kept for the life of the process.
    pub otlp_channel: Option<Channel>,
    /// The tools gateway's outbound HTTP client (no redirects of its own).
    pub gateway_http: reqwest::Client,
    /// The MCP server registrations the gateway read lately.
    pub gateway_registry: Arc<crate::gateway::Registry>,
    /// Signs the decisions the Agent Firewall API hands out (the current key of `signing_keys`).
    pub firewall_signer: Arc<Signer>,
    /// The key that signs run seals, firewall decisions and evidence bundles, and the
    /// retired keys published beside it (`/.well-known/jwks.json`).
    pub signing_keys: Arc<Keyring>,
    /// The external agents' decisions sessions open on this instance.
    pub firewall_sessions: Arc<crate::firewall::Sessions>,
    /// Hot reads kept in memory, dropped on write across replicas.
    pub caches: Arc<Caches>,
    /// The single sign-on provider's relying party; `None` when `NEXC_OIDC_ISSUER` is unset.
    pub oidc: Option<Arc<OidcClient>>,
}

impl AppState {
    /// Builds the state with the production LLM provider router.
    pub fn new(settings: Settings, db: PgPool) -> anyhow::Result<Self> {
        let http = http_client()?;
        let claude_code = ClaudeCodeProvider::new(
            settings.claude_bin.clone(),
            settings.data_dir.join("claude-code"),
        );
        let metrics = Arc::new(Metrics::default());
        let router = Arc::new(
            ProviderRouter::new(http.clone(), settings.llm_fallbacks, claude_code).with_breaker(
                CircuitBreaker::new(
                    settings.llm_breaker_failures,
                    settings.llm_breaker_open,
                    metrics.clone(),
                ),
            ),
        );
        let ontology: Option<Arc<dyn OntologyBackend>> = match &settings.onto_url {
            Some(url) if settings.onto_mode == OntoMode::Service => {
                let token = settings.onto_token.as_ref().map(|t| t.expose().as_str());
                Some(Arc::new(GrpcOntology::with_channel(
                    url,
                    token,
                    &settings.grpc_channel(),
                    metrics.clone(),
                )?))
            }
            _ => None,
        };
        let state = Self::with_provider_and_metrics(settings, db, http, router, metrics)?;
        Ok(match ontology {
            Some(backend) => state.with_ontology(backend),
            None => state,
        })
    }

    /// The state with `backend` as its onto-intelli (tests inject the stub here).
    pub fn with_ontology(mut self, backend: Arc<dyn OntologyBackend>) -> Self {
        self.ontology = Some(backend);
        self
    }

    /// How this instance reaches onto-intelli: service whenever a backend is
    /// configured (a stub in tests included), off otherwise.
    pub fn onto_mode(&self) -> OntoMode {
        match &self.ontology {
            Some(_) => OntoMode::Service,
            None => OntoMode::Off,
        }
    }

    /// Builds the state with a custom LLM provider (tests inject fakes here).
    pub fn with_provider(
        settings: Settings,
        db: PgPool,
        http: reqwest::Client,
        provider: Arc<dyn LlmProvider>,
    ) -> anyhow::Result<Self> {
        Self::with_provider_and_metrics(settings, db, http, provider, Arc::new(Metrics::default()))
    }

    fn with_provider_and_metrics(
        settings: Settings,
        db: PgPool,
        http: reqwest::Client,
        provider: Arc<dyn LlmProvider>,
        metrics: Arc<Metrics>,
    ) -> anyhow::Result<Self> {
        let hub = Arc::new(Hub::default());
        let channel = settings.grpc_channel();
        let runtime_grpc = match settings.runtime_protocol {
            RuntimeProtocol::Grpc => Some(crate::engine::executor::runtime_grpc::channel_with(
                &settings.runtime_grpc_url,
                &channel,
            )?),
            RuntimeProtocol::Ndjson => None,
        };
        let otlp_channel = match &settings.otlp_endpoint {
            Some(endpoint) => Some(crate::otlp::channel_with(endpoint, &channel)?),
            None => None,
        };
        let firewall_signer = Arc::new(Signer::from_seed(settings.firewall_signing_key.expose())?);
        let signing_keys = Arc::new(Keyring::new(
            firewall_signer.clone(),
            settings.signing_keys_retired.clone(),
        ));
        if settings.signing_key_derived && settings.is_production() {
            tracing::warn!(kid = %firewall_signer.kid(),
                "the signing key is derived from NEXC_MASTER_KEY; set NEXC_SIGNING_KEY so that seals and decisions do not depend on the key that encrypts secrets");
        }
        let oidc = match &settings.oidc {
            Some(oidc) => Some(Arc::new(OidcClient::new(
                oidc.clone(),
                settings.is_production(),
            )?)),
            None => None,
        };
        Ok(AppState {
            oidc,
            caches: Arc::new(
                Caches::new(settings.cache_ttl, metrics.clone())
                    .with_vocabulary_ttl(settings.onto_vocabulary_ttl),
            ),
            runtime_grpc,
            otlp_channel,
            gateway_http: crate::gateway::http_client()?,
            gateway_registry: Arc::default(),
            firewall_signer,
            signing_keys,
            firewall_sessions: Arc::default(),
            jwt: Arc::new(JwtKeys::new(
                settings.jwt_secret.expose(),
                settings.access_ttl,
            )),
            agent_tokens: Arc::new(AgentTokenKeys::new(settings.agent_token_key.expose())),
            secret_box: Arc::new(SecretBox::new(settings.master_key.expose())),
            llm: Arc::new(LlmService::new(provider, metrics.clone())),
            limiters: Arc::new(RateLimiters::new(
                settings.gateway_calls_per_minute,
                settings.api_calls_per_minute,
            )),
            sessions: Arc::default(),
            engine: Arc::new(Engine::new(hub.instance())),
            symphony: Arc::new(SymphonyBridge::new(&settings)),
            health: Arc::new(HealthCache::default()),
            memories: Arc::new(MemoryIndex::default()),
            passage_vectors: Arc::default(),
            ontology: None,
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
