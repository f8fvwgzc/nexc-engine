//! onto-intelli: where nexc's ontology lives (`docs/ONTOLOGY.md`).
//!
//! nexc keeps no ontology model of its own. onto-intelli is the one place
//! where entity kinds, link types and actions are defined, configured and
//! reasoned over, with one tenant per nexc workspace (`ws_{workspace id}`):
//!
//! * every graph's execution vocabulary (its node types and relation types)
//!   is read from and written to onto-intelli's `ExecutionService`, through
//!   an in-process read-through cache ([`vocabulary`]);
//! * the workspace's teams, projects, cycles, issues, agents, graphs,
//!   documents, people and runs are projected into it as entities and links
//!   ([`projection`]), next to the audit's facts ([`export`]): ids, names,
//!   states and links, never content. onto-intelli's own UI renders that map;
//!   the backend mints the browser's session for it;
//! * the assistant reads the schema, dry-runs changes and files proposals
//!   (`SchemaService`), which people approve in onto-intelli;
//! * questions about what agents did are answered from the facts ([`ask`]).
//!
//! Every call is made as someone: a member through onto-intelli's
//! impersonation headers ([`Principal::user`]), the assistant and the
//! runtime as agents, the export as the backend's own service principal.
//!
//! The server is behind [`OntologyBackend`], so the gRPC client ([`grpc`])
//! and an in-memory stub ([`stub`]) are interchangeable; tests use the stub,
//! directly or behind an in-process server. An embedded engine would be a
//! third implementation of the same trait (see `docs/ONTOLOGY.md`).
#![forbid(unsafe_code)]

pub mod ask;
pub mod export;
pub mod facts;
pub mod grpc;
pub mod migrate;
pub mod projection;
pub mod stub;
pub mod tools;
pub mod vocabulary;

use std::fmt;
use std::pin::Pin;

use futures::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::domain::ontology::Ontology;
use crate::domain::workspace::WorkspaceRole;

/// The generated `onto_intelli.v1` messages, client and server.
#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    unreachable_pub,
    rustdoc::all
)]
pub mod pb {
    tonic::include_proto!("onto_intelli.v1");
}

/// Why onto-intelli could not answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OntoError {
    /// The server is not there, not answering in time, or refused the connection.
    #[error("onto-intelli is not reachable: {0}")]
    Unavailable(String),
    /// The server refused the token, or the principal may not do what was asked.
    #[error("onto-intelli refused the request: {0}")]
    Denied(String),
    /// What was asked for does not exist (a graph without a vocabulary yet).
    #[error("onto-intelli has no such thing: {0}")]
    NotFound(String),
    /// The request does not fit: a bad argument, a value the schema refuses.
    #[error("onto-intelli rejected the request: {0}")]
    Invalid(String),
    /// Anything else the server reported.
    #[error("onto-intelli failed: {0}")]
    Other(String),
}

/// What `AdminService/Health` reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OntoHealth {
    pub status: String,
    pub version: String,
    pub revision: u64,
    /// The LLM provider onto-intelli explains with (`mock`, `anthropic`, `none`, …).
    pub llm: String,
    pub llm_available: bool,
}

/// What `EventService/Ingest` accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IngestOutcome {
    pub accepted: u64,
    /// True when the records were queued rather than applied before the reply.
    pub queued: bool,
}

/// onto-intelli's roles, as nexc grants them (`docs/ONTOLOGY.md` §2). The
/// server's governance decides what each may do: by default `schema_admin`
/// applies schema changes, `approver` approves proposals, `agent` only
/// proposes.
pub mod role {
    pub const VIEWER: &str = "viewer";
    pub const MEMBER: &str = "member";
    pub const APPROVER: &str = "approver";
    pub const SCHEMA_ADMIN: &str = "schema_admin";
    pub const AGENT: &str = "agent";
}

/// The onto-intelli roles of a workspace role: owners and admins approve
/// proposals and may apply schema changes, members read and write, guests
/// only read.
pub fn roles_for(role: WorkspaceRole) -> Vec<String> {
    let roles: &[&str] = match role {
        WorkspaceRole::Owner | WorkspaceRole::Admin => {
            &[role::MEMBER, role::APPROVER, role::SCHEMA_ADMIN]
        }
        WorkspaceRole::Member => &[role::MEMBER],
        WorkspaceRole::Guest => &[role::VIEWER],
    };
    roles.iter().map(|r| (*r).to_owned()).collect()
}

/// The tenant that holds a nexc workspace's ontology: `ws_{id without dashes}`
/// (its PostgreSQL schema is `onto_ws_…`).
pub fn tenant_of(workspace_id: Uuid) -> String {
    format!("ws_{}", workspace_id.simple())
}

/// The onto-intelli workspace, inside the tenant, that holds the nexc
/// workspace's entities (`{prefix}{workspace id}`).
pub fn workspace_name(prefix: &str, workspace_id: Uuid) -> String {
    format!("{prefix}{workspace_id}")
}

/// Who a call is made as, and where.
///
/// onto-intelli's bearer authentication binds the backend's token to the
/// backend's own service principal. A principal with an `id` travels as the
/// impersonation headers (`x-onto-act-as`, `x-onto-act-roles`,
/// `x-onto-act-workspaces`, `x-onto-act-entity`), so onto-intelli checks and
/// audits the person or the agent, not the service; one without acts as the
/// service itself (the export).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Principal {
    /// `user:{id}`, `assistant:{user id}`, `agent:runtime`; empty for the service itself.
    pub id: String,
    /// onto-intelli roles ([`role`]).
    pub roles: Vec<String>,
    /// The person the principal stands for (`person:{user id}`).
    pub entity: Option<String>,
    /// `x-onto-tenant` ([`tenant_of`]); empty addresses the server's default tenant.
    pub tenant: String,
    /// The onto-intelli workspace ([`workspace_name`]).
    pub workspace: String,
}

impl Principal {
    /// The backend's own service principal in `workspace` of the default
    /// tenant (tests, and servers without tenants).
    pub fn system(workspace: impl Into<String>) -> Self {
        Principal {
            id: String::new(),
            roles: Vec::new(),
            entity: None,
            tenant: String::new(),
            workspace: workspace.into(),
        }
    }

    /// The backend's own service principal in a nexc workspace's tenant: the
    /// export and the projection, which write what nexc knows.
    pub fn service(prefix: &str, workspace_id: Uuid) -> Self {
        Principal {
            tenant: tenant_of(workspace_id),
            ..Principal::system(workspace_name(prefix, workspace_id))
        }
    }

    /// A member of a nexc workspace, with the onto-intelli roles of their workspace role.
    pub fn user(prefix: &str, workspace_id: Uuid, user_id: Uuid, role: WorkspaceRole) -> Self {
        Principal {
            id: format!("user:{user_id}"),
            roles: roles_for(role),
            entity: Some(person_id(user_id)),
            ..Principal::service(prefix, workspace_id)
        }
    }

    /// The assistant working for a member: an agent, which reads, plans and
    /// proposes but never applies or approves.
    pub fn assistant(prefix: &str, workspace_id: Uuid, user_id: Uuid) -> Self {
        Principal {
            id: format!("assistant:{user_id}"),
            roles: vec![role::AGENT.to_owned()],
            entity: Some(person_id(user_id)),
            ..Principal::service(prefix, workspace_id)
        }
    }

    /// The run engine reading a graph's vocabulary: an agent.
    pub fn runtime(prefix: &str, workspace_id: Uuid) -> Self {
        Principal {
            id: "agent:runtime".to_owned(),
            roles: vec![role::AGENT.to_owned()],
            ..Principal::service(prefix, workspace_id)
        }
    }

    /// The backend's own service principal in the same tenant and workspace.
    pub fn as_service(&self) -> Self {
        Principal {
            id: String::new(),
            roles: Vec::new(),
            entity: None,
            ..self.clone()
        }
    }

    /// Whether the call is made for someone (the impersonation headers go along).
    pub fn acts(&self) -> bool {
        !self.id.is_empty()
    }

    /// Whether the principal holds `role`.
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
}

/// The entity of a person (`person:{user id}`).
pub fn person_id(user_id: Uuid) -> String {
    format!("person:{user_id}")
}

/// The entity of a graph, which also holds its execution vocabulary
/// (`execution_graph:{graph id}`).
pub fn graph_entity(graph_id: Uuid) -> String {
    format!("execution_graph:{graph_id}")
}

/// How onto-intelli should answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AskMode {
    /// From the graph when it can, the model otherwise.
    #[default]
    Auto,
    /// Only from the graph.
    Deterministic,
    /// Always through the model.
    Llm,
}

impl AskMode {
    fn as_str(self) -> &'static str {
        match self {
            AskMode::Auto => "auto",
            AskMode::Deterministic => "deterministic",
            AskMode::Llm => "llm",
        }
    }
}

/// A question for `IntelliService/Ask`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AskRequest {
    pub question: String,
    pub mode: AskMode,
    /// Facts the model may be given, in tokens; the server's default when `None`.
    pub token_budget: Option<u32>,
}

/// An entity onto-intelli resolved from the question's words.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedEntity {
    pub id: String,
    pub kind: String,
    pub title: String,
}

/// onto-intelli's answer.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct AskAnswer {
    pub answer: String,
    /// `deterministic`, `llm`, `llm_cache`, `answer_cache`, `fallback`, `clarify`, `proposal` or `action`.
    pub mode: String,
    pub intent: String,
    pub entities: Vec<ResolvedEntity>,
    /// The facts the answer rests on, as onto-intelli states them (`agent:x read document:y`).
    pub facts: Vec<String>,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cost_usd: f64,
    /// onto-intelli's own measure of the answer, in milliseconds.
    pub total_ms: f64,
    pub note: String,
}

impl AskAnswer {
    /// Tokens the answer cost: prompt and completion.
    pub fn tokens(&self) -> u64 {
        self.prompt_tokens + self.completion_tokens
    }
}

/// One event of `IntelliService/AskStream`.
#[derive(Debug, Clone, PartialEq)]
pub enum AskStreamEvent {
    /// A stage of the answer was reached (`intent`, `retrieve`, `llm`).
    Stage(String),
    Entities(Vec<ResolvedEntity>),
    /// A piece of the answer's text.
    Delta(String),
    Done(AskAnswer),
    Error(String),
}

/// A stream of answer events.
pub type AskStream = Pin<Box<dyn Stream<Item = AskStreamEvent> + Send>>;

/// An entity of the graph.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Entity {
    /// `kind:key`.
    pub id: String,
    pub kind: String,
    pub title: String,
    pub state: String,
    pub props: Value,
}

impl Entity {
    /// A string property, when set.
    pub fn prop_str(&self, name: &str) -> Option<&str> {
        self.props.get(name).and_then(Value::as_str)
    }
}

/// `OntologyService/FindEntities`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FindQuery {
    pub kind: String,
    /// Properties the entities must have with these values.
    pub props: Value,
    pub state: Vec<String>,
    pub limit: u32,
}

/// Which way `OntologyService/Traverse` walks the links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Out,
    In,
    Both,
}

impl Direction {
    fn as_str(self) -> &'static str {
        match self {
            Direction::Out => "out",
            Direction::In => "in",
            Direction::Both => "both",
        }
    }
}

/// `OntologyService/Traverse`.
#[derive(Debug, Clone, PartialEq)]
pub struct TraverseQuery {
    pub start: Vec<String>,
    pub rels: Vec<String>,
    pub direction: Direction,
    pub depth: u32,
    pub limit: u32,
    /// Keep only entities of these kinds (every kind when empty).
    pub kinds: Vec<String>,
}

/// A link of the graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct Edge {
    pub from: String,
    pub rel: String,
    pub to: String,
}

/// What a traversal found.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Subgraph {
    pub nodes: Vec<Entity>,
    pub edges: Vec<Edge>,
    pub truncated: bool,
}

/// `EventService/List`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EventQuery {
    pub actor: Option<String>,
    pub target: Option<String>,
    /// Exact, or a dotted prefix (`agent.` matches `agent.read`).
    pub action: Option<String>,
    /// Unix milliseconds.
    pub since_ms: Option<i64>,
    pub until_ms: Option<i64>,
    pub limit: u32,
}

/// An event of the audit trail onto-intelli keeps.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OntoEvent {
    pub id: String,
    /// Unix milliseconds.
    pub ts: i64,
    pub actor: String,
    pub action: String,
    pub target: String,
    /// `success`, `failure`, `denied` or `pending`.
    pub outcome: String,
    pub payload: Value,
}

impl OntoEvent {
    /// A string of the payload, when set.
    pub fn payload_str(&self, name: &str) -> Option<&str> {
        self.payload.get(name).and_then(Value::as_str)
    }
}

/// A schema change set (`SchemaService/Plan`): changes such as
/// `{"op": "put_link_type", "name": …, "def": {…}}` and the migrations that
/// resolve what they break.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SchemaChange {
    pub changes: Vec<Value>,
    #[serde(default)]
    pub migrations: Vec<Value>,
    #[serde(default)]
    pub message: String,
}

/// A proposal to file (`SchemaService/Propose`): a change set with the
/// reasons for it, for people to review.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SchemaProposalDraft {
    pub title: String,
    pub rationale: String,
    pub changes: Vec<Value>,
    #[serde(default)]
    pub migrations: Vec<Value>,
    /// Where it comes from (`nexc-assistant`).
    #[serde(default)]
    pub source: String,
}

/// A browser session minted for a principal (`POST /v1/auth/session`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OntoSession {
    /// `onto1.…`: one principal, one tenant, short-lived; it cannot impersonate.
    pub token: String,
    /// Unix seconds.
    pub expires_at: i64,
    pub tenant: String,
}

/// An onto-intelli server, or something standing in for one.
#[async_trait::async_trait]
pub trait OntologyBackend: Send + Sync {
    /// Where the facts go, for the operator.
    fn location(&self) -> String;

    async fn health(&self) -> Result<OntoHealth, OntoError>;

    /// Streams records (`{"op": "upsert", …}`, `{"op": "link", …}`,
    /// `{"op": "event", …}`, `{"op": "delete", …}`) into the principal's
    /// tenant and workspace; they are applied before the reply, so a reply
    /// means they are in.
    async fn ingest(
        &self,
        principal: &Principal,
        records: Vec<Value>,
    ) -> Result<IngestOutcome, OntoError>;

    async fn ask(&self, principal: &Principal, req: AskRequest) -> Result<AskAnswer, OntoError>;

    async fn ask_stream(
        &self,
        principal: &Principal,
        req: AskRequest,
    ) -> Result<AskStream, OntoError>;

    async fn find(&self, principal: &Principal, q: FindQuery) -> Result<Vec<Entity>, OntoError>;

    async fn traverse(
        &self,
        principal: &Principal,
        q: TraverseQuery,
    ) -> Result<Subgraph, OntoError>;

    async fn events(
        &self,
        principal: &Principal,
        q: EventQuery,
    ) -> Result<Vec<OntoEvent>, OntoError>;

    /// A graph's node types and relation types (`ExecutionService/Vocabulary`);
    /// [`OntoError::NotFound`] when the graph has none in onto-intelli yet.
    async fn vocabulary(&self, principal: &Principal, graph: &str) -> Result<Ontology, OntoError>;

    /// Replaces a graph's vocabulary in one atomic write
    /// (`ExecutionService/SetVocabulary`): types are created, changed or
    /// removed. The graph must exist ([`OntologyBackend::init_graph`]).
    async fn set_vocabulary(
        &self,
        principal: &Principal,
        graph: &str,
        vocabulary: &Ontology,
    ) -> Result<Ontology, OntoError>;

    /// Creates a graph, or names an existing one (`ExecutionService/InitGraph`),
    /// with `vocabulary`'s types upserted; without one, a new graph gets
    /// nothing (nexc always passes the types it wants).
    async fn init_graph(
        &self,
        principal: &Principal,
        graph: &str,
        title: &str,
        vocabulary: Option<&Ontology>,
    ) -> Result<Ontology, OntoError>;

    /// The tenant's active schema document (`SchemaService/Get`): object
    /// types, link types, action types, rules, with `schema_version`.
    async fn schema(&self, principal: &Principal) -> Result<Value, OntoError>;

    /// Dry-runs a change set (`SchemaService/Plan`): validation, diff, impact.
    async fn plan_schema(
        &self,
        principal: &Principal,
        change: &SchemaChange,
    ) -> Result<Value, OntoError>;

    /// Files a proposal (`SchemaService/Propose`); people approve it in onto-intelli.
    async fn propose_schema(
        &self,
        principal: &Principal,
        proposal: &SchemaProposalDraft,
    ) -> Result<Value, OntoError>;

    /// Proposals of the tenant, newest first (`SchemaService/ListProposals`).
    async fn proposals(
        &self,
        principal: &Principal,
        status: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Value>, OntoError>;

    /// Mints a short-lived browser session for `principal` in its tenant
    /// (`POST /v1/auth/session`); the backend's own token never reaches a page.
    async fn mint_session(
        &self,
        principal: &Principal,
        ttl_secs: u64,
    ) -> Result<OntoSession, OntoError>;
}

impl fmt::Display for Principal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.acts() {
            write!(f, "{} ({})", self.id, self.roles.join(","))
        } else {
            f.write_str("service")
        }
    }
}

/// `serde_json::Value` ⇄ `google.protobuf.Struct` / `Value`.
pub mod json {
    use prost_types::value::Kind;
    use prost_types::{ListValue, Struct};
    use serde_json::{Map, Number, Value};

    pub fn to_proto(value: &Value) -> prost_types::Value {
        let kind = match value {
            Value::Null => Kind::NullValue(0),
            Value::Bool(b) => Kind::BoolValue(*b),
            Value::Number(n) => Kind::NumberValue(n.as_f64().unwrap_or(0.0)),
            Value::String(s) => Kind::StringValue(s.clone()),
            Value::Array(items) => Kind::ListValue(ListValue {
                values: items.iter().map(to_proto).collect(),
            }),
            Value::Object(_) => Kind::StructValue(to_struct(value)),
        };
        prost_types::Value { kind: Some(kind) }
    }

    /// An object as a `Struct`; anything else becomes an empty one.
    pub fn to_struct(value: &Value) -> Struct {
        let fields = match value {
            Value::Object(map) => map.iter().map(|(k, v)| (k.clone(), to_proto(v))).collect(),
            _ => Default::default(),
        };
        Struct { fields }
    }

    pub fn from_proto(value: &prost_types::Value) -> Value {
        match &value.kind {
            None | Some(Kind::NullValue(_)) => Value::Null,
            Some(Kind::BoolValue(b)) => Value::Bool(*b),
            Some(Kind::NumberValue(n)) => {
                // Whole numbers come back as they went in; `jsonb` would do the same.
                if n.fract() == 0.0 && n.abs() < 9.0e15 {
                    Value::from(*n as i64)
                } else {
                    Number::from_f64(*n).map_or(Value::Null, Value::Number)
                }
            }
            Some(Kind::StringValue(s)) => Value::String(s.clone()),
            Some(Kind::ListValue(list)) => {
                Value::Array(list.values.iter().map(from_proto).collect())
            }
            Some(Kind::StructValue(s)) => from_struct(s),
        }
    }

    pub fn from_struct(s: &Struct) -> Value {
        let map: Map<String, Value> = s
            .fields
            .iter()
            .map(|(k, v)| (k.clone(), from_proto(v)))
            .collect();
        Value::Object(map)
    }

    #[cfg(test)]
    mod tests {
        use serde_json::json;

        use super::*;

        #[test]
        fn json_survives_the_round_trip() {
            let value = json!({"a": 1, "b": "x", "c": [true, null, 2.5], "d": {"e": -3}});
            assert_eq!(from_struct(&to_struct(&value)), value);
            assert_eq!(from_proto(&to_proto(&json!([1, "two"]))), json!([1, "two"]));
            assert_eq!(to_struct(&json!("not an object")).fields.len(), 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn principals_carry_the_mapped_roles_and_the_tenant() {
        let w = Uuid::nil();
        let u = Uuid::from_u128(7);
        let owner = Principal::user("nexc-", w, u, WorkspaceRole::Owner);
        assert_eq!(owner.roles, ["member", "approver", "schema_admin"]);
        assert_eq!(owner.tenant, "ws_00000000000000000000000000000000");
        assert_eq!(owner.workspace, format!("nexc-{w}"));
        assert_eq!(owner.entity.as_deref(), Some(&*format!("person:{u}")));
        assert_eq!(
            Principal::user("nexc-", w, u, WorkspaceRole::Admin).roles,
            owner.roles
        );
        assert_eq!(
            Principal::user("nexc-", w, u, WorkspaceRole::Member).roles,
            ["member"]
        );
        assert_eq!(
            Principal::user("nexc-", w, u, WorkspaceRole::Guest).roles,
            ["viewer"]
        );
        let assistant = Principal::assistant("nexc-", w, u);
        assert_eq!(assistant.roles, ["agent"]);
        assert!(assistant.acts() && !assistant.has_role("approver"));
        assert_eq!(Principal::runtime("nexc-", w).roles, ["agent"]);
        let service = Principal::service("nexc-", w);
        assert!(!service.acts());
        assert_eq!(service.tenant, owner.tenant);
        assert_eq!(service.to_string(), "service");
    }
}
