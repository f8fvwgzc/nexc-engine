//! The tonic client of a real onto-intelli: `authorization: Bearer` and
//! `x-onto-workspace` on every request, the server's status codes mapped to
//! [`OntoError`], and the proto messages mapped to the backend's types
//! ([`convert`], shared with the stub server).

use std::time::Duration;

use futures::StreamExt;
use serde_json::Value;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint};
use tonic::{Code, Request, Status};

use super::pb::admin_service_client::AdminServiceClient;
use super::pb::event_service_client::EventServiceClient;
use super::pb::intelli_service_client::IntelliServiceClient;
use super::pb::ontology_service_client::OntologyServiceClient;
use super::{
    AskAnswer, AskRequest, AskStream, AskStreamEvent, Entity, EventQuery, FindQuery, IngestOutcome,
    OntoError, OntoEvent, OntoHealth, OntologyBackend, Principal, Subgraph, TraverseQuery, json,
};

/// Opening the TCP connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// One question, including the model's time when it is used.
const ASK_TIMEOUT: Duration = Duration::from_secs(60);
/// A graph query; deterministic, so quick.
const QUERY_TIMEOUT: Duration = Duration::from_secs(10);
/// One ingest stream.
const INGEST_TIMEOUT: Duration = Duration::from_secs(60);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);

/// A real onto-intelli at `url`.
#[derive(Clone)]
pub struct GrpcOntology {
    url: String,
    channel: Channel,
    token: Option<MetadataValue<tonic::metadata::Ascii>>,
}

impl GrpcOntology {
    /// Connects lazily: the first request opens the connection, and a server
    /// that is down is a failed request, never a failed start.
    pub fn new(url: &str, token: Option<&str>) -> Result<Self, OntoError> {
        let endpoint = Endpoint::from_shared(url.to_owned())
            .map_err(|err| OntoError::Invalid(format!("NEXC_ONTO_URL: {err}")))?
            .connect_timeout(CONNECT_TIMEOUT)
            .tcp_nodelay(true);
        let token = match token {
            Some(token) => Some(
                format!("Bearer {token}")
                    .parse()
                    .map_err(|_| OntoError::Invalid("NEXC_ONTO_TOKEN is not ASCII".into()))?,
            ),
            None => None,
        };
        Ok(GrpcOntology {
            url: url.to_owned(),
            channel: endpoint.connect_lazy(),
            token,
        })
    }

    fn request<T>(&self, message: T, workspace: Option<&str>, timeout: Duration) -> Request<T> {
        let mut request = Request::new(message);
        if let Some(token) = &self.token {
            request
                .metadata_mut()
                .insert("authorization", token.clone());
        }
        if let Some(workspace) = workspace
            && let Ok(value) = workspace.parse()
        {
            request.metadata_mut().insert("x-onto-workspace", value);
        }
        request.set_timeout(timeout);
        request
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
        Code::InvalidArgument | Code::NotFound | Code::FailedPrecondition | Code::AlreadyExists => {
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
        OntoError::Invalid(m) => Status::invalid_argument(m.clone()),
        OntoError::Other(m) => Status::internal(m.clone()),
    }
}

#[async_trait::async_trait]
impl OntologyBackend for GrpcOntology {
    fn location(&self) -> String {
        self.url.clone()
    }

    async fn health(&self) -> Result<OntoHealth, OntoError> {
        let mut client = AdminServiceClient::new(self.channel.clone());
        let reply = client
            .health(self.request((), None, HEALTH_TIMEOUT))
            .await
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
        workspace: &str,
        records: Vec<Value>,
    ) -> Result<IngestOutcome, OntoError> {
        let mut client = EventServiceClient::new(self.channel.clone());
        let messages: Vec<prost_types::Struct> = records.iter().map(json::to_struct).collect();
        let stream = futures::stream::iter(messages);
        let reply = client
            .ingest(self.request(stream, Some(workspace), INGEST_TIMEOUT))
            .await
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
        let reply = client
            .ask(self.request(message, Some(&principal.workspace), ASK_TIMEOUT))
            .await
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
        let stream = client
            .ask_stream(self.request(message, Some(&principal.workspace), ASK_TIMEOUT))
            .await
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
        let reply = client
            .find_entities(self.request(message, Some(&principal.workspace), QUERY_TIMEOUT))
            .await
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
        let reply = client
            .traverse(self.request(message, Some(&principal.workspace), QUERY_TIMEOUT))
            .await
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
        let reply = client
            .list(self.request(message, Some(&principal.workspace), QUERY_TIMEOUT))
            .await
            .map_err(|s| error_of(&s))?
            .into_inner();
        Ok(reply.events.into_iter().map(convert::event_from).collect())
    }
}

/// Proto messages ⇄ the backend's types, both ways so the stub server can
/// speak the same wire format.
pub mod convert {
    use serde_json::Value;

    use super::super::{
        AskAnswer, AskMode, AskRequest, AskStreamEvent, Direction, Edge, Entity, EventQuery,
        FindQuery, OntoEvent, ResolvedEntity, Subgraph, TraverseQuery, json, pb,
    };

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
            error_of(&Status::internal("boom")),
            OntoError::Other(_)
        ));
        for err in [
            OntoError::Unavailable("a".into()),
            OntoError::Denied("b".into()),
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
