//! The A2A (Agent-to-Agent protocol) adapter's types: the agent card an
//! exposed agent publishes, the tasks it receives over JSON-RPC, and the
//! remote agents (counterparts) a workspace registers. The JSON-RPC shapes
//! follow the A2A specification (`tasks/send`, `tasks/get`, `tasks/cancel`,
//! `tasks/sendSubscribe`); the nexc specifics travel in `metadata` under
//! `nexc.*` keys.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;
use uuid::Uuid;

use super::capabilities::DataLabel;
use super::mcp::is_valid_name;
use super::validation::FieldErrors;

/// Longest counterpart name (the tool is `a2a:<name>`).
pub const NAME_MAX: usize = 64;
/// Longest card URL or header.
pub const TEXT_MAX: usize = 2_000;
/// Prefix of the gateway tool that sends a task to a counterpart.
pub const TOOL_PREFIX: &str = "a2a:";
/// Most local agents one counterpart's allow-list names.
pub const ALLOWED_AGENTS_MAX: usize = 64;
/// The `metadata` key carrying the number of A2A hops before a task.
pub const HOP_KEY: &str = "nexc.hop";
/// The `metadata` key carrying the principals a task passed through.
pub const CHAIN_KEY: &str = "nexc.chain";
/// Most principals kept in a chain (a longer one is refused as too deep).
pub const CHAIN_MAX: usize = 32;
/// Most parts in one inbound message.
pub const PARTS_MAX: usize = 32;
/// Most files in one inbound message.
pub const FILES_MAX: usize = 8;
/// Most characters of text an inbound message may carry (the node's content).
pub const MESSAGE_CHARS_MAX: usize = 64 * 1024;
/// Largest artifact answered inline (base64) rather than by URI.
pub const INLINE_ARTIFACT_MAX: usize = 4 * 1024 * 1024;
/// Longest task title (the first line of the message).
pub const TITLE_MAX: usize = 200;

// --- JSON-RPC ------------------------------------------------------------------------------------

/// JSON-RPC: the request could not be parsed.
pub const E_PARSE: i64 = -32700;
/// JSON-RPC: not a request.
pub const E_INVALID_REQUEST: i64 = -32600;
/// JSON-RPC: no such method.
pub const E_METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC: the params do not fit the method.
pub const E_INVALID_PARAMS: i64 = -32602;
/// JSON-RPC: the server failed.
pub const E_INTERNAL: i64 = -32603;
/// A2A: no task with that id (for this caller).
pub const E_TASK_NOT_FOUND: i64 = -32001;
/// A2A: the task already ended.
pub const E_TASK_NOT_CANCELABLE: i64 = -32002;
/// A2A: push notifications are not supported.
pub const E_PUSH_UNSUPPORTED: i64 = -32003;
/// A2A: the operation is not supported.
pub const E_UNSUPPORTED: i64 = -32004;
/// A2A: a part's content type is not accepted.
pub const E_CONTENT_TYPE: i64 = -32005;
/// nexc: no usable agent API token (HTTP 401).
pub const E_UNAUTHENTICATED: i64 = -32010;
/// nexc: the token's agent may not do this (HTTP 403).
pub const E_FORBIDDEN: i64 = -32011;
/// nexc: no exposed agent at that address (HTTP 404).
pub const E_AGENT_NOT_FOUND: i64 = -32012;

/// A JSON-RPC error, with the HTTP status it is answered with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub status: u16,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        let status = match code {
            E_UNAUTHENTICATED => 401,
            E_FORBIDDEN => 403,
            E_AGENT_NOT_FOUND => 404,
            _ => 200,
        };
        RpcError {
            code,
            message: message.into(),
            status,
        }
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(E_INVALID_PARAMS, message)
    }

    /// The same error answered with another HTTP status.
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    /// The JSON-RPC error object.
    pub fn to_value(&self) -> Value {
        json!({"code": self.code, "message": self.message})
    }
}

/// The states of an A2A task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Submitted,
    Working,
    Completed,
    Canceled,
    Failed,
}

impl TaskState {
    /// The wire spelling (A2A `TaskState`).
    pub fn as_str(self) -> &'static str {
        match self {
            TaskState::Submitted => "submitted",
            TaskState::Working => "working",
            TaskState::Completed => "completed",
            TaskState::Canceled => "canceled",
            TaskState::Failed => "failed",
        }
    }

    /// Whether the task can no longer change.
    pub fn is_final(self) -> bool {
        matches!(
            self,
            TaskState::Completed | TaskState::Canceled | TaskState::Failed
        )
    }

    /// The state a run's status maps to.
    pub fn of_run(status: super::run::RunStatus) -> Self {
        use super::run::RunStatus;
        match status {
            RunStatus::Queued => TaskState::Submitted,
            RunStatus::Running => TaskState::Working,
            RunStatus::Succeeded => TaskState::Completed,
            RunStatus::Failed => TaskState::Failed,
            RunStatus::Cancelled => TaskState::Canceled,
        }
    }

    /// The state an A2A client reported, when it is one.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "submitted" => TaskState::Submitted,
            "working" | "input-required" => TaskState::Working,
            "completed" => TaskState::Completed,
            "canceled" | "cancelled" => TaskState::Canceled,
            "failed" | "rejected" | "unknown" => TaskState::Failed,
            _ => return None,
        })
    }
}

/// One part of an A2A message, as accepted inbound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    Text(String),
    /// A file with its bytes (base64 on the wire) or a URI the agent is told about.
    File {
        name: String,
        mime: String,
        bytes: Option<Vec<u8>>,
        uri: Option<String>,
    },
    /// Structured data, kept as JSON.
    Data(Value),
}

/// An inbound message read field by field; nothing is trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundMessage {
    pub role: String,
    pub parts: Vec<Part>,
}

impl InboundMessage {
    /// Reads an A2A `Message`; refuses what does not fit the limits.
    pub fn parse(value: &Value) -> Result<Self, RpcError> {
        use base64::Engine as _;
        let object = value
            .as_object()
            .ok_or_else(|| RpcError::invalid_params("`message` must be an object"))?;
        let role = object
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .chars()
            .take(32)
            .collect();
        let raw = object
            .get("parts")
            .and_then(Value::as_array)
            .ok_or_else(|| RpcError::invalid_params("`message.parts` must be an array"))?;
        if raw.is_empty() || raw.len() > PARTS_MAX {
            return Err(RpcError::invalid_params(format!(
                "`message.parts` must have 1 to {PARTS_MAX} parts"
            )));
        }
        let mut parts = Vec::with_capacity(raw.len());
        let mut chars = 0usize;
        let mut files = 0usize;
        for part in raw {
            let kind = part
                .get("type")
                .or_else(|| part.get("kind"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            match kind {
                "text" => {
                    let text = part
                        .get("text")
                        .and_then(Value::as_str)
                        .ok_or_else(|| RpcError::invalid_params("a text part needs `text`"))?;
                    chars += text.chars().count();
                    parts.push(Part::Text(text.to_owned()));
                }
                "file" => {
                    files += 1;
                    let file = part
                        .get("file")
                        .and_then(Value::as_object)
                        .ok_or_else(|| RpcError::invalid_params("a file part needs `file`"))?;
                    let name = file
                        .get("name")
                        .and_then(Value::as_str)
                        .map(|n| n.rsplit(['/', '\\']).next().unwrap_or_default().trim())
                        .filter(|n| !n.is_empty())
                        .unwrap_or("attachment")
                        .chars()
                        .take(255)
                        .collect();
                    let mime = file
                        .get("mimeType")
                        .or_else(|| file.get("mime_type"))
                        .and_then(Value::as_str)
                        .unwrap_or("application/octet-stream")
                        .chars()
                        .take(127)
                        .collect();
                    let bytes = match file.get("bytes").and_then(Value::as_str) {
                        Some(encoded) => Some(
                            base64::engine::general_purpose::STANDARD
                                .decode(encoded.trim())
                                .map_err(|_| {
                                    RpcError::invalid_params("`file.bytes` is not base64")
                                })?,
                        ),
                        None => None,
                    };
                    let uri = file
                        .get("uri")
                        .and_then(Value::as_str)
                        .map(|u| u.chars().take(TEXT_MAX).collect());
                    if bytes.is_none() && uri.is_none() {
                        return Err(RpcError::invalid_params(
                            "a file part needs `file.bytes` or `file.uri`",
                        ));
                    }
                    parts.push(Part::File {
                        name,
                        mime,
                        bytes,
                        uri,
                    });
                }
                "data" => {
                    let data = part.get("data").cloned().unwrap_or(Value::Null);
                    chars += data.to_string().len();
                    parts.push(Part::Data(data));
                }
                other => {
                    return Err(RpcError::new(
                        E_CONTENT_TYPE,
                        format!(
                            "part type `{}` is not accepted; text, file and data are",
                            clean(other)
                        ),
                    ));
                }
            }
        }
        if chars > MESSAGE_CHARS_MAX {
            return Err(RpcError::invalid_params(format!(
                "the message carries more than {MESSAGE_CHARS_MAX} characters of text"
            )));
        }
        if files > FILES_MAX {
            return Err(RpcError::invalid_params(format!(
                "at most {FILES_MAX} files in one message"
            )));
        }
        Ok(InboundMessage { role, parts })
    }

    /// The task's title: the first line of its first text part.
    pub fn title(&self) -> String {
        self.parts
            .iter()
            .find_map(|p| match p {
                Part::Text(text) => text.lines().map(str::trim).find(|l| !l.is_empty()),
                _ => None,
            })
            .map(|line| line.chars().take(TITLE_MAX).collect())
            .unwrap_or_else(|| "A2A task".to_owned())
    }

    /// The text parts joined, each data part as JSON, for the node's content.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for part in &self.parts {
            match part {
                Part::Text(text) => {
                    if !out.is_empty() {
                        out.push_str("\n\n");
                    }
                    out.push_str(text.trim());
                }
                Part::Data(data) => {
                    if !out.is_empty() {
                        out.push_str("\n\n");
                    }
                    out.push_str("```json\n");
                    out.push_str(&serde_json::to_string_pretty(data).unwrap_or_default());
                    out.push_str("\n```");
                }
                Part::File { .. } => {}
            }
        }
        out
    }
}

fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(64).collect()
}

/// What `metadata` says about the chain a task is part of: the number of
/// nexc-mediated hops before it and the principals it passed through.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hops {
    pub hop: u8,
    pub chain: Vec<String>,
}

impl Hops {
    /// Reads `nexc.hop` and `nexc.chain` from a task's metadata; absent means
    /// a direct call. A chain over [`CHAIN_MAX`] or a hop over 255 is refused.
    pub fn parse(metadata: Option<&Value>) -> Result<Self, RpcError> {
        let Some(metadata) = metadata else {
            return Ok(Hops::default());
        };
        let hop =
            match metadata.get(HOP_KEY) {
                None | Some(Value::Null) => 0,
                Some(value) => value.as_u64().filter(|h| *h <= 255).ok_or_else(|| {
                    RpcError::invalid_params("`metadata.nexc.hop` must be 0 to 255")
                })? as u8,
            };
        let chain: Vec<String> = match metadata.get(CHAIN_KEY) {
            None | Some(Value::Null) => Vec::new(),
            Some(value) => value
                .as_array()
                .ok_or_else(|| {
                    RpcError::invalid_params("`metadata.nexc.chain` must be an array of principals")
                })?
                .iter()
                .map(|p| {
                    p.as_str()
                        .map(|s| s.chars().take(super::trace::PRINCIPAL_MAX).collect())
                        .ok_or_else(|| {
                            RpcError::invalid_params("`metadata.nexc.chain` must name principals")
                        })
                })
                .collect::<Result<_, _>>()?,
        };
        if chain.len() > CHAIN_MAX {
            return Err(RpcError::invalid_params(format!(
                "`metadata.nexc.chain` names more than {CHAIN_MAX} principals"
            )));
        }
        Ok(Hops { hop, chain })
    }

    /// The hops an outbound call from `principal` carries: one more, with the principal appended.
    pub fn next(&self, principal: &str) -> Hops {
        let mut chain = self.chain.clone();
        chain.push(principal.to_owned());
        Hops {
            hop: self.hop.saturating_add(1),
            chain,
        }
    }

    /// Whether `principal` already handled the task (a loop).
    pub fn contains(&self, principal: &str) -> bool {
        self.chain.iter().any(|p| p == principal)
    }

    /// The `metadata` entries that carry the hops.
    pub fn metadata(&self) -> Value {
        json!({HOP_KEY: self.hop, CHAIN_KEY: self.chain})
    }
}

// --- counterparts --------------------------------------------------------------------------------

/// A remote A2A agent the workspace registered, as shown (the header never).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct A2aCounterpart {
    pub id: Uuid,
    pub workspace_id: Uuid,
    /// `[a-z0-9_-]{1,64}`; the tool agents call it by is `a2a:<name>`.
    pub name: String,
    pub card_url: String,
    /// The host the card URL names (what the egress rules decide on).
    pub host: String,
    /// The agent card as last fetched; `{}` until one was.
    #[schema(value_type = Object)]
    pub card: Value,
    /// Whether an `Authorization` header is kept for it.
    pub has_auth_header: bool,
    /// Local agents that may call it; empty: any agent the policy allows.
    pub allowed_agents: Vec<Uuid>,
    /// The strictest classification of data that may be sent to it.
    pub max_label: DataLabel,
    /// The external agent principal its inbound calls act as; API tokens are issued to it.
    pub agent_id: Uuid,
    pub principal_id: String,
    #[schema(required = true)]
    pub last_checked_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub last_check_ok: Option<bool>,
    pub last_check_detail: String,
    #[schema(required = true)]
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl A2aCounterpart {
    /// The JSON-RPC endpoint of the remote agent: the card's `url`, else the card URL.
    pub fn endpoint(&self) -> String {
        self.card
            .get("url")
            .and_then(Value::as_str)
            .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
            .map(str::to_owned)
            .unwrap_or_else(|| self.card_url.clone())
    }

    /// The host of the endpoint tasks go to.
    pub fn endpoint_host(&self) -> String {
        reqwest::Url::parse(&self.endpoint())
            .ok()
            .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
            .unwrap_or_else(|| self.host.clone())
    }

    /// Whether the card says the remote agent streams (`tasks/sendSubscribe`).
    pub fn streams(&self) -> bool {
        self.card
            .get("capabilities")
            .and_then(|c| c.get("streaming"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// Whether `agent` may call it: every agent, or one the allow-list names.
    pub fn allows_agent(&self, agent: Option<Uuid>) -> bool {
        self.allowed_agents.is_empty() || agent.is_some_and(|id| self.allowed_agents.contains(&id))
    }

    /// The gateway tool agents call it by.
    pub fn tool_name(&self) -> String {
        format!("{TOOL_PREFIX}{}", self.name)
    }

    /// The gateway tool, as offered to the model.
    pub fn gateway_tool(&self) -> super::mcp::GatewayTool {
        let remote_name = self
            .card
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&self.name);
        let remote_description = self
            .card
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default();
        super::mcp::GatewayTool {
            name: self.tool_name(),
            description: format!(
                "Send a task to the remote A2A agent `{remote_name}` ({}) through the platform's \
                 gateway and wait for its answer. The message is screened for data that may not \
                 leave; the answer's text comes back, its files are kept as artifacts.",
                if remote_description.is_empty() {
                    "no description"
                } else {
                    remote_description
                }
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "message": {"type": "string", "description": "What to ask the remote agent to do."},
                    "session_id": {"type": "string", "description": "An A2A session to continue, if any."}
                },
                "required": ["message"],
                "additionalProperties": false
            }),
        }
    }
}

/// `a2a:<name>` split into the counterpart's name, when `tool` is one.
pub fn counterpart_of_tool(tool: &str) -> Option<&str> {
    tool.strip_prefix(TOOL_PREFIX)
        .filter(|name| is_valid_name(name))
}

/// What a counterpart is registered with.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct A2aCounterpartInput {
    pub name: String,
    /// Where its agent card is (`https://host/.well-known/agent.json`).
    pub card_url: String,
    /// An `Authorization` header value sent with every task, kept encrypted and never shown again.
    #[serde(default)]
    pub auth_header: Option<String>,
    /// Local agents that may call it; empty: any agent the policy allows.
    #[serde(default)]
    pub allowed_agents: Vec<Uuid>,
    #[serde(default = "default_label")]
    pub max_label: DataLabel,
}

fn default_label() -> DataLabel {
    DataLabel::Internal
}

impl A2aCounterpartInput {
    /// Adds every problem with the input to `errors` (whether the host may be
    /// reached is the server's call, made with its settings).
    pub fn check(&self, errors: &mut FieldErrors) {
        if !is_valid_name(self.name.trim()) {
            errors.add(
                "name",
                format!("must be lowercase letters, digits, `_` or `-` (max {NAME_MAX})"),
            );
        }
        if let Err(problem) = super::alert_routing::check_url(self.card_url.trim()) {
            errors.add("card_url", problem);
        }
        if self
            .auth_header
            .as_ref()
            .is_some_and(|h| h.len() > TEXT_MAX || h.chars().any(char::is_control))
        {
            errors.add("auth_header", "must be one line of text");
        }
        if self.allowed_agents.len() > ALLOWED_AGENTS_MAX {
            errors.add(
                "allowed_agents",
                format!("at most {ALLOWED_AGENTS_MAX} agents"),
            );
        }
    }
}

/// A counterpart just registered, with the token its inbound calls present.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RegisteredA2aCounterpart {
    pub counterpart: A2aCounterpart,
    /// The API token issued to the counterpart's principal: `nexc_agt_…`, shown once.
    pub token: super::agent::IssuedAgentApiToken,
}

/// What fetching a counterpart's card found.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct A2aCheck {
    pub ok: bool,
    pub detail: String,
    /// The card, when one was read.
    #[schema(value_type = Object, required = true)]
    pub card: Option<Value>,
    pub latency_ms: u64,
}

/// Checks that a fetched document is an A2A agent card: a name, an http(s)
/// `url` and a `skills` array. Returns the card trimmed to what is kept.
pub fn validate_card(card: &Value) -> Result<Value, String> {
    let object = card.as_object().ok_or("the card is not a JSON object")?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .filter(|n| !n.trim().is_empty())
        .ok_or("the card has no `name`")?;
    let url = object
        .get("url")
        .and_then(Value::as_str)
        .ok_or("the card has no `url`")?;
    super::alert_routing::check_url(url).map_err(|e| format!("the card's `url` {e}"))?;
    let skills = object
        .get("skills")
        .and_then(Value::as_array)
        .ok_or("the card has no `skills` array")?;
    let mut kept = serde_json::Map::new();
    kept.insert(
        "name".into(),
        json!(name.chars().take(200).collect::<String>()),
    );
    kept.insert("url".into(), json!(url));
    if let Some(description) = object.get("description").and_then(Value::as_str) {
        kept.insert(
            "description".into(),
            json!(description.chars().take(2_000).collect::<String>()),
        );
    }
    for key in [
        "version",
        "documentationUrl",
        "provider",
        "capabilities",
        "authentication",
    ] {
        if let Some(value) = object.get(key) {
            kept.insert(key.into(), value.clone());
        }
    }
    for key in ["defaultInputModes", "defaultOutputModes"] {
        if let Some(value) = object.get(key).and_then(Value::as_array) {
            kept.insert(key.into(), json!(value.iter().take(16).collect::<Vec<_>>()));
        }
    }
    kept.insert(
        "skills".into(),
        json!(skills.iter().take(64).collect::<Vec<_>>()),
    );
    let value = Value::Object(kept);
    if value.to_string().len() > 64 * 1024 {
        return Err("the card is larger than 64 KiB".into());
    }
    Ok(value)
}

// --- inbound tasks, as the console lists them ----------------------------------------------------

/// A task an exposed agent received over A2A.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct A2aTask {
    /// The A2A task id (UUID v7).
    pub id: Uuid,
    pub workspace_id: Uuid,
    /// The exposed agent.
    pub agent_id: Uuid,
    /// The counterpart that called, as the principal its token names.
    #[schema(required = true)]
    pub caller_agent_id: Option<Uuid>,
    pub caller_principal: String,
    /// The one-node run that answers it.
    pub run_id: Uuid,
    pub node_id: Uuid,
    #[schema(required = true)]
    pub session_id: Option<String>,
    /// A2A hops before the task (0 for a counterpart's own call).
    pub hop: i32,
    /// The principals the task passed through before.
    pub chain: Vec<String>,
    /// The rule that refused the task, when the firewall did.
    #[schema(required = true)]
    pub refused_rule: Option<String>,
    #[schema(required = true)]
    pub refused_reason: Option<String>,
    /// The task's title (the first line of the message).
    pub title: String,
    /// The A2A state: `submitted`, `working`, `completed`, `canceled` or `failed`.
    pub state: String,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_read_field_by_field() {
        let message = InboundMessage::parse(&json!({
            "role": "user",
            "parts": [
                {"type": "text", "text": "  \nWrite the report\nwith care"},
                {"type": "file", "file": {"name": "a/b/figures.csv", "mimeType": "text/csv", "bytes": "YSxi"}},
                {"type": "data", "data": {"quarter": "Q3"}}
            ]
        }))
        .unwrap();
        assert_eq!(message.title(), "Write the report");
        assert!(
            message
                .text()
                .starts_with("Write the report\nwith care\n\n```json")
        );
        assert!(matches!(
            &message.parts[1],
            Part::File { name, mime, bytes: Some(b), uri: None } if name == "figures.csv" && mime == "text/csv" && b == b"a,b"
        ));
        assert_eq!(
            InboundMessage::parse(&json!({"parts": []}))
                .unwrap_err()
                .code,
            E_INVALID_PARAMS
        );
        assert_eq!(
            InboundMessage::parse(&json!({"parts": [{"type": "video"}]}))
                .unwrap_err()
                .code,
            E_CONTENT_TYPE
        );
        assert_eq!(
            InboundMessage::parse(&json!({"parts": [{"type": "file", "file": {"name": "x"}}]}))
                .unwrap_err()
                .code,
            E_INVALID_PARAMS
        );
        assert_eq!(
            InboundMessage::parse(
                &json!({"parts": [{"type": "text", "text": "x".repeat(MESSAGE_CHARS_MAX + 1)}]})
            )
            .unwrap_err()
            .code,
            E_INVALID_PARAMS
        );
        assert_eq!(
            InboundMessage::parse(&json!({"parts": [{"kind": "text", "text": ""}]}))
                .unwrap()
                .title(),
            "A2A task"
        );
    }

    #[test]
    fn hops_travel_in_the_metadata() {
        assert_eq!(Hops::parse(None).unwrap(), Hops::default());
        let hops = Hops::parse(Some(
            &json!({"nexc.hop": 2, "nexc.chain": ["agent://w/A", "agent://w/B"]}),
        ))
        .unwrap();
        assert_eq!(hops.hop, 2);
        assert!(hops.contains("agent://w/B") && !hops.contains("agent://w/C"));
        let next = hops.next("agent://w/C");
        assert_eq!(next.hop, 3);
        assert_eq!(next.chain.len(), 3);
        assert_eq!(next.metadata()["nexc.chain"][2], "agent://w/C");
        assert!(Hops::parse(Some(&json!({"nexc.hop": 300}))).is_err());
        assert!(Hops::parse(Some(&json!({"nexc.chain": "no"}))).is_err());
        assert!(Hops::parse(Some(&json!({"nexc.chain": [1]}))).is_err());
        let long: Vec<String> = (0..CHAIN_MAX + 1)
            .map(|i| format!("agent://w/{i}"))
            .collect();
        assert!(Hops::parse(Some(&json!({"nexc.chain": long}))).is_err());
    }

    #[test]
    fn states_errors_and_cards() {
        assert_eq!(
            TaskState::of_run(super::super::run::RunStatus::Running),
            TaskState::Working
        );
        assert!(TaskState::Completed.is_final() && !TaskState::Submitted.is_final());
        assert_eq!(TaskState::parse("input-required"), Some(TaskState::Working));
        assert_eq!(TaskState::parse("nope"), None);
        assert_eq!(RpcError::new(E_UNAUTHENTICATED, "x").status, 401);
        assert_eq!(RpcError::new(E_FORBIDDEN, "x").status, 403);
        assert_eq!(RpcError::new(E_AGENT_NOT_FOUND, "x").status, 404);
        assert_eq!(RpcError::new(E_TASK_NOT_FOUND, "x").status, 200);
        assert_eq!(counterpart_of_tool("a2a:stub"), Some("stub"));
        assert_eq!(counterpart_of_tool("a2a:Bad Name"), None);
        assert_eq!(counterpart_of_tool("mcp:x/y"), None);
        let card = validate_card(&json!({
            "name": "Remote", "url": "https://remote.example/a2a", "skills": [{"id": "s"}],
            "capabilities": {"streaming": true}, "extra": "dropped"
        }))
        .unwrap();
        assert!(card.get("extra").is_none());
        assert_eq!(card["capabilities"]["streaming"], true);
        assert!(validate_card(&json!({"name": "x", "url": "ftp://x", "skills": []})).is_err());
        assert!(validate_card(&json!({"name": "x", "url": "https://x.example"})).is_err());
        assert!(validate_card(&json!([])).is_err());
    }

    #[test]
    fn inputs_are_checked() {
        let good = A2aCounterpartInput {
            name: "stub".into(),
            card_url: "https://remote.example/.well-known/agent.json".into(),
            auth_header: Some("Bearer x".into()),
            allowed_agents: vec![],
            max_label: DataLabel::Internal,
        };
        let mut errors = FieldErrors::default();
        good.check(&mut errors);
        assert!(errors.is_empty(), "{:?}", errors.as_map());
        let bad = A2aCounterpartInput {
            name: "Bad Name".into(),
            card_url: "ftp://nope".into(),
            auth_header: Some("a\nb".into()),
            allowed_agents: vec![Uuid::nil(); ALLOWED_AGENTS_MAX + 1],
            max_label: DataLabel::Public,
        };
        let mut errors = FieldErrors::default();
        bad.check(&mut errors);
        let fields: Vec<&String> = errors.as_map().keys().collect();
        assert_eq!(
            fields,
            ["allowed_agents", "auth_header", "card_url", "name"]
        );
    }
}
