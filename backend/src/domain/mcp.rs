//! MCP servers a workspace registers (the tools gateway, AgentSecOps phase
//! 4): the backend connects to them on an agent's behalf, lists their tools
//! and offers them to agents as `mcp:<server>/<tool>` under the policy. The
//! runtime never talks to a server: every call goes up the stream as a tool
//! request and the backend performs it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use super::capabilities::DataLabel;
use super::string_enum;
use super::validation::FieldErrors;

/// Longest server name.
pub const NAME_MAX: usize = 64;
/// Longest command, URL or argument.
pub const TEXT_MAX: usize = 2_000;
/// Most arguments of a stdio command.
pub const ARGS_MAX: usize = 32;
/// Most tools one allow-list names.
pub const ALLOWED_TOOLS_MAX: usize = 128;
/// Longest tool name a server may publish that is still offered.
pub const TOOL_NAME_MAX: usize = 100;
/// Prefix of a gateway tool of an MCP server.
pub const TOOL_PREFIX: &str = "mcp:";
/// The built-in gateway tool that fetches a URL.
pub const FETCH_URL: &str = "fetch_url";

string_enum!(
    /// How the backend reaches a server.
    McpTransport {
        /// A child process the backend starts for each call, JSON-RPC over its stdin and stdout.
        Stdio => "stdio",
        /// Streamable HTTP: JSON-RPC posted to the URL.
        Http => "http",
    }
);

/// A tool as a server lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// JSON Schema of the arguments.
    #[serde(default = "empty_schema")]
    #[schema(value_type = Object)]
    pub input_schema: Value,
}

fn empty_schema() -> Value {
    serde_json::json!({"type": "object", "properties": {}})
}

/// A registered server, as shown (the auth header never).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct McpServer {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub transport: McpTransport,
    #[schema(required = true)]
    pub command: Option<String>,
    pub args: Vec<String>,
    #[schema(required = true)]
    pub url: Option<String>,
    /// Whether an `Authorization` header is kept for the server.
    pub has_auth_header: bool,
    /// Tools agents may call; empty means every tool the server lists.
    pub allowed_tools: Vec<String>,
    /// The strictest classification of data that may be sent to the server.
    pub max_label: DataLabel,
    /// The tools the server listed at the last connection test.
    pub tools: Vec<McpTool>,
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

impl McpServer {
    /// Whether agents may call `tool` of this server.
    pub fn allows_tool(&self, tool: &str) -> bool {
        self.allowed_tools.is_empty() || self.allowed_tools.iter().any(|t| t == tool)
    }

    /// The host an egress span names for this server: the URL's host, or
    /// `local:<name>` for a child process.
    pub fn host(&self) -> String {
        match (self.transport, &self.url) {
            (McpTransport::Http, Some(url)) => reqwest::Url::parse(url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_owned))
                .unwrap_or_else(|| url.clone()),
            _ => format!("local:{}", self.name),
        }
    }

    /// The gateway tools of this server (`mcp:<name>/<tool>`), as offered to agents.
    pub fn gateway_tools(&self) -> Vec<GatewayTool> {
        self.tools
            .iter()
            .filter(|t| self.allows_tool(&t.name))
            .map(|t| GatewayTool {
                name: format!("{TOOL_PREFIX}{}/{}", self.name, t.name),
                description: t.description.clone(),
                input_schema: t.input_schema.clone(),
            })
            .collect()
    }
}

/// A tool the backend performs for an agent, as the runtime offers it to the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct GatewayTool {
    pub name: String,
    pub description: String,
    #[schema(value_type = Object)]
    pub input_schema: Value,
}

impl GatewayTool {
    /// The built-in `fetch_url`.
    pub fn fetch_url() -> Self {
        GatewayTool {
            name: FETCH_URL.into(),
            description: "Fetch a web page or an API response over HTTP(S) GET through the \
                platform's gateway. Only hosts the agent's policy allows are reached; the \
                response is limited in size and to text, JSON, XML and HTML."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "The http(s) URL to fetch."}
                },
                "required": ["url"],
                "additionalProperties": false
            }),
        }
    }
}

/// `mcp:<server>/<tool>` split into its parts, when `name` is one.
pub fn split_tool_name(name: &str) -> Option<(&str, &str)> {
    let rest = name.strip_prefix(TOOL_PREFIX)?;
    let (server, tool) = rest.split_once('/')?;
    (is_valid_name(server) && !tool.is_empty() && tool.len() <= TOOL_NAME_MAX)
        .then_some((server, tool))
}

/// Whether `name` may stand in a tool list of the capabilities or a policy:
/// a plain tool name (`write_file`, `fetch_url`), a gateway tool
/// (`mcp:<server>/<tool>`) or every tool of a server (`mcp:<server>/*`).
pub fn is_tool_entry(name: &str) -> bool {
    fn plain(name: &str) -> bool {
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
    }
    match name.strip_prefix(TOOL_PREFIX) {
        None => plain(name),
        Some(rest) => match rest.split_once('/') {
            Some((server, tool)) => {
                is_valid_name(server)
                    && (tool == "*" || (plain(tool) && tool.len() <= TOOL_NAME_MAX))
            }
            None => false,
        },
    }
}

/// Whether a server name fits: lowercase letters, digits, `_` and `-`.
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME_MAX
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// The fields a server is registered with.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct McpServerInput {
    pub name: String,
    pub transport: McpTransport,
    /// `stdio`: the executable to start.
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    /// `http`: where the server listens.
    #[serde(default)]
    pub url: Option<String>,
    /// `http`: an `Authorization` header value, kept encrypted and never shown again.
    #[serde(default)]
    pub auth_header: Option<String>,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default = "default_label")]
    pub max_label: DataLabel,
}

fn default_label() -> DataLabel {
    DataLabel::Internal
}

impl McpServerInput {
    /// Adds every problem with the input to `errors`.
    pub fn check(&self, errors: &mut FieldErrors) {
        if !is_valid_name(self.name.trim()) {
            errors.add(
                "name",
                format!("must be lowercase letters, digits, `_` or `-` (max {NAME_MAX})"),
            );
        }
        match self.transport {
            McpTransport::Stdio => {
                match self.command.as_deref().map(str::trim) {
                    Some(command) if !command.is_empty() && command.len() <= TEXT_MAX => {}
                    _ => errors.add("command", "names the executable of a stdio server"),
                }
                if self.url.is_some() {
                    errors.add("url", "a stdio server has no URL");
                }
                if self.auth_header.is_some() {
                    errors.add("auth_header", "a stdio server takes no header");
                }
            }
            McpTransport::Http => {
                match self.url.as_deref().map(str::trim) {
                    Some(url)
                        if url.len() <= TEXT_MAX
                            && reqwest::Url::parse(url).is_ok_and(|u| {
                                matches!(u.scheme(), "http" | "https") && u.host_str().is_some()
                            }) => {}
                    _ => errors.add("url", "must be an http(s) URL"),
                }
                if self.command.is_some() || !self.args.is_empty() {
                    errors.add("command", "an HTTP server has no command");
                }
                if self
                    .auth_header
                    .as_ref()
                    .is_some_and(|h| h.len() > TEXT_MAX || h.chars().any(char::is_control))
                {
                    errors.add("auth_header", "must be one line of text");
                }
            }
        }
        if self.args.len() > ARGS_MAX || self.args.iter().any(|a| a.len() > TEXT_MAX) {
            errors.add(
                "args",
                format!("at most {ARGS_MAX} arguments of {TEXT_MAX} characters"),
            );
        }
        if self.allowed_tools.len() > ALLOWED_TOOLS_MAX
            || self
                .allowed_tools
                .iter()
                .any(|t| t.trim().is_empty() || t.len() > TOOL_NAME_MAX)
        {
            errors.add(
                "allowed_tools",
                format!("at most {ALLOWED_TOOLS_MAX} tool names of {TOOL_NAME_MAX} characters"),
            );
        }
    }
}

/// What a connection test found.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct McpCheck {
    pub ok: bool,
    pub detail: String,
    pub tools: Vec<McpTool>,
    /// How long the server took to answer `initialize` and `tools/list`.
    pub latency_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_split_and_server_names_are_checked() {
        assert_eq!(
            split_tool_name("mcp:github/search_issues"),
            Some(("github", "search_issues"))
        );
        assert_eq!(split_tool_name("fetch_url"), None);
        assert_eq!(split_tool_name("mcp:GitHub/x"), None);
        assert_eq!(split_tool_name("mcp:github/"), None);
        assert!(is_valid_name("my-server_2"));
        assert!(!is_valid_name("My Server"));
        assert!(is_tool_entry("write_file") && is_tool_entry("fetch_url"));
        assert!(is_tool_entry("mcp:github/search") && is_tool_entry("mcp:github/*"));
        assert!(!is_tool_entry("mcp:github") && !is_tool_entry("mcp:Git Hub/x"));
        assert!(!is_tool_entry("write file") && !is_tool_entry(""));
        assert!(!is_valid_name(&"a".repeat(65)));
        let fetch = GatewayTool::fetch_url();
        assert_eq!(fetch.name, FETCH_URL);
        assert_eq!(fetch.input_schema["required"][0], "url");
    }

    #[test]
    fn inputs_are_checked_per_transport() {
        let stdio = McpServerInput {
            name: "files".into(),
            transport: McpTransport::Stdio,
            command: Some("python3".into()),
            args: vec!["-I".into(), "server.py".into()],
            url: None,
            auth_header: None,
            allowed_tools: vec![],
            max_label: DataLabel::Internal,
        };
        let mut errors = FieldErrors::default();
        stdio.check(&mut errors);
        assert!(errors.is_empty(), "{:?}", errors.as_map());
        let bad = McpServerInput {
            name: "Bad Name".into(),
            transport: McpTransport::Http,
            command: Some("x".into()),
            args: vec![],
            url: Some("ftp://nope".into()),
            auth_header: Some("a\nb".into()),
            allowed_tools: vec!["".into()],
            max_label: DataLabel::Public,
        };
        let mut errors = FieldErrors::default();
        bad.check(&mut errors);
        let fields: Vec<&String> = errors.as_map().keys().collect();
        assert_eq!(
            fields,
            ["allowed_tools", "auth_header", "command", "name", "url"]
        );
    }

    #[test]
    fn a_server_offers_its_allowed_tools_under_its_name() {
        let server = McpServer {
            id: Uuid::now_v7(),
            workspace_id: Uuid::now_v7(),
            name: "github".into(),
            transport: McpTransport::Http,
            command: None,
            args: vec![],
            url: Some("https://mcp.github.example/v1".into()),
            has_auth_header: true,
            allowed_tools: vec!["search".into()],
            max_label: DataLabel::Internal,
            tools: vec![
                McpTool {
                    name: "search".into(),
                    description: "Search".into(),
                    input_schema: empty_schema(),
                },
                McpTool {
                    name: "delete_repo".into(),
                    description: "".into(),
                    input_schema: empty_schema(),
                },
            ],
            last_checked_at: None,
            last_check_ok: None,
            last_check_detail: String::new(),
            created_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let offered: Vec<String> = server.gateway_tools().into_iter().map(|t| t.name).collect();
        assert_eq!(offered, ["mcp:github/search"]);
        assert_eq!(server.host(), "mcp.github.example");
        assert!(server.allows_tool("search") && !server.allows_tool("delete_repo"));
    }
}
