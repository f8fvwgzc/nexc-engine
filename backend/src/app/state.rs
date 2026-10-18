//! Shared application state handed to every handler and engine task.

use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;

use tonic::transport::Channel;

use crate::config::{RuntimeProtocol, Settings};
use crate::engine::Engine;
use crate::engine::executor::symphony::SymphonyBridge;
use crate::http::middleware::rate_limit::RateLimiters;
use crate::llm::LlmProvider;
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
use crate::security::secret_box::SecretBox;
use crate::security::signing::Signer;

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
    /// The onto-intelli the audit's facts go to and questions come from;
    /// `None` turns "ask the audit" off (`NEXC_ONTO_URL` unset).
    pub ontology: Option<Arc<dyn OntologyBackend>>,
    /// The channel to the runtime's gRPC endpoint (`NEXC_RUNTIME_PROTOCOL=grpc`).
    pub runtime_grpc: Option<Channel>,
    /// The tools gateway's outbound HTTP client (no redirects of its own).
    pub gateway_http: reqwest::Client,
    /// Signs the decisions the Agent Firewall API hands out.
    pub firewall_signer: Arc<Signer>,
    /// The external agents' decisions sessions open on this instance.
    pub firewall_sessions: Arc<crate::firewall::Sessions>,
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
        let ontology: Option<Arc<dyn OntologyBackend>> = match &settings.onto_url {
            Some(url) => {
                let token = settings.onto_token.as_ref().map(|t| t.expose().as_str());
                Some(Arc::new(GrpcOntology::new(url, token)?))
            }
            None => None,
        };
        let state = Self::with_provider(settings, db, http, router)?;
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

    /// Builds the state with a custom LLM provider (tests inject fakes here).
    pub fn with_provider(
        settings: Settings,
        db: PgPool,
        http: reqwest::Client,
        provider: Arc<dyn LlmProvider>,
    ) -> anyhow::Result<Self> {
        let metrics = Arc::new(Metrics::default());
        let hub = Arc::new(Hub::default());
        let runtime_grpc = match settings.runtime_protocol {
            RuntimeProtocol::Grpc => Some(crate::engine::executor::runtime_grpc::channel(
                &settings.runtime_grpc_url,
            )?),
            RuntimeProtocol::Ndjson => None,
        };
        let firewall_signer = Arc::new(Signer::from_seed(settings.firewall_signing_key.expose())?);
        Ok(AppState {
            runtime_grpc,
            gateway_http: crate::gateway::http_client()?,
            firewall_signer,
            firewall_sessions: Arc::default(),
            jwt: Arc::new(JwtKeys::new(
                settings.jwt_secret.expose(),
                settings.access_ttl,
            )),
            agent_tokens: Arc::new(AgentTokenKeys::new(settings.agent_token_key.expose())),
            secret_box: Arc::new(SecretBox::new(settings.master_key.expose())),
            llm: Arc::new(LlmService::new(provider, metrics.clone())),
            limiters: Arc::new(RateLimiters::new(settings.gateway_calls_per_minute)),
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
