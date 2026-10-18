//! An onto-intelli that lives in memory: the facts it is given, the links
//! between them and the events it was told about, answering `find`,
//! `traverse` and `events` the way the server does for the audit's
//! questions, and `ask` the way a test tells it to. Tests use it directly
//! as the backend, or behind [`server`], an in-process gRPC server that the
//! real client talks to.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Mutex;

use chrono::Utc;
use serde_json::{Map, Value, json};

use super::{
    AskAnswer, AskRequest, AskStream, AskStreamEvent, Direction, Entity, EventQuery, FindQuery,
    IngestOutcome, OntoError, OntoEvent, OntoHealth, OntologyBackend, Principal, Subgraph,
    TraverseQuery,
};

/// Answers a question; tests set one with [`StubOntology::answer_with`].
pub type Answerer = Box<dyn Fn(&Principal, &AskRequest) -> AskAnswer + Send + Sync>;

#[derive(Default)]
struct State {
    /// workspace → entity id → entity.
    entities: BTreeMap<String, BTreeMap<String, Entity>>,
    /// (workspace, from, rel, to).
    links: BTreeSet<(String, String, String, String)>,
    events: Vec<(String, OntoEvent)>,
    /// Every record ingested, as given, with its workspace.
    records: Vec<(String, Value)>,
    ingests: u64,
    asks: Vec<(Principal, AskRequest)>,
    failure: Option<OntoError>,
    answerer: Option<Answerer>,
}

/// The in-memory onto-intelli.
pub struct StubOntology {
    state: Mutex<State>,
}

impl Default for StubOntology {
    fn default() -> Self {
        Self::new()
    }
}

impl StubOntology {
    pub fn new() -> Self {
        StubOntology {
            state: Mutex::new(State::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Every call fails with `error` until called again with `None`.
    pub fn fail_with(&self, error: Option<OntoError>) {
        self.lock().failure = error;
    }

    /// How `ask` answers from now on.
    pub fn answer_with(&self, answerer: Answerer) {
        self.lock().answerer = Some(answerer);
    }

    /// Everything ingested so far, as given.
    pub fn records(&self) -> Vec<Value> {
        self.lock().records.iter().map(|(_, r)| r.clone()).collect()
    }

    /// Ingest calls so far.
    pub fn ingests(&self) -> u64 {
        self.lock().ingests
    }

    /// The questions asked so far.
    pub fn asks(&self) -> Vec<(Principal, AskRequest)> {
        self.lock().asks.clone()
    }

    pub fn entity(&self, workspace: &str, id: &str) -> Option<Entity> {
        self.lock().entities.get(workspace)?.get(id).cloned()
    }

    /// Ids of the entities of a workspace.
    pub fn entity_ids(&self, workspace: &str) -> Vec<String> {
        self.lock()
            .entities
            .get(workspace)
            .map(|e| e.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn has_link(&self, workspace: &str, from: &str, rel: &str, to: &str) -> bool {
        self.lock()
            .links
            .contains(&(workspace.into(), from.into(), rel.into(), to.into()))
    }

    /// The links of a workspace as `(from, rel, to)`.
    pub fn links(&self, workspace: &str) -> Vec<(String, String, String)> {
        self.lock()
            .links
            .iter()
            .filter(|(ws, ..)| ws == workspace)
            .map(|(_, f, r, t)| (f.clone(), r.clone(), t.clone()))
            .collect()
    }

    pub fn events_of(&self, workspace: &str) -> Vec<OntoEvent> {
        self.lock()
            .events
            .iter()
            .filter(|(ws, _)| ws == workspace)
            .map(|(_, e)| e.clone())
            .collect()
    }

    /// Forgets everything (not the configured failure or answerer).
    pub fn clear(&self) {
        let mut state = self.lock();
        state.entities.clear();
        state.links.clear();
        state.events.clear();
        state.records.clear();
    }

    fn check(state: &State) -> Result<(), OntoError> {
        match &state.failure {
            Some(err) => Err(err.clone()),
            None => Ok(()),
        }
    }

    fn apply(state: &mut State, workspace: &str, record: &Value) -> Result<(), OntoError> {
        let op = record.get("op").and_then(Value::as_str);
        let has_identity = record.get("id").is_some() || record.get("kind").is_some();
        match op {
            Some("upsert") => Self::upsert(state, workspace, record),
            None if has_identity => Self::upsert(state, workspace, record),
            Some("link") => {
                let (from, rel, to) = link_of(record)?;
                state.links.insert((workspace.into(), from, rel, to));
                Ok(())
            }
            Some("unlink") => {
                let (from, rel, to) = link_of(record)?;
                state.links.remove(&(workspace.into(), from, rel, to));
                Ok(())
            }
            Some("event") => {
                let action = record
                    .get("action")
                    .and_then(Value::as_str)
                    .ok_or_else(|| OntoError::Invalid("event without action".into()))?;
                let n = state.events.len() + 1;
                let event = OntoEvent {
                    id: format!("evt:{n}"),
                    ts: record
                        .get("ts")
                        .and_then(Value::as_i64)
                        .unwrap_or_else(|| Utc::now().timestamp_millis()),
                    actor: text(record, "actor").unwrap_or_default(),
                    action: action.to_owned(),
                    target: text(record, "target").unwrap_or_default(),
                    outcome: text(record, "outcome").unwrap_or_else(|| "success".into()),
                    payload: record.get("payload").cloned().unwrap_or(Value::Null),
                };
                state.events.push((workspace.into(), event));
                Ok(())
            }
            Some(other) => Err(OntoError::Invalid(format!("unknown op `{other}`"))),
            None => Err(OntoError::Invalid(
                "unrecognized record: expected a mutation (op) or an entity (id/kind)".into(),
            )),
        }
    }

    fn upsert(state: &mut State, workspace: &str, record: &Value) -> Result<(), OntoError> {
        let id = match text(record, "id") {
            Some(id) => id,
            None => {
                let kind = text(record, "kind")
                    .ok_or_else(|| OntoError::Invalid("entity without kind".into()))?;
                let key = text(record, "key")
                    .ok_or_else(|| OntoError::Invalid("entity without id or key".into()))?;
                format!("{kind}:{key}")
            }
        };
        let kind = text(record, "kind")
            .or_else(|| id.split_once(':').map(|(k, _)| k.to_owned()))
            .unwrap_or_default();
        let entities = state.entities.entry(workspace.into()).or_default();
        let entity = entities.entry(id.clone()).or_insert_with(|| Entity {
            id: id.clone(),
            kind: kind.clone(),
            props: json!({}),
            ..Entity::default()
        });
        if let Some(title) = text(record, "title") {
            entity.title = title;
        }
        if let Some(state_) = text(record, "state") {
            entity.state = state_;
        }
        if let Some(Value::Object(given)) = record.get("props") {
            let replace = record
                .get("replace")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let mut props = if replace {
                Map::new()
            } else {
                entity.props.as_object().cloned().unwrap_or_default()
            };
            props.extend(given.iter().map(|(k, v)| (k.clone(), v.clone())));
            entity.props = Value::Object(props);
        }
        if let Some(Value::Array(links)) = record.get("links") {
            for spec in links {
                let rel = text(spec, "rel")
                    .ok_or_else(|| OntoError::Invalid("link without rel".into()))?;
                let to =
                    text(spec, "to").ok_or_else(|| OntoError::Invalid("link without to".into()))?;
                state.links.insert((workspace.into(), id.clone(), rel, to));
            }
        }
        Ok(())
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn link_of(record: &Value) -> Result<(String, String, String), OntoError> {
    match (
        text(record, "from"),
        text(record, "rel"),
        text(record, "to"),
    ) {
        (Some(from), Some(rel), Some(to)) => Ok((from, rel, to)),
        _ => Err(OntoError::Invalid("link needs from, rel and to".into())),
    }
}

/// Whether every property of `wanted` is on `entity` with the same value.
fn props_match(entity: &Entity, wanted: &Value) -> bool {
    match wanted {
        Value::Object(map) => map.iter().all(|(k, v)| entity.props.get(k) == Some(v)),
        _ => true,
    }
}

#[async_trait::async_trait]
impl OntologyBackend for StubOntology {
    fn location(&self) -> String {
        "stub://in-memory".into()
    }

    async fn health(&self) -> Result<OntoHealth, OntoError> {
        Self::check(&self.lock())?;
        Ok(OntoHealth {
            status: "ok".into(),
            version: "stub".into(),
            revision: 0,
            llm: "none".into(),
            llm_available: false,
        })
    }

    async fn ingest(
        &self,
        workspace: &str,
        records: Vec<Value>,
    ) -> Result<IngestOutcome, OntoError> {
        let mut state = self.lock();
        Self::check(&state)?;
        state.ingests += 1;
        for record in &records {
            Self::apply(&mut state, workspace, record)?;
            state.records.push((workspace.into(), record.clone()));
        }
        Ok(IngestOutcome {
            accepted: records.len() as u64,
            queued: false,
        })
    }

    async fn ask(&self, principal: &Principal, req: AskRequest) -> Result<AskAnswer, OntoError> {
        let mut state = self.lock();
        Self::check(&state)?;
        state.asks.push((principal.clone(), req.clone()));
        Ok(match &state.answerer {
            Some(answer) => answer(principal, &req),
            None => AskAnswer {
                answer: "The stub has no answer configured.".into(),
                mode: "deterministic".into(),
                intent: "lookup".into(),
                ..AskAnswer::default()
            },
        })
    }

    async fn ask_stream(
        &self,
        principal: &Principal,
        req: AskRequest,
    ) -> Result<AskStream, OntoError> {
        let answer = self.ask(principal, req).await?;
        let events = vec![
            AskStreamEvent::Stage("intent".into()),
            AskStreamEvent::Entities(answer.entities.clone()),
            AskStreamEvent::Delta(answer.answer.clone()),
            AskStreamEvent::Done(answer),
        ];
        Ok(Box::pin(futures::stream::iter(events)))
    }

    async fn find(&self, principal: &Principal, q: FindQuery) -> Result<Vec<Entity>, OntoError> {
        let state = self.lock();
        Self::check(&state)?;
        let limit = if q.limit == 0 { 50 } else { q.limit as usize };
        Ok(state
            .entities
            .get(&principal.workspace)
            .into_iter()
            .flat_map(|e| e.values())
            .filter(|e| q.kind.is_empty() || e.kind == q.kind)
            .filter(|e| q.state.is_empty() || q.state.contains(&e.state))
            .filter(|e| props_match(e, &q.props))
            .take(limit)
            .cloned()
            .collect())
    }

    async fn traverse(
        &self,
        principal: &Principal,
        q: TraverseQuery,
    ) -> Result<Subgraph, OntoError> {
        let state = self.lock();
        Self::check(&state)?;
        let ws = &principal.workspace;
        let limit = if q.limit == 0 { 100 } else { q.limit as usize };
        let mut seen: BTreeSet<String> = q.start.iter().cloned().collect();
        let mut queue: VecDeque<(String, u32)> = q.start.iter().map(|s| (s.clone(), 0)).collect();
        let mut edges: BTreeSet<(String, String, String)> = BTreeSet::new();
        let mut order: Vec<String> = q.start.clone();
        let mut truncated = false;
        while let Some((id, depth)) = queue.pop_front() {
            if depth >= q.depth {
                continue;
            }
            for (lws, from, rel, to) in &state.links {
                if lws != ws || (!q.rels.is_empty() && !q.rels.contains(rel)) {
                    continue;
                }
                let next = match q.direction {
                    Direction::Out if from == &id => to,
                    Direction::In if to == &id => from,
                    Direction::Both if from == &id => to,
                    Direction::Both if to == &id => from,
                    _ => continue,
                };
                edges.insert((from.clone(), rel.clone(), to.clone()));
                if seen.insert(next.clone()) {
                    if order.len() >= limit {
                        truncated = true;
                        continue;
                    }
                    order.push(next.clone());
                    queue.push_back((next.clone(), depth + 1));
                }
            }
        }
        let entities = state.entities.get(ws);
        let nodes = order
            .into_iter()
            .map(|id| {
                entities
                    .and_then(|e| e.get(&id).cloned())
                    .unwrap_or_else(|| Entity {
                        kind: id
                            .split_once(':')
                            .map(|(k, _)| k.to_owned())
                            .unwrap_or_default(),
                        id,
                        ..Entity::default()
                    })
            })
            .filter(|e| q.kinds.is_empty() || q.kinds.contains(&e.kind))
            .collect();
        Ok(Subgraph {
            nodes,
            edges: edges
                .into_iter()
                .map(|(from, rel, to)| super::Edge { from, rel, to })
                .collect(),
            truncated,
        })
    }

    async fn events(
        &self,
        principal: &Principal,
        q: EventQuery,
    ) -> Result<Vec<OntoEvent>, OntoError> {
        let state = self.lock();
        Self::check(&state)?;
        let limit = if q.limit == 0 { 50 } else { q.limit as usize };
        let prefix = q
            .action
            .as_ref()
            .map(|a| format!("{}.", a.trim_end_matches('.')));
        let mut found: Vec<OntoEvent> = state
            .events
            .iter()
            .filter(|(ws, _)| ws == &principal.workspace)
            .map(|(_, e)| e)
            .filter(|e| q.actor.as_ref().is_none_or(|a| &e.actor == a))
            .filter(|e| q.target.as_ref().is_none_or(|t| &e.target == t))
            .filter(|e| {
                q.action.as_ref().is_none_or(|a| {
                    &e.action == a || prefix.as_ref().is_some_and(|p| e.action.starts_with(p))
                })
            })
            .filter(|e| q.since_ms.is_none_or(|s| e.ts >= s))
            .filter(|e| q.until_ms.is_none_or(|u| e.ts <= u))
            .cloned()
            .collect();
        found.sort_by(|a, b| b.ts.cmp(&a.ts).then_with(|| b.id.cmp(&a.id)));
        found.truncate(limit);
        Ok(found)
    }
}

pub mod server {
    //! The stub behind the real wire: an in-process gRPC server with the
    //! `onto_intelli.v1` services the backend uses, so the tonic client is
    //! exercised end to end (metadata, streaming, status codes) without a
    //! real onto-intelli.

    use std::net::SocketAddr;
    use std::sync::Arc;

    use futures::StreamExt;
    use serde_json::Value;
    use tokio::sync::oneshot;
    use tokio::task::JoinHandle;
    use tonic::metadata::MetadataMap;
    use tonic::{Request, Response, Status, Streaming};

    use super::StubOntology;
    use crate::ontology::grpc::{convert, status_of};
    use crate::ontology::pb::admin_service_server::{AdminService, AdminServiceServer};
    use crate::ontology::pb::event_service_server::{EventService, EventServiceServer};
    use crate::ontology::pb::intelli_service_server::{IntelliService, IntelliServiceServer};
    use crate::ontology::pb::ontology_service_server::{OntologyService, OntologyServiceServer};
    use crate::ontology::{
        AskStreamEvent, OntoError, OntologyBackend, Principal, PrincipalRole, json, pb,
    };

    /// The services, sharing one stub.
    #[derive(Clone)]
    pub struct StubServer {
        stub: Arc<StubOntology>,
        /// The bearer token callers must present, when set.
        token: Option<String>,
    }

    /// A running stub server.
    pub struct Running {
        pub addr: SocketAddr,
        stop: Option<oneshot::Sender<()>>,
        task: JoinHandle<()>,
    }

    impl Running {
        /// `http://127.0.0.1:{port}`, for `NEXC_ONTO_URL`.
        pub fn url(&self) -> String {
            format!("http://{}", self.addr)
        }

        /// Stops the server.
        pub async fn stop(mut self) {
            if let Some(stop) = self.stop.take() {
                let _ = stop.send(());
            }
            let _ = (&mut self.task).await;
        }
    }

    impl Drop for Running {
        fn drop(&mut self) {
            if let Some(stop) = self.stop.take() {
                let _ = stop.send(());
            }
        }
    }

    /// Serves `stub` on a free local port.
    pub async fn serve(stub: Arc<StubOntology>, token: Option<&str>) -> std::io::Result<Running> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = StubServer {
            stub,
            token: token.map(str::to_owned),
        };
        let (stop_tx, stop_rx) = oneshot::channel();
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        let task = tokio::spawn(async move {
            let result = tonic::transport::Server::builder()
                .add_service(AdminServiceServer::new(server.clone()))
                .add_service(EventServiceServer::new(server.clone()))
                .add_service(IntelliServiceServer::new(server.clone()))
                .add_service(OntologyServiceServer::new(server))
                .serve_with_incoming_shutdown(incoming, async {
                    let _ = stop_rx.await;
                })
                .await;
            if let Err(err) = result {
                tracing::error!(error = %err, "stub onto-intelli server failed");
            }
        });
        Ok(Running {
            addr,
            stop: Some(stop_tx),
            task,
        })
    }

    impl StubServer {
        fn principal(&self, metadata: &MetadataMap) -> Result<Principal, Status> {
            if let Some(expected) = &self.token {
                let presented = metadata
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("Bearer "));
                if presented != Some(expected.as_str()) {
                    return Err(Status::unauthenticated("invalid token"));
                }
            }
            let workspace = metadata
                .get("x-onto-workspace")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("default")
                .to_owned();
            Ok(Principal {
                id: "stub-token".into(),
                role: PrincipalRole::Admin,
                workspace,
            })
        }
    }

    fn failed(err: OntoError) -> Status {
        status_of(&err)
    }

    #[tonic::async_trait]
    impl AdminService for StubServer {
        async fn health(&self, req: Request<()>) -> Result<Response<pb::HealthStatus>, Status> {
            self.principal(req.metadata())?;
            let health = self.stub.health().await.map_err(failed)?;
            Ok(Response::new(pb::HealthStatus {
                status: health.status,
                version: health.version,
                revision: health.revision,
                llm: health.llm,
                llm_available: health.llm_available,
            }))
        }

        async fn stats(&self, _: Request<()>) -> Result<Response<prost_types::Struct>, Status> {
            Err(Status::unimplemented("stub"))
        }

        async fn schema(&self, _: Request<()>) -> Result<Response<prost_types::Struct>, Status> {
            Err(Status::unimplemented("stub"))
        }

        async fn actions(&self, _: Request<()>) -> Result<Response<prost_types::Struct>, Status> {
            Err(Status::unimplemented("stub"))
        }

        async fn call(
            &self,
            _: Request<pb::CallRequest>,
        ) -> Result<Response<pb::CallResponse>, Status> {
            Err(Status::unimplemented("stub"))
        }
    }

    #[tonic::async_trait]
    impl EventService for StubServer {
        async fn append(&self, _: Request<pb::EventInput>) -> Result<Response<pb::Event>, Status> {
            Err(Status::unimplemented("stub"))
        }

        async fn list(
            &self,
            req: Request<pb::EventFilter>,
        ) -> Result<Response<pb::EventList>, Status> {
            let principal = self.principal(req.metadata())?;
            let query = convert::event_query_from(req.into_inner());
            let events = self.stub.events(&principal, query).await.map_err(failed)?;
            Ok(Response::new(pb::EventList {
                events: events.into_iter().map(convert::event_to).collect(),
            }))
        }

        async fn ingest(
            &self,
            req: Request<Streaming<prost_types::Struct>>,
        ) -> Result<Response<pb::IngestSummary>, Status> {
            let principal = self.principal(req.metadata())?;
            let mut stream = req.into_inner();
            let mut records: Vec<Value> = Vec::new();
            while let Some(record) = stream.next().await {
                records.push(json::from_struct(&record?));
            }
            let outcome = self
                .stub
                .ingest(&principal.workspace, records)
                .await
                .map_err(failed)?;
            Ok(Response::new(pb::IngestSummary {
                accepted: outcome.accepted,
                queued: outcome.queued,
                result: None,
            }))
        }
    }

    type AskEventStream =
        std::pin::Pin<Box<dyn futures::Stream<Item = Result<pb::AskEvent, Status>> + Send>>;

    #[tonic::async_trait]
    impl IntelliService for StubServer {
        type AskStreamStream = AskEventStream;

        async fn ask(
            &self,
            req: Request<pb::AskRequest>,
        ) -> Result<Response<pb::AskResponse>, Status> {
            let principal = self.principal(req.metadata())?;
            let request = convert::ask_request_from(req.into_inner());
            let answer = self.stub.ask(&principal, request).await.map_err(failed)?;
            Ok(Response::new(convert::ask_answer_to(&answer)))
        }

        async fn ask_stream(
            &self,
            req: Request<pb::AskRequest>,
        ) -> Result<Response<Self::AskStreamStream>, Status> {
            let principal = self.principal(req.metadata())?;
            let request = convert::ask_request_from(req.into_inner());
            let events = self
                .stub
                .ask_stream(&principal, request)
                .await
                .map_err(failed)?;
            let wire = events.map(|event| {
                Ok(match event {
                    AskStreamEvent::Stage(text) => pb::AskEvent {
                        r#type: "stage".into(),
                        text,
                        data: None,
                    },
                    AskStreamEvent::Entities(entities) => pb::AskEvent {
                        r#type: "entities".into(),
                        text: String::new(),
                        data: Some(json::to_proto(
                            &serde_json::to_value(entities).unwrap_or_default(),
                        )),
                    },
                    AskStreamEvent::Delta(text) => pb::AskEvent {
                        r#type: "delta".into(),
                        text,
                        data: None,
                    },
                    AskStreamEvent::Done(answer) => pb::AskEvent {
                        r#type: "done".into(),
                        text: String::new(),
                        data: Some(convert::ask_answer_to_value(&answer)),
                    },
                    AskStreamEvent::Error(text) => pb::AskEvent {
                        r#type: "error".into(),
                        text,
                        data: None,
                    },
                })
            });
            Ok(Response::new(Box::pin(wire)))
        }

        async fn build_context(
            &self,
            _: Request<pb::ContextRequest>,
        ) -> Result<Response<prost_types::Struct>, Status> {
            Err(Status::unimplemented("stub"))
        }

        async fn infer(
            &self,
            _: Request<pb::GraphQuery>,
        ) -> Result<Response<prost_types::Struct>, Status> {
            Err(Status::unimplemented("stub"))
        }

        async fn dependencies(
            &self,
            _: Request<pb::GraphQuery>,
        ) -> Result<Response<prost_types::Struct>, Status> {
            Err(Status::unimplemented("stub"))
        }

        async fn impact(
            &self,
            _: Request<pb::GraphQuery>,
        ) -> Result<Response<prost_types::Struct>, Status> {
            Err(Status::unimplemented("stub"))
        }

        async fn root_cause(
            &self,
            _: Request<pb::GraphQuery>,
        ) -> Result<Response<prost_types::Struct>, Status> {
            Err(Status::unimplemented("stub"))
        }

        async fn critical_path(
            &self,
            _: Request<pb::GraphQuery>,
        ) -> Result<Response<prost_types::Struct>, Status> {
            Err(Status::unimplemented("stub"))
        }

        async fn cycles(
            &self,
            _: Request<pb::GraphQuery>,
        ) -> Result<Response<prost_types::Struct>, Status> {
            Err(Status::unimplemented("stub"))
        }
    }

    #[tonic::async_trait]
    impl OntologyService for StubServer {
        async fn upsert_entity(
            &self,
            _: Request<pb::EntityInput>,
        ) -> Result<Response<pb::Entity>, Status> {
            Err(Status::unimplemented("stub"))
        }

        async fn patch_entity(
            &self,
            _: Request<pb::EntityPatch>,
        ) -> Result<Response<pb::Entity>, Status> {
            Err(Status::unimplemented("stub"))
        }

        async fn get_entity(
            &self,
            req: Request<pb::EntityRef>,
        ) -> Result<Response<pb::Entity>, Status> {
            let principal = self.principal(req.metadata())?;
            let id = req.into_inner().id;
            match self.stub.entity(&principal.workspace, &id) {
                Some(entity) => Ok(Response::new(convert::entity_to(
                    &entity,
                    &principal.workspace,
                ))),
                None => Err(Status::not_found(format!("entity {id}"))),
            }
        }

        async fn delete_entity(
            &self,
            _: Request<pb::EntityRef>,
        ) -> Result<Response<pb::DeleteResult>, Status> {
            Err(Status::unimplemented("stub"))
        }

        async fn find_entities(
            &self,
            req: Request<pb::FindRequest>,
        ) -> Result<Response<pb::EntityList>, Status> {
            let principal = self.principal(req.metadata())?;
            let query = convert::find_query_from(req.into_inner());
            let found = self.stub.find(&principal, query).await.map_err(failed)?;
            Ok(Response::new(pb::EntityList {
                total: found.len() as u64,
                entities: found
                    .iter()
                    .map(|e| convert::entity_to(e, &principal.workspace))
                    .collect(),
            }))
        }

        async fn search(
            &self,
            _: Request<pb::SearchRequest>,
        ) -> Result<Response<pb::SearchResult>, Status> {
            Err(Status::unimplemented("stub"))
        }

        async fn create_link(
            &self,
            _: Request<pb::LinkInput>,
        ) -> Result<Response<pb::LinkResult>, Status> {
            Err(Status::unimplemented("stub"))
        }

        async fn delete_link(
            &self,
            _: Request<pb::LinkInput>,
        ) -> Result<Response<pb::LinkResult>, Status> {
            Err(Status::unimplemented("stub"))
        }

        async fn apply(
            &self,
            _: Request<pb::ApplyRequest>,
        ) -> Result<Response<pb::MutationResult>, Status> {
            Err(Status::unimplemented("stub"))
        }

        async fn neighbors(
            &self,
            _: Request<pb::NeighborsRequest>,
        ) -> Result<Response<pb::NeighborsResult>, Status> {
            Err(Status::unimplemented("stub"))
        }

        async fn traverse(
            &self,
            req: Request<pb::TraverseRequest>,
        ) -> Result<Response<pb::Subgraph>, Status> {
            let principal = self.principal(req.metadata())?;
            let query = convert::traverse_query_from(req.into_inner());
            let found = self
                .stub
                .traverse(&principal, query)
                .await
                .map_err(failed)?;
            Ok(Response::new(convert::subgraph_to(
                &found,
                &principal.workspace,
            )))
        }

        async fn path(
            &self,
            _: Request<pb::PathRequest>,
        ) -> Result<Response<pb::PathResult>, Status> {
            Err(Status::unimplemented("stub"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal() -> Principal {
        Principal::system("ws")
    }

    #[tokio::test]
    async fn records_become_entities_links_and_events() {
        let stub = StubOntology::new();
        stub.ingest(
            "ws",
            vec![
                json!({"op": "upsert", "kind": "agent", "key": "a", "title": "A",
                       "props": {"principal": "agent://w/a"},
                       "links": [{"rel": "ran", "to": "run:1"}]}),
                json!({"id": "run:1", "kind": "run", "state": "running"}),
                json!({"op": "link", "from": "run:1", "rel": "executed", "to": "node:1"}),
                json!({"op": "event", "action": "agent.read", "actor": "agent:a",
                       "target": "document:d", "ts": 5, "payload": {"label": "restricted"}}),
            ],
        )
        .await
        .unwrap();
        assert_eq!(stub.entity("ws", "agent:a").unwrap().title, "A");
        assert!(stub.has_link("ws", "agent:a", "ran", "run:1"));
        assert!(stub.has_link("ws", "run:1", "executed", "node:1"));
        assert_eq!(
            stub.events_of("ws")[0].payload_str("label"),
            Some("restricted")
        );

        // A second upsert merges properties and keeps the state.
        stub.ingest("ws", vec![json!({"id": "run:1", "props": {"cost": 1}})])
            .await
            .unwrap();
        let run = stub.entity("ws", "run:1").unwrap();
        assert_eq!(run.state, "running");
        assert_eq!(run.props["cost"], 1);

        let found = stub
            .find(
                &principal(),
                FindQuery {
                    kind: "run".into(),
                    ..FindQuery::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        let walked = stub
            .traverse(
                &principal(),
                TraverseQuery {
                    start: vec!["node:1".into()],
                    rels: vec![],
                    direction: Direction::In,
                    depth: 3,
                    limit: 10,
                    kinds: vec![],
                },
            )
            .await
            .unwrap();
        let ids: Vec<&str> = walked.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["node:1", "run:1", "agent:a"]);
        let events = stub
            .events(
                &principal(),
                EventQuery {
                    action: Some("agent".into()),
                    since_ms: Some(1),
                    ..EventQuery::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert!(
            stub.entity("other", "agent:a").is_none(),
            "workspaces are apart"
        );

        stub.fail_with(Some(OntoError::Unavailable("down".into())));
        assert!(stub.health().await.is_err());
        let bad = stub.ingest("ws", vec![json!({"op": "nope"})]).await;
        assert!(matches!(bad, Err(OntoError::Unavailable(_))));
        stub.fail_with(None);
        let bad = stub.ingest("ws", vec![json!({"op": "nope"})]).await;
        assert!(matches!(bad, Err(OntoError::Invalid(_))));
    }
}
