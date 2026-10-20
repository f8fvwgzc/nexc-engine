//! The tonic client of a real onto-intelli: `authorization: Bearer`,
//! `x-onto-tenant` and `x-onto-workspace` on every request, the
//! impersonation headers when the call is made for someone, the server's
//! status codes mapped to [`OntoError`], and the proto messages mapped to the
//! backend's types ([`convert`], shared with the stub server). Browser
//! sessions are minted over the same port's HTTP API.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use serde_json::{Value, json};
use tonic::metadata::{AsciiMetadataValue, MetadataMap, MetadataValue};
use tonic::transport::Channel;
use tonic::{Code, Request, Status};

use crate::domain::ontology::Ontology;
use crate::observability::metrics::Metrics;
use crate::rpc::ChannelSettings;

use super::pb::admin_service_client::AdminServiceClient;
use super::pb::event_service_client::EventServiceClient;
use super::pb::execution_service_client::ExecutionServiceClient;
use super::pb::intelli_service_client::IntelliServiceClient;
use super::pb::ontology_service_client::OntologyServiceClient;
use super::pb::schema_service_client::SchemaServiceClient;
use super::{
    AskAnswer, AskRequest, AskStream, AskStreamEvent, Entity, EventQuery, FindQuery, IngestOutcome,
    OntoError, OntoEvent, OntoHealth, OntoSession, OntologyBackend, Principal, SchemaChange,
    SchemaProposalDraft, Subgraph, TraverseQuery, json, pb,
};

/// Opening the TCP connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// One question, including the model's time when it is used.
const ASK_TIMEOUT: Duration = Duration::from_secs(60);
/// A graph query, a vocabulary or a schema read; deterministic, so quick.
const QUERY_TIMEOUT: Duration = Duration::from_secs(10);
/// A write that opens a tenant the first time (its tables are created then).
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
/// One ingest stream.
const INGEST_TIMEOUT: Duration = Duration::from_secs(60);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);

/// The metadata names onto-intelli reads.
pub mod header {
    pub const TENANT: &str = "x-onto-tenant";
    pub const WORKSPACE: &str = "x-onto-workspace";
    pub const ACT_AS: &str = "x-onto-act-as";
    pub const ACT_ROLES: &str = "x-onto-act-roles";
    pub const ACT_WORKSPACES: &str = "x-onto-act-workspaces";
    pub const ACT_ENTITY: &str = "x-onto-act-entity";
}

/// A real onto-intelli at `url`.
#[derive(Clone)]
pub struct GrpcOntology {
    url: String,
    channel: Channel,
    token: Option<MetadataValue<tonic::metadata::Ascii>>,
    /// The bearer token itself, for the HTTP API (sessions).
    bearer: Option<String>,
    http: reqwest::Client,
    metrics: Arc<Metrics>,
}

impl GrpcOntology {
    /// Connects lazily with the default keepalive and pool: the first request
    /// opens the connection, and a server that is down is a failed request,
    /// never a failed start.
    pub fn new(url: &str, token: Option<&str>) -> Result<Self, OntoError> {
        Self::with_channel(
            url,
            token,
            &ChannelSettings::default(),
            Arc::new(Metrics::default()),
        )
    }

    /// Connects lazily, the channel kept as `settings` say (`NEXC_GRPC_*`),
    /// every call timed into `metrics` (`nexc_grpc_client_latency_seconds`).
    pub fn with_channel(
        url: &str,
        token: Option<&str>,
        settings: &ChannelSettings,
        metrics: Arc<Metrics>,
    ) -> Result<Self, OntoError> {
        let settings = settings.connect_timeout(CONNECT_TIMEOUT);
        let channel = crate::rpc::channel(url, &settings)
            .map_err(|err| OntoError::Invalid(format!("NEXC_ONTO_URL: {err}")))?;
        let bearer = token.map(str::to_owned);
        let token = match token {
            Some(token) => Some(
                format!("Bearer {token}")
                    .parse()
                    .map_err(|_| OntoError::Invalid("NEXC_ONTO_TOKEN is not ASCII".into()))?,
            ),
            None => None,
        };
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(QUERY_TIMEOUT)
            .user_agent(concat!("nexc-engine/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|err| OntoError::Invalid(format!("HTTP client: {err}")))?;
        Ok(GrpcOntology {
            url: url.trim_end_matches('/').to_owned(),
            channel,
            token,
            bearer,
            http,
            metrics,
        })
    }

    /// Times one call of `service`/`method`.
    fn timed<T>(&self, service: &str, method: &str, started: Instant, result: T) -> T {
        self.metrics.grpc_client(service, method, started.elapsed());
        result
    }

    fn request<T>(
        &self,
        message: T,
        principal: Option<&Principal>,
        timeout: Duration,
    ) -> Request<T> {
        let mut request = Request::new(message);
        if let Some(token) = &self.token {
            request
                .metadata_mut()
                .insert("authorization", token.clone());
        }
        if let Some(principal) = principal {
            principal_metadata(request.metadata_mut(), principal);
        }
        request.set_timeout(timeout);
        request
    }
}

fn ascii(value: &str) -> Option<AsciiMetadataValue> {
    value.parse().ok()
}

/// The tenant, the workspace and, when the call is made for someone, who:
/// what onto-intelli authorizes and audits the call by.
pub fn principal_metadata(metadata: &mut MetadataMap, principal: &Principal) {
    let mut put = |name: &'static str, value: &str| {
        if !value.is_empty()
            && let Some(value) = ascii(value)
        {
            metadata.insert(name, value);
        }
    };
    put(header::TENANT, &principal.tenant);
    put(header::WORKSPACE, &principal.workspace);
    if principal.acts() {
        put(header::ACT_AS, &principal.id);
        put(header::ACT_ROLES, &principal.roles.join(","));
        put(header::ACT_WORKSPACES, &principal.workspace);
        if let Some(entity) = &principal.entity {
            put(header::ACT_ENTITY, entity);
        }
    }
}

/// The principal the metadata names (the stub server's side of [`principal_metadata`]).
pub fn principal_of(metadata: &MetadataMap) -> Principal {
    let get = |name: &str| {
        metadata
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .unwrap_or_default()
            .to_owned()
    };
    let id = get(header::ACT_AS);
    let roles = get(header::ACT_ROLES)
        .split(',')
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(str::to_owned)
        .collect();
    let entity = Some(get(header::ACT_ENTITY)).filter(|e| !e.is_empty());
    let workspace = Some(get(header::WORKSPACE))
        .filter(|w| !w.is_empty())
        .unwrap_or_else(|| get(header::ACT_WORKSPACES));
    Principal {
        id,
        roles,
        entity,
        tenant: get(header::TENANT),
        workspace,
    }
}

/// The error a gRPC status stands for.
pub fn error_of(status: &Status) -> OntoError {
    let message = status.message().to_owned();
    match status.code() {
        // `Unknown` is what tonic reports for a transport failure.
        Code::Unavailable | Code::DeadlineExceeded | Code::Cancelled | Code::Unknown => {
            OntoError::Unavailable(if message.is_empty() {
                status.code().description().to_owned()
            } else {
                message
            })
        }
        Code::PermissionDenied | Code::Unauthenticated => OntoError::Denied(message),
        Code::NotFound => OntoError::NotFound(message),
        Code::InvalidArgument | Code::FailedPrecondition | Code::AlreadyExists => {
            OntoError::Invalid(message)
        }
        _ => OntoError::Other(format!("{}: {message}", status.code().description())),
    }
}

/// The gRPC status an error stands for (the stub server's side of [`error_of`]).
pub fn status_of(error: &OntoError) -> Status {
    match error {
        OntoError::Unavailable(m) => Status::unavailable(m.clone()),
        OntoError::Denied(m) => Status::permission_denied(m.clone()),
        OntoError::NotFound(m) => Status::not_found(m.clone()),
        OntoError::Invalid(m) => Status::invalid_argument(m.clone()),
        OntoError::Other(m) => Status::internal(m.clone()),
    }
}

/// The error an HTTP answer of onto-intelli stands for.
fn http_error(status: reqwest::StatusCode, body: &Value) -> OntoError {
    let message = body["error"]["message"]
        .as_str()
        .or_else(|| body["error"].as_str())
        .unwrap_or_else(|| status.canonical_reason().unwrap_or("error"))
        .to_owned();
    match status.as_u16() {
        401 | 403 => OntoError::Denied(message),
        404 => OntoError::NotFound(message),
        400 | 409 | 422 => OntoError::Invalid(message),
        502..=504 => OntoError::Unavailable(message),
        _ => OntoError::Other(format!("{status}: {message}")),
    }
}

#[async_trait::async_trait]
impl OntologyBackend for GrpcOntology {
    fn location(&self) -> String {
        self.url.clone()
    }

    async fn health(&self) -> Result<OntoHealth, OntoError> {
        let mut client = AdminServiceClient::new(self.channel.clone());
        let started = Instant::now();
        let reply = client.health(self.request((), None, HEALTH_TIMEOUT)).await;
        let reply = self
            .timed("AdminService", "Health", started, reply)
            .map_err(|s| error_of(&s))?
            .into_inner();
        Ok(OntoHealth {
            status: reply.status,
            version: reply.version,
            revision: reply.revision,
            llm: reply.llm,
            llm_available: reply.llm_available,
        })
    }

    /// One `Ingest` stream per batch, applied atomically by the server.
    /// Records carry no `ts`: the stream's `google.protobuf.Struct` would
    /// turn it into a double, which the server refuses as a timestamp, and
    /// its `Append` only reads back events newer than the last 64. Events
    /// are stamped by the server when they arrive and carry the moment they
    /// happened in `payload.at` (see `facts`).
    async fn ingest(
        &self,
        principal: &Principal,
        records: Vec<Value>,
    ) -> Result<IngestOutcome, OntoError> {
        let mut client = EventServiceClient::new(self.channel.clone());
        let messages: Vec<prost_types::Struct> = records.iter().map(json::to_struct).collect();
        let stream = futures::stream::iter(messages);
        let started = Instant::now();
        let reply = client
            .ingest(self.request(stream, Some(principal), INGEST_TIMEOUT))
            .await;
        let reply = self
            .timed("EventService", "Ingest", started, reply)
            .map_err(|s| error_of(&s))?
            .into_inner();
        Ok(IngestOutcome {
            accepted: reply.accepted,
            queued: reply.queued,
        })
    }

    async fn ask(&self, principal: &Principal, req: AskRequest) -> Result<AskAnswer, OntoError> {
        let mut client = IntelliServiceClient::new(self.channel.clone());
        let message = convert::ask_request_to(&req, &principal.workspace);
        let started = Instant::now();
        let reply = client
            .ask(self.request(message, Some(principal), ASK_TIMEOUT))
            .await;
        let reply = self
            .timed("IntelliService", "Ask", started, reply)
            .map_err(|s| error_of(&s))?
            .into_inner();
        Ok(convert::ask_answer_from(reply))
    }

    async fn ask_stream(
        &self,
        principal: &Principal,
        req: AskRequest,
    ) -> Result<AskStream, OntoError> {
        let mut client = IntelliServiceClient::new(self.channel.clone());
        let message = convert::ask_request_to(&req, &principal.workspace);
        let started = Instant::now();
        let stream = client
            .ask_stream(self.request(message, Some(principal), ASK_TIMEOUT))
            .await;
        // The stream's opening; its events follow at the model's pace.
        let stream = self
            .timed("IntelliService", "AskStream", started, stream)
            .map_err(|s| error_of(&s))?
            .into_inner();
        let events = stream.map(|item| match item {
            Ok(event) => convert::ask_event_from(event),
            Err(status) => AskStreamEvent::Error(error_of(&status).to_string()),
        });
        Ok(Box::pin(events))
    }

    async fn find(&self, principal: &Principal, q: FindQuery) -> Result<Vec<Entity>, OntoError> {
        let mut client = OntologyServiceClient::new(self.channel.clone());
        let message = convert::find_query_to(&q, &principal.workspace);
        let started = Instant::now();
        let reply = client
            .find_entities(self.request(message, Some(principal), QUERY_TIMEOUT))
            .await;
        let reply = self
            .timed("OntologyService", "FindEntities", started, reply)
            .map_err(|s| error_of(&s))?
            .into_inner();
        Ok(reply.entities.iter().map(convert::entity_from).collect())
    }

    async fn traverse(
        &self,
        principal: &Principal,
        q: TraverseQuery,
    ) -> Result<Subgraph, OntoError> {
        let mut client = OntologyServiceClient::new(self.channel.clone());
        let message = convert::traverse_query_to(&q);
        let started = Instant::now();
        let reply = client
            .traverse(self.request(message, Some(principal), QUERY_TIMEOUT))
            .await;
        let reply = self
            .timed("OntologyService", "Traverse", started, reply)
            .map_err(|s| error_of(&s))?
            .into_inner();
        Ok(convert::subgraph_from(reply))
    }

    async fn events(
        &self,
        principal: &Principal,
        q: EventQuery,
    ) -> Result<Vec<OntoEvent>, OntoError> {
        let mut client = EventServiceClient::new(self.channel.clone());
        let message = convert::event_query_to(&q, &principal.workspace);
        let started = Instant::now();
        let reply = client
            .list(self.request(message, Some(principal), QUERY_TIMEOUT))
            .await;
        let reply = self
            .timed("EventService", "List", started, reply)
            .map_err(|s| error_of(&s))?
            .into_inner();
        Ok(reply.events.into_iter().map(convert::event_from).collect())
    }

    async fn vocabulary(&self, principal: &Principal, graph: &str) -> Result<Ontology, OntoError> {
        let mut client = ExecutionServiceClient::new(self.channel.clone());
        let message = pb::GraphRef {
            graph: graph.to_owned(),
        };
        let started = Instant::now();
        let reply = client
            .vocabulary(self.request(message, Some(principal), QUERY_TIMEOUT))
            .await;
        let reply = self
            .timed("ExecutionService", "Vocabulary", started, reply)
            .map_err(|s| error_of(&s))?
            .into_inner();
        convert::vocabulary_from(&json::from_struct(&reply))
    }

    async fn set_vocabulary(
        &self,
        principal: &Principal,
        graph: &str,
        vocabulary: &Ontology,
    ) -> Result<Ontology, OntoError> {
        let mut client = ExecutionServiceClient::new(self.channel.clone());
        let (node_types, relation_types) = convert::vocabulary_to(vocabulary);
        let message = pb::VocabularyRequest {
            graph: graph.to_owned(),
            node_types,
            relation_types,
            workspace: principal.workspace.clone(),
        };
        let started = Instant::now();
        let reply = client
            .set_vocabulary(self.request(message, Some(principal), WRITE_TIMEOUT))
            .await;
        let reply = self
            .timed("ExecutionService", "SetVocabulary", started, reply)
            .map_err(|s| error_of(&s))?
            .into_inner();
        convert::vocabulary_from(&json::from_struct(&reply)["vocabulary"])
    }

    async fn init_graph(
        &self,
        principal: &Principal,
        graph: &str,
        title: &str,
        vocabulary: Option<&Ontology>,
    ) -> Result<Ontology, OntoError> {
        let mut client = ExecutionServiceClient::new(self.channel.clone());
        let (node_types, relation_types) =
            vocabulary.map(convert::vocabulary_to).unwrap_or_default();
        let message = pb::InitGraphRequest {
            graph: graph.to_owned(),
            title: title.to_owned(),
            empty: vocabulary.is_none(),
            node_types,
            relation_types,
            workspace: principal.workspace.clone(),
        };
        let started = Instant::now();
        let reply = client
            .init_graph(self.request(message, Some(principal), WRITE_TIMEOUT))
            .await;
        let reply = self
            .timed("ExecutionService", "InitGraph", started, reply)
            .map_err(|s| error_of(&s))?
            .into_inner();
        convert::vocabulary_from(&json::from_struct(&reply)["vocabulary"])
    }

    async fn schema(&self, principal: &Principal) -> Result<Value, OntoError> {
        let mut client = SchemaServiceClient::new(self.channel.clone());
        let started = Instant::now();
        let reply = client
            .get(self.request(
                pb::SchemaVersionRequest { version: 0 },
                Some(principal),
                QUERY_TIMEOUT,
            ))
            .await;
        let reply = self
            .timed("SchemaService", "Get", started, reply)
            .map_err(|s| error_of(&s))?
            .into_inner();
        Ok(json::from_struct(&reply))
    }

    async fn plan_schema(
        &self,
        principal: &Principal,
        change: &SchemaChange,
    ) -> Result<Value, OntoError> {
        let mut client = SchemaServiceClient::new(self.channel.clone());
        let message = pb::SchemaChangeRequest {
            changes: change.changes.iter().map(json::to_struct).collect(),
            message: change.message.clone(),
            expected_version: 0,
            migrations: change.migrations.iter().map(json::to_struct).collect(),
            dry_run: true,
            workspace: principal.workspace.clone(),
        };
        let started = Instant::now();
        let reply = client
            .plan(self.request(message, Some(principal), QUERY_TIMEOUT))
            .await;
        let reply = self
            .timed("SchemaService", "Plan", started, reply)
            .map_err(|s| error_of(&s))?
            .into_inner();
        Ok(json::from_struct(&reply))
    }

    async fn propose_schema(
        &self,
        principal: &Principal,
        proposal: &SchemaProposalDraft,
    ) -> Result<Value, OntoError> {
        let mut client = SchemaServiceClient::new(self.channel.clone());
        let message = pb::ProposeRequest {
            title: proposal.title.clone(),
            rationale: proposal.rationale.clone(),
            changes: proposal.changes.iter().map(json::to_struct).collect(),
            migrations: proposal.migrations.iter().map(json::to_struct).collect(),
            message: String::new(),
            source: proposal.source.clone(),
            workspace: principal.workspace.clone(),
        };
        let started = Instant::now();
        let reply = client
            .propose(self.request(message, Some(principal), WRITE_TIMEOUT))
            .await;
        let reply = self
            .timed("SchemaService", "Propose", started, reply)
            .map_err(|s| error_of(&s))?
            .into_inner();
        Ok(json::from_struct(&reply))
    }

    async fn proposals(
        &self,
        principal: &Principal,
        status: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Value>, OntoError> {
        let mut client = SchemaServiceClient::new(self.channel.clone());
        let message = pb::ListProposalsRequest {
            status: status.unwrap_or_default().to_owned(),
            limit,
        };
        let started = Instant::now();
        let reply = client
            .list_proposals(self.request(message, Some(principal), QUERY_TIMEOUT))
            .await;
        let reply = self
            .timed("SchemaService", "ListProposals", started, reply)
            .map_err(|s| error_of(&s))?
            .into_inner();
        Ok(match json::from_struct(&reply)["proposals"].take() {
            Value::Array(items) => items,
            _ => Vec::new(),
        })
    }

    async fn mint_session(
        &self,
        principal: &Principal,
        ttl_secs: u64,
    ) -> Result<OntoSession, OntoError> {
        let mut body = json!({ "tenant": principal.tenant, "ttl_secs": ttl_secs });
        if principal.acts() {
            body["act_as"] = json!(principal.id);
            body["roles"] = json!(principal.roles);
            body["workspaces"] = json!([principal.workspace]);
            if let Some(entity) = &principal.entity {
                body["entity"] = json!(entity);
            }
        }
        let mut request = self
            .http
            .post(format!("{}/v1/auth/session", self.url))
            .json(&body);
        if let Some(bearer) = &self.bearer {
            request = request.bearer_auth(bearer);
        }
        let started = Instant::now();
        let response = request.send().await;
        self.metrics
            .grpc_client("HttpApi", "AuthSession", started.elapsed());
        let response = response.map_err(|err| OntoError::Unavailable(err.to_string()))?;
        let status = response.status();
        let reply: Value = response.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(http_error(status, &reply));
        }
        let token = reply["token"]
            .as_str()
            .filter(|t| !t.is_empty())
            .ok_or_else(|| OntoError::Other("the session answer carries no token".into()))?;
        Ok(OntoSession {
            token: token.to_owned(),
            expires_at: reply["expires_at"].as_i64().unwrap_or_default(),
            tenant: reply["tenant"]
                .as_str()
                .unwrap_or(&principal.tenant)
                .to_owned(),
        })
    }
}

/// Proto messages ⇄ the backend's types, both ways so the stub server can
/// speak the same wire format.
pub mod convert {
    use serde_json::Value;

    use super::super::{
        AskAnswer, AskMode, AskRequest, AskStreamEvent, Direction, Edge, Entity, EventQuery,
        FindQuery, OntoError, OntoEvent, ResolvedEntity, Subgraph, TraverseQuery, json, pb,
    };
    use crate::domain::ontology::{NodeType, Ontology, RelationType};

    /// A vocabulary as `ExecutionService` takes it: node types and relation
    /// types in nexc's own field names.
    pub fn vocabulary_to(
        vocabulary: &Ontology,
    ) -> (Vec<prost_types::Struct>, Vec<prost_types::Struct>) {
        let structs = |value: serde_json::Result<Value>| -> prost_types::Struct {
            json::to_struct(&value.unwrap_or(Value::Null))
        };
        (
            vocabulary
                .node_types
                .iter()
                .map(|t| structs(serde_json::to_value(t)))
                .collect(),
            vocabulary
                .relation_types
                .iter()
                .map(|t| structs(serde_json::to_value(t)))
                .collect(),
        )
    }

    /// The vocabulary of an `ExecutionService` answer (`{node_types, relation_types}`).
    pub fn vocabulary_from(value: &Value) -> Result<Ontology, OntoError> {
        let list = |key: &str| value[key].as_array().cloned().unwrap_or_default();
        let node_types = list("node_types")
            .into_iter()
            .map(serde_json::from_value::<NodeType>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| OntoError::Other(format!("a node type does not fit: {err}")))?;
        let relation_types = list("relation_types")
            .into_iter()
            .map(serde_json::from_value::<RelationType>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| OntoError::Other(format!("a relation type does not fit: {err}")))?;
        Ok(Ontology {
            node_types,
            relation_types,
        })
    }

    /// The `{graph, node_types, relation_types}` answer of a vocabulary.
    pub fn vocabulary_value(graph: &str, vocabulary: &Ontology) -> Value {
        serde_json::json!({
            "graph": graph,
            "node_types": vocabulary.node_types,
            "relation_types": vocabulary.relation_types,
        })
    }

    pub fn ask_request_to(req: &AskRequest, workspace: &str) -> pb::AskRequest {
        pb::AskRequest {
            question: req.question.clone(),
            workspace: workspace.to_owned(),
            mode: req.mode.as_str().to_owned(),
            token_budget: req.token_budget.unwrap_or(0),
            ..pb::AskRequest::default()
        }
    }

    pub fn ask_request_from(message: pb::AskRequest) -> AskRequest {
        AskRequest {
            question: message.question,
            mode: match message.mode.as_str() {
                "deterministic" => AskMode::Deterministic,
                "llm" => AskMode::Llm,
                _ => AskMode::Auto,
            },
            token_budget: (message.token_budget > 0).then_some(message.token_budget),
        }
    }

    fn resolved_from(entity: pb::ResolvedEntity) -> ResolvedEntity {
        ResolvedEntity {
            id: entity.id,
            kind: entity.kind,
            title: entity.title,
        }
    }

    fn resolved_to(entity: &ResolvedEntity) -> pb::ResolvedEntity {
        pb::ResolvedEntity {
            id: entity.id.clone(),
            kind: entity.kind.clone(),
            title: entity.title.clone(),
            mention: String::new(),
            confidence: 1.0,
        }
    }

    pub fn ask_answer_from(reply: pb::AskResponse) -> AskAnswer {
        let usage = reply.usage.unwrap_or_default();
        AskAnswer {
            answer: reply.answer,
            mode: reply.mode,
            intent: reply.intent,
            entities: reply.entities.into_iter().map(resolved_from).collect(),
            facts: reply.facts,
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            cost_usd: usage.cost_usd,
            total_ms: reply.timings.map(|t| t.total_ms).unwrap_or(0.0),
            note: reply.note,
        }
    }

    pub fn ask_answer_to(answer: &AskAnswer) -> pb::AskResponse {
        pb::AskResponse {
            answer: answer.answer.clone(),
            mode: answer.mode.clone(),
            intent: answer.intent.clone(),
            entities: answer.entities.iter().map(resolved_to).collect(),
            facts: answer.facts.clone(),
            usage: Some(pb::TokenUsage {
                prompt_tokens: answer.prompt_tokens,
                completion_tokens: answer.completion_tokens,
                cost_usd: answer.cost_usd,
                ..pb::TokenUsage::default()
            }),
            timings: Some(pb::Timings {
                total_ms: answer.total_ms,
                ..pb::Timings::default()
            }),
            note: answer.note.clone(),
            ..pb::AskResponse::default()
        }
    }

    /// The answer as the `data` of a `done` stream event: the server's JSON
    /// of its `AskResponse`, which is what `ask_answer_from_value` reads.
    pub fn ask_answer_to_value(answer: &AskAnswer) -> prost_types::Value {
        json::to_proto(&serde_json::json!({
            "answer": answer.answer,
            "mode": answer.mode,
            "intent": answer.intent,
            "entities": answer.entities,
            "facts": answer.facts,
            "usage": {
                "prompt_tokens": answer.prompt_tokens,
                "completion_tokens": answer.completion_tokens,
                "cost_usd": answer.cost_usd,
            },
            "timings": {"total_ms": answer.total_ms},
            "note": answer.note,
        }))
    }

    fn ask_answer_from_value(value: &Value) -> AskAnswer {
        let text = |path: &[&str]| -> String {
            let mut at = value;
            for key in path {
                at = &at[*key];
            }
            at.as_str().unwrap_or_default().to_owned()
        };
        let number = |path: &[&str]| -> f64 {
            let mut at = value;
            for key in path {
                at = &at[*key];
            }
            at.as_f64().unwrap_or(0.0)
        };
        AskAnswer {
            answer: text(&["answer"]),
            mode: text(&["mode"]),
            intent: text(&["intent"]),
            entities: value["entities"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|e| ResolvedEntity {
                    id: e["id"].as_str().unwrap_or_default().to_owned(),
                    kind: e["kind"].as_str().unwrap_or_default().to_owned(),
                    title: e["title"].as_str().unwrap_or_default().to_owned(),
                })
                .collect(),
            facts: value["facts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            prompt_tokens: number(&["usage", "prompt_tokens"]) as u64,
            completion_tokens: number(&["usage", "completion_tokens"]) as u64,
            cost_usd: number(&["usage", "cost_usd"]),
            total_ms: number(&["timings", "total_ms"]),
            note: text(&["note"]),
        }
    }

    pub fn ask_event_from(event: pb::AskEvent) -> AskStreamEvent {
        let data = event
            .data
            .as_ref()
            .map(json::from_proto)
            .unwrap_or(Value::Null);
        match event.r#type.as_str() {
            "stage" => AskStreamEvent::Stage(event.text),
            "entities" => AskStreamEvent::Entities(
                data.as_array()
                    .into_iter()
                    .flatten()
                    .map(|e| ResolvedEntity {
                        id: e["id"].as_str().unwrap_or_default().to_owned(),
                        kind: e["kind"].as_str().unwrap_or_default().to_owned(),
                        title: e["title"].as_str().unwrap_or_default().to_owned(),
                    })
                    .collect(),
            ),
            "delta" => AskStreamEvent::Delta(event.text),
            "done" => AskStreamEvent::Done(ask_answer_from_value(&data)),
            "error" => AskStreamEvent::Error(event.text),
            other => AskStreamEvent::Stage(other.to_owned()),
        }
    }

    pub fn entity_from(entity: &pb::Entity) -> Entity {
        Entity {
            id: entity.id.clone(),
            kind: entity.kind.clone(),
            title: entity.title.clone(),
            state: entity.state.clone(),
            props: entity
                .props
                .as_ref()
                .map(json::from_struct)
                .unwrap_or_else(|| Value::Object(Default::default())),
        }
    }

    pub fn entity_to(entity: &Entity, workspace: &str) -> pb::Entity {
        pb::Entity {
            id: entity.id.clone(),
            kind: entity.kind.clone(),
            workspace: workspace.to_owned(),
            title: entity.title.clone(),
            state: entity.state.clone(),
            props: Some(json::to_struct(&entity.props)),
            ..pb::Entity::default()
        }
    }

    pub fn find_query_to(q: &FindQuery, workspace: &str) -> pb::FindRequest {
        pb::FindRequest {
            kind: q.kind.clone(),
            workspace: workspace.to_owned(),
            state: q.state.clone(),
            props: q.props.is_object().then(|| json::to_struct(&q.props)),
            limit: q.limit,
            ..pb::FindRequest::default()
        }
    }

    pub fn find_query_from(message: pb::FindRequest) -> FindQuery {
        FindQuery {
            kind: message.kind,
            props: message
                .props
                .as_ref()
                .map(json::from_struct)
                .unwrap_or(Value::Null),
            state: message.state,
            limit: message.limit,
        }
    }

    pub fn traverse_query_to(q: &TraverseQuery) -> pb::TraverseRequest {
        pb::TraverseRequest {
            start: q.start.clone(),
            rels: q.rels.clone(),
            direction: q.direction.as_str().to_owned(),
            depth: q.depth,
            limit: q.limit,
            kinds: q.kinds.clone(),
        }
    }

    pub fn traverse_query_from(message: pb::TraverseRequest) -> TraverseQuery {
        TraverseQuery {
            start: message.start,
            rels: message.rels,
            direction: match message.direction.as_str() {
                "in" => Direction::In,
                "both" => Direction::Both,
                _ => Direction::Out,
            },
            depth: message.depth,
            limit: message.limit,
            kinds: message.kinds,
        }
    }

    pub fn subgraph_from(message: pb::Subgraph) -> Subgraph {
        Subgraph {
            nodes: message.nodes.iter().map(entity_from).collect(),
            edges: message
                .edges
                .into_iter()
                .map(|e| Edge {
                    from: e.from,
                    rel: e.rel,
                    to: e.to,
                })
                .collect(),
            truncated: message.truncated,
        }
    }

    pub fn subgraph_to(graph: &Subgraph, workspace: &str) -> pb::Subgraph {
        pb::Subgraph {
            nodes: graph
                .nodes
                .iter()
                .map(|n| entity_to(n, workspace))
                .collect(),
            edges: graph
                .edges
                .iter()
                .map(|e| pb::Edge {
                    from: e.from.clone(),
                    rel: e.rel.clone(),
                    to: e.to.clone(),
                })
                .collect(),
            truncated: graph.truncated,
        }
    }

    pub fn event_query_to(q: &EventQuery, workspace: &str) -> pb::EventFilter {
        pb::EventFilter {
            workspace: workspace.to_owned(),
            actor: q.actor.clone().unwrap_or_default(),
            target: q.target.clone().unwrap_or_default(),
            action: q.action.clone().unwrap_or_default(),
            since: q.since_ms.unwrap_or(0),
            until: q.until_ms.unwrap_or(0),
            limit: q.limit,
        }
    }

    pub fn event_query_from(message: pb::EventFilter) -> EventQuery {
        let some = |s: String| (!s.is_empty()).then_some(s);
        EventQuery {
            actor: some(message.actor),
            target: some(message.target),
            action: some(message.action),
            since_ms: (message.since > 0).then_some(message.since),
            until_ms: (message.until > 0).then_some(message.until),
            limit: message.limit,
        }
    }

    pub fn event_from(event: pb::Event) -> OntoEvent {
        OntoEvent {
            id: event.id,
            ts: event.ts,
            actor: event.actor,
            action: event.action,
            target: event.target,
            outcome: event.outcome,
            payload: event
                .payload
                .as_ref()
                .map(json::from_proto)
                .unwrap_or(Value::Null),
        }
    }

    pub fn event_to(event: OntoEvent) -> pb::Event {
        pb::Event {
            id: event.id,
            ts: event.ts,
            actor: event.actor,
            action: event.action,
            target: event.target,
            outcome: event.outcome,
            payload: Some(json::to_proto(&event.payload)),
            ..pb::Event::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn statuses_map_to_errors_and_back() {
        let unavailable = error_of(&Status::unavailable("connection refused"));
        assert!(matches!(unavailable, OntoError::Unavailable(_)));
        assert!(matches!(
            error_of(&Status::deadline_exceeded("")),
            OntoError::Unavailable(_)
        ));
        assert!(matches!(
            error_of(&Status::permission_denied(
                "user:1 has no access to workspace w"
            )),
            OntoError::Denied(_)
        ));
        assert!(matches!(
            error_of(&Status::unauthenticated("invalid token")),
            OntoError::Denied(_)
        ));
        assert!(matches!(
            error_of(&Status::invalid_argument("bad")),
            OntoError::Invalid(_)
        ));
        assert!(matches!(
            error_of(&Status::not_found("execution_graph:g")),
            OntoError::NotFound(_)
        ));
        assert!(matches!(
            error_of(&Status::failed_precondition("no nexc-execution package")),
            OntoError::Invalid(_)
        ));
        assert!(matches!(
            error_of(&Status::internal("boom")),
            OntoError::Other(_)
        ));
        for err in [
            OntoError::Unavailable("a".into()),
            OntoError::Denied("b".into()),
            OntoError::NotFound("n".into()),
            OntoError::Invalid("c".into()),
            OntoError::Other("d".into()),
        ] {
            assert_eq!(
                std::mem::discriminant(&error_of(&status_of(&err))),
                std::mem::discriminant(&err)
            );
        }
        assert!(GrpcOntology::new("not a url", None).is_err());
        assert!(GrpcOntology::new("http://localhost:50051", Some("t")).is_ok());
    }
}
