//! onto-intelli as the audit's query engine ("ask the audit", AgentSecOps
//! phase 3, `docs/AGENTSECOPS.md` §4.5).
//!
//! The trace spine records what every agent did as spans. The export worker
//! ([`export`]) turns them into ontology facts (entities, links and events:
//! ids, names, hosts, counts and decisions, never content) and streams them
//! to onto-intelli, one onto-intelli workspace per nexc workspace. Questions
//! ([`ask`]) are answered from that graph: the canned audit questions as
//! graph and event queries the backend composes (0 tokens, milliseconds),
//! anything else through onto-intelli's own `Ask`, which reaches a model only
//! to explain, with facts it cites.
//!
//! The server is behind [`OntologyBackend`], so the gRPC client ([`grpc`])
//! and an in-memory stub ([`stub`]) are interchangeable; tests use the stub,
//! directly or behind an in-process gRPC server.
#![forbid(unsafe_code)]

pub mod ask;
pub mod export;
pub mod facts;
pub mod grpc;
pub mod stub;

use std::fmt;
use std::pin::Pin;

use futures::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

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
    /// The server refused the token, or the principal may not see what was asked.
    #[error("onto-intelli refused the request: {0}")]
    Denied(String),
    /// The request does not fit: an unknown entity, a bad argument.
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

/// The role a question is asked with, in onto-intelli's terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalRole {
    /// Everything in the workspace, restricted entities included.
    Admin,
    /// Read and write; restricted entities only through an ACL.
    Member,
    /// Read only.
    Viewer,
}

/// Who is asking, and about which onto-intelli workspace.
///
/// onto-intelli's bearer authentication binds the backend's token to one
/// principal; the asker's identity and role travel with the request so the
/// backend's own clearance filter ([`ask::Clearance`]) and the audit entry
/// on both sides name the person, not the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    /// `user:{id}` of the member, `system` for the export worker.
    pub id: String,
    pub role: PrincipalRole,
    /// The onto-intelli workspace (`{prefix}{nexc workspace id}`).
    pub workspace: String,
}

impl Principal {
    /// The backend itself, exporting facts into `workspace`.
    pub fn system(workspace: impl Into<String>) -> Self {
        Principal {
            id: "system".into(),
            role: PrincipalRole::Admin,
            workspace: workspace.into(),
        }
    }
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

/// An onto-intelli server, or something standing in for one.
#[async_trait::async_trait]
pub trait OntologyBackend: Send + Sync {
    /// Where the facts go, for the operator.
    fn location(&self) -> String;

    async fn health(&self) -> Result<OntoHealth, OntoError>;

    /// Streams records (`{"op": "upsert", …}`, `{"op": "link", …}`,
    /// `{"op": "event", …}`) into `workspace`; they are applied before the
    /// reply, so a reply means they are in.
    async fn ingest(
        &self,
        workspace: &str,
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
}

/// The onto-intelli workspace that holds a nexc workspace's facts.
pub fn workspace_name(prefix: &str, workspace_id: Uuid) -> String {
    format!("{prefix}{workspace_id}")
}

impl fmt::Display for PrincipalRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PrincipalRole::Admin => "admin",
            PrincipalRole::Member => "member",
            PrincipalRole::Viewer => "viewer",
        })
    }
}

/// `serde_json::Value` ⇄ `google.protobuf.Struct` / `Value`.
pub(crate) mod json {
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
