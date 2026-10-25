//! HTTP layer: handlers, extractors, problem+json errors, middleware and
//! the OpenAPI document.
#![forbid(unsafe_code)]

pub mod extract;
pub mod handlers;
pub mod middleware;
pub mod problem;

use axum::extract::DefaultBodyLimit;
use utoipa::openapi::security::{Http, HttpAuthScheme, SecurityScheme};
use utoipa::{Modify, OpenApi};
use utoipa_axum::router::{OpenApiRouter, UtoipaMethodRouterExt};
use utoipa_axum::routes;

use crate::app::AppState;
use crate::domain::knowledge::DOCUMENT_MAX_BYTES;
use handlers::*;

/// Room for a request's framing on top of the largest document.
pub const UPLOAD_SLACK: usize = 64 * 1024;

/// API description; paths are collected from the routers below.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "nexc-engine API",
        description = "Graph engine for LLM-planned, agent-executed task graphs. \
            Errors are RFC 7807 `application/problem+json`. Realtime: SSE `/graphs/{gid}/events` \
            and WebSocket `/graphs/{gid}/ws` authenticated with single-use tickets.",
        license(name = "MIT", identifier = "MIT")
    ),
    modifiers(&BearerAuth),
    components(schemas(
        problem::Problem,
        crate::realtime::events::SseEvent,
        crate::realtime::events::WsMessage,
        // Only used as a query parameter, which does not register a schema by itself.
        handlers::exports::ExportFormat,
    )),
    tags(
        (name = "auth", description = "Accounts and sessions"),
        (name = "settings", description = "Per-user LLM settings"),
        (name = "workspaces", description = "Organisations, their members and invitations"),
        (name = "teams", description = "Teams of a workspace"),
        (name = "issues", description = "Issues, workflow states and projects"),
        (name = "graphs", description = "Graphs, dependency detection and analysis"),
        (name = "nodes"), (name = "edges"),
        (name = "plans", description = "LLM planning"),
        (name = "runs", description = "Execution"),
        (name = "artifacts"),
        (name = "artifact-files", description = "Text files of a workspace (code, HTML, Markdown) kept with every version, in folders, on issues; and keeping what runs produced"),
        (name = "templates"), (name = "agents"), (name = "memories"),
        (name = "mcp-servers", description = "MCP servers of a workspace, called through the tools gateway"),
        (name = "policies", description = "Versioned policies and what they resolve to"),
        (name = "approvals", description = "Tool calls that wait for a human"),
        (name = "alerts", description = "What needs a look"),
        (name = "alert-destinations", description = "Where a workspace's alerts and approvals are sent: signed webhooks and Slack"),
        (name = "planner", description = "The Planner: queue, running work, agents and the backends it routes nodes to"), (name = "realtime"), (name = "health"),
        (name = "audit", description = "Ask the audit: questions about what agents did, answered from onto-intelli's facts"),
        (name = "admin", description = "The platform console: every workspace and account, for platform administrators only"),
        (name = "secops", description = "The AgentSecOps console: the workspace's agents, runs, denials, approvals, alerts and risk on one screen"),
        (name = "a2a", description = "The A2A adapter: exposed agents' cards and JSON-RPC endpoint, registered counterparts, inbound tasks"),
    )
)]
pub struct ApiDoc;

struct BearerAuth;

impl Modify for BearerAuth {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        let scheme =
            utoipa::openapi::security::HttpBuilder::from(Http::new(HttpAuthScheme::Bearer))
                .bearer_format("JWT")
                .build();
        components.add_security_scheme("bearer", SecurityScheme::Http(scheme));
    }
}

/// Routes under `/api/v1`.
fn v1_routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(health::healthz))
        .routes(routes!(health::readyz))
        .routes(routes!(agent_token_keys::jwks))
        .routes(routes!(auth::register))
        .routes(routes!(auth::login))
        .routes(routes!(auth::refresh))
        .routes(routes!(auth::logout))
        .routes(routes!(sso::providers))
        .routes(routes!(sso::identities))
        .routes(routes!(sso::oidc_login))
        .routes(routes!(sso::oidc_callback))
        .routes(routes!(sso::oidc_link))
        .routes(routes!(auth::me, account::update_me))
        .routes(routes!(account::change_password))
        .routes(routes!(account::reset_password))
        .routes(routes!(account::sessions))
        .routes(routes!(account::activity))
        .routes(routes!(account::two_factor_status))
        .routes(routes!(account::two_factor_setup))
        .routes(routes!(account::two_factor_enable))
        .routes(routes!(account::two_factor_disable))
        .routes(routes!(account::end_all_sessions))
        .routes(routes!(account::export))
        .routes(routes!(account::delete_me))
        .routes(routes!(
            settings::get_llm,
            settings::put_llm,
            settings::delete_llm
        ))
        .routes(routes!(settings::llm_models))
        .routes(routes!(
            settings::get_workspace_llm,
            settings::put_workspace_llm,
            settings::delete_workspace_llm
        ))
        .routes(routes!(workspaces::list, workspaces::create))
        .routes(routes!(
            workspaces::get,
            workspaces::update,
            workspaces::delete
        ))
        .routes(routes!(workspaces::members, workspaces::invite))
        .routes(routes!(
            workspaces::update_member,
            workspaces::remove_member
        ))
        .routes(routes!(workspaces::invites))
        .routes(routes!(workspaces::usage))
        .routes(routes!(workspaces::guardrails, workspaces::put_guardrails))
        .routes(routes!(workspaces::assistant))
        .routes(routes!(workspaces::assistant_conversations))
        .routes(routes!(
            workspaces::assistant_conversation,
            workspaces::delete_assistant_conversation
        ))
        .routes(routes!(workspaces::audit_log))
        .routes(routes!(evidence::audit_verify))
        .routes(routes!(evidence::run_evidence))
        .routes(routes!(audit::ask))
        .routes(routes!(audit::ask_stream))
        .routes(routes!(audit::questions))
        .routes(routes!(policies::list, policies::create))
        .routes(routes!(policies::resolve))
        .routes(routes!(approvals::list))
        .routes(routes!(approvals::get))
        .routes(routes!(approvals::approve))
        .routes(routes!(approvals::deny))
        .routes(routes!(alerts::list))
        .routes(routes!(alerts::acknowledge))
        .routes(routes!(exports::alerts))
        .routes(routes!(exports::audit))
        .routes(routes!(
            alert_destinations::list,
            alert_destinations::create
        ))
        .routes(routes!(
            alert_destinations::get,
            alert_destinations::update,
            alert_destinations::delete
        ))
        .routes(routes!(alert_destinations::send_test))
        .routes(routes!(alert_destinations::deliveries))
        .routes(routes!(secops::overview))
        .routes(routes!(secops::spans))
        .routes(routes!(secops::events))
        .routes(routes!(artifacts::release))
        .routes(routes!(search::search))
        .routes(routes!(platform::workspaces))
        .routes(routes!(platform::workspace))
        .routes(routes!(platform::delete_workspace))
        .routes(routes!(platform::assign_owner))
        .routes(routes!(platform::users))
        .routes(routes!(platform::update_user))
        .routes(routes!(platform::erase_user))
        .routes(routes!(platform::issue_password_reset))
        .routes(routes!(platform::user_identities))
        .routes(routes!(platform::unlink_identity))
        .routes(routes!(platform::events))
        .routes(routes!(infrastructure::status))
        .routes(routes!(infrastructure::check))
        .routes(routes!(latency::report))
        .routes(routes!(transfer::list, transfer::start))
        .routes(routes!(transfer::check))
        .routes(routes!(insight::timeline))
        .routes(routes!(insight::day_summary, insight::write_day_summary))
        .routes(routes!(insight::days))
        .routes(routes!(ontology::status))
        .routes(routes!(ontology::session))
        .routes(routes!(workspaces::delete_invite))
        .routes(routes!(teams::list, teams::create))
        .routes(routes!(teams::get, teams::update, teams::delete))
        .routes(routes!(teams::members))
        .routes(routes!(teams::set_member, teams::remove_member))
        .routes(routes!(issues::list))
        .routes(routes!(issues::create))
        .routes(routes!(issues::get, issues::update, issues::delete))
        .routes(routes!(issues::create_graph))
        .routes(routes!(issues::labels, issues::create_label))
        .routes(routes!(issues::update_label, issues::delete_label))
        .routes(routes!(cycles::list, cycles::create))
        .routes(routes!(cycles::update, cycles::delete))
        .routes(routes!(issues::events))
        .routes(routes!(issues::inbox))
        .routes(routes!(issues::mark_read))
        .routes(routes!(issues::create_comment))
        .routes(routes!(issues::update_comment, issues::delete_comment))
        .routes(routes!(issues::states, issues::create_state))
        .routes(routes!(issues::update_state, issues::delete_state))
        .routes(routes!(issues::projects, issues::create_project))
        .routes(routes!(issues::update_project, issues::delete_project))
        .routes(routes!(
            artifact_files::list_folders,
            artifact_files::create_folder
        ))
        .routes(routes!(
            artifact_files::update_folder,
            artifact_files::delete_folder
        ))
        // The body of an upload or a new version is the file itself; the request body
        // limit (1 MiB) is also the largest version, so these routes need no limit of their own.
        .routes(routes!(
            artifact_files::list_files,
            artifact_files::create_file
        ))
        .routes(routes!(
            artifact_files::get_file,
            artifact_files::update_file,
            artifact_files::delete_file
        ))
        .routes(routes!(artifact_files::versions))
        .routes(routes!(
            artifact_files::content,
            artifact_files::update_content
        ))
        .routes(routes!(artifact_files::raw))
        .routes(routes!(artifact_files::duplicate))
        .routes(routes!(artifact_files::run_artifacts))
        .routes(routes!(artifact_files::keep))
        .routes(routes!(graphs::list, graphs::create))
        .routes(routes!(graphs::get, graphs::update, graphs::delete))
        .routes(routes!(graphs::replace_ontology))
        .routes(routes!(graphs::suggestions))
        .routes(routes!(graphs::analysis))
        .routes(routes!(templates::list))
        .routes(routes!(templates::instantiate))
        .routes(routes!(nodes::create))
        .routes(routes!(nodes::update, nodes::delete))
        .routes(routes!(edges::create))
        .routes(routes!(edges::update, edges::delete))
        .routes(routes!(plans::create))
        .routes(routes!(plans::get))
        .routes(routes!(plans::apply))
        .routes(routes!(runs::create, runs::list))
        .routes(routes!(runs::get))
        .routes(routes!(runs::cancel))
        .routes(routes!(runs::cancel_agent))
        .routes(routes!(trace::trace))
        .routes(routes!(trace::verify))
        .routes(routes!(sessions::tree))
        .routes(routes!(sessions::list))
        .routes(routes!(payloads::payload))
        .routes(routes!(artifacts::list))
        .routes(routes!(artifacts::zip))
        .routes(routes!(artifacts::download))
        .routes(routes!(agents::list, agents::create))
        .routes(routes!(agents::get, agents::update, agents::delete))
        .routes(routes!(agents::activity))
        .routes(routes!(agents::quarantine))
        .routes(routes!(agents::release))
        .routes(routes!(agent_templates::list))
        .routes(routes!(agent_templates::instantiate))
        .routes(routes!(agent_templates::evaluate))
        .routes(routes!(skills::list))
        .routes(routes!(skills::get))
        .routes(routes!(agents::tokens, agents::issue_token))
        .routes(routes!(agents::revoke_token))
        .routes(routes!(a2a::tasks))
        .routes(routes!(mcp_servers::list, mcp_servers::create))
        .routes(routes!(mcp_servers::get, mcp_servers::delete))
        .routes(routes!(mcp_servers::check))
        .routes(routes!(a2a_counterparts::list, a2a_counterparts::create))
        .routes(routes!(a2a_counterparts::get, a2a_counterparts::delete))
        .routes(routes!(a2a_counterparts::check))
        .routes(
            routes!(knowledge::upload)
                .layer(DefaultBodyLimit::max(DOCUMENT_MAX_BYTES + UPLOAD_SLACK)),
        )
        .routes(routes!(knowledge::list))
        .routes(routes!(
            knowledge::get,
            knowledge::set_label,
            knowledge::delete
        ))
        .routes(routes!(knowledge::search))
        .routes(routes!(knowledge::passage))
        .routes(routes!(knowledge::topics))
        .routes(routes!(knowledge::set_topic_classification))
        .routes(routes!(knowledge::rebuild_topics))
        .routes(routes!(knowledge::settings, knowledge::put_settings))
        .routes(routes!(memories::list))
        .routes(routes!(memories::topics))
        .routes(routes!(memories::rebuild_topics))
        .routes(routes!(
            memories::get,
            memories::set_label,
            memories::delete
        ))
        .routes(routes!(planner::status))
        .routes(routes!(realtime::ticket))
        .routes(routes!(realtime::events))
        .routes(routes!(realtime::websocket))
        .merge(deprecated_routes())
}

/// Former addresses that keep working, documented as deprecated.
#[allow(deprecated)]
fn deprecated_routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(planner::status_legacy))
}

/// Every documented route: `/api/v1/*`, `/metrics`, the signing keys at
/// `/.well-known/jwks.json`, and the A2A surface at the root (the well-known
/// card and the JSON-RPC endpoint live where the protocol expects them,
/// outside the console's API).
pub fn api_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .nest("/api/v1", v1_routes())
        .routes(routes!(health::metrics))
        .routes(routes!(evidence::jwks))
        .routes(routes!(a2a::well_known))
        .routes(routes!(a2a::card))
        .routes(routes!(a2a::rpc).layer(DefaultBodyLimit::max(a2a::RPC_BODY_LIMIT)))
        .routes(routes!(a2a::artifact))
}

/// The OpenAPI document (also served at `/api/openapi.json`).
pub fn openapi() -> utoipa::openapi::OpenApi {
    api_router().split_for_parts().1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_lists_contract_paths() {
        let spec = serde_json::to_value(openapi()).unwrap();
        let paths = spec["paths"].as_object().unwrap();
        for p in [
            "/api/v1/auth/register",
            "/api/v1/auth/oidc/login",
            "/api/v1/auth/oidc/callback",
            "/api/v1/auth/oidc/link",
            "/api/v1/auth/providers",
            "/api/v1/auth/identities",
            "/api/v1/admin/users/{uid}/identities",
            "/api/v1/admin/users/{uid}/identities/{iid}",
            "/api/v1/graphs/{gid}",
            "/api/v1/graphs/{gid}/plans/{pid}/apply",
            "/api/v1/runs/{rid}/artifacts.zip",
            "/api/v1/graphs/from-template",
            "/api/v1/graphs/{gid}/events",
            "/api/v1/runs/{rid}/trace",
            "/api/v1/runs/{rid}/trace/verify",
            "/api/v1/runs/{rid}/evidence",
            "/api/v1/workspaces/{wid}/audit/verify",
            "/.well-known/jwks.json",
            "/api/v1/runs/{rid}/sessions",
            "/api/v1/runs/{rid}/agents/{sid}/cancel",
            "/api/v1/spans/{sid}/payload",
            "/api/v1/workspaces/{wid}/sessions",
            "/api/v1/agents/{id}/activity",
            "/api/v1/agents/{id}/quarantine",
            "/api/v1/agents/{id}/release",
            "/api/v1/workspaces/{wid}/audit/ask",
            "/api/v1/workspaces/{wid}/audit/ask/stream",
            "/api/v1/workspaces/{wid}/audit/questions",
            "/api/v1/workspaces/{wid}/policies",
            "/api/v1/workspaces/{wid}/policies/resolve",
            "/api/v1/approvals/{id}/approve",
            "/api/v1/approvals/{id}/deny",
            "/api/v1/artifacts/{aid}/release",
            "/api/v1/workspaces/{wid}/artifact-folders",
            "/api/v1/workspaces/{wid}/artifact-folders/{fid}",
            "/api/v1/workspaces/{wid}/artifact-files",
            "/api/v1/artifact-files/{id}",
            "/api/v1/artifact-files/{id}/versions",
            "/api/v1/artifact-files/{id}/content",
            "/api/v1/artifact-files/{id}/raw",
            "/api/v1/artifact-files/{id}/duplicate",
            "/api/v1/issues/{iid}/run-artifacts",
            "/api/v1/issues/{iid}/run-artifacts/{aid}/keep",
            "/api/v1/workspaces/{wid}/alerts",
            "/api/v1/workspaces/{wid}/alerts/export",
            "/api/v1/workspaces/{wid}/audit/export",
            "/api/v1/workspaces/{wid}/alert-destinations",
            "/api/v1/workspaces/{wid}/alert-destinations/{id}",
            "/api/v1/workspaces/{wid}/alert-destinations/{id}/test",
            "/api/v1/workspaces/{wid}/alert-destinations/{id}/deliveries",
            "/api/v1/workspaces/{wid}/secops/overview",
            "/api/v1/workspaces/{wid}/secops/spans",
            "/api/v1/workspaces/{wid}/secops/events",
            "/api/v1/agent-templates",
            "/api/v1/agents/from-template/{name}",
            "/api/v1/workspaces/{wid}/mcp-servers",
            "/api/v1/workspaces/{wid}/mcp-servers/{sid}/check",
            "/api/v1/agents/{id}/tokens",
            "/api/v1/agent-tokens/jwks.json",
            "/api/v1/agents/{id}/tokens/{tid}",
            "/api/v1/agents/{id}/a2a/tasks",
            "/api/v1/workspaces/{wid}/a2a-counterparts",
            "/api/v1/workspaces/{wid}/a2a-counterparts/{cid}/check",
            "/.well-known/agent.json",
            "/a2a/agents/{principal}/card",
            "/a2a/agents/{principal}",
            "/a2a/tasks/{tid}/artifacts/{aid}",
            "/metrics",
            "/api/v1/admin/latency",
        ] {
            assert!(paths.contains_key(p), "missing {p}");
        }
        let node = &spec["components"]["schemas"]["GraphNode"]["properties"];
        assert!(node["agent_role"].is_object() && node["executor"].is_object());
        assert!(spec["components"]["schemas"]["ProposedNode"]["properties"]["ref"].is_object());
        assert!(spec["components"]["securitySchemes"]["bearer"].is_object());
    }

    /// Every `$ref` of the spec points at a schema it defines: a dangling one
    /// breaks the generated client types.
    #[test]
    fn spec_references_resolve() {
        fn collect(value: &serde_json::Value, refs: &mut Vec<String>) {
            match value {
                serde_json::Value::Object(map) => {
                    for (key, inner) in map {
                        match (key.as_str(), inner.as_str()) {
                            ("$ref", Some(target)) => refs.push(target.to_owned()),
                            _ => collect(inner, refs),
                        }
                    }
                }
                serde_json::Value::Array(items) => items.iter().for_each(|i| collect(i, refs)),
                _ => {}
            }
        }
        let spec = serde_json::to_value(openapi()).unwrap();
        let mut refs = Vec::new();
        collect(&spec, &mut refs);
        assert!(refs.len() > 100, "the spec was walked");
        let schemas = spec["components"]["schemas"].as_object().unwrap();
        let dangling: Vec<&String> = refs
            .iter()
            .filter(|r| {
                r.strip_prefix("#/components/schemas/")
                    .is_none_or(|name| !schemas.contains_key(name))
            })
            .collect();
        assert!(dangling.is_empty(), "unresolved references: {dangling:?}");
    }

    /// Every enum value the spec documents must be a value the API really
    /// accepts and emits; generated client types depend on it.
    #[test]
    fn spec_enum_values_match_the_wire_format() {
        use crate::domain::{
            account, agent, alert, alert_routing, approval, artifact_file, audit, capabilities,
            cycle, evidence, graph, issue, knowledge, memory, ontology, plan, platform,
            policy_document, recording, run, search, session, settings, trace, usage, user,
            workspace,
        };
        use serde_json::Value;

        fn check<T: serde::de::DeserializeOwned + serde::Serialize>(values: &[Value]) {
            for v in values {
                let parsed: T = serde_json::from_value(v.clone())
                    .unwrap_or_else(|_| panic!("spec value {v} is not accepted on the wire"));
                assert_eq!(&serde_json::to_value(parsed).unwrap(), v);
            }
        }

        let spec = serde_json::to_value(openapi()).unwrap();
        let schemas = spec["components"]["schemas"].as_object().unwrap();
        let mut checked = 0;
        for (name, schema) in schemas {
            let Some(values) = schema["enum"].as_array() else {
                continue;
            };
            match name.as_str() {
                "Role" => check::<user::Role>(values),
                "NodeStatus" => check::<graph::NodeStatus>(values),
                "Executor" => check::<graph::Executor>(values),
                "NodeOrigin" => check::<graph::NodeOrigin>(values),
                "EdgeOrigin" => check::<graph::EdgeOrigin>(values),
                "RunStatus" => check::<run::RunStatus>(values),
                "WorkspaceRole" => check::<workspace::WorkspaceRole>(values),
                "TeamRole" => check::<workspace::TeamRole>(values),
                "MemoryKind" => check::<memory::MemoryKind>(values),
                "MemoryScope" => check::<memory::MemoryScope>(values),
                "PlanStatus" => check::<plan::PlanStatus>(values),
                "LlmProviderKind" => check::<settings::LlmProviderKind>(values),
                "KeySource" => check::<settings::KeySource>(values),
                "ConfigScope" => check::<settings::ConfigScope>(values),
                "UsagePurpose" => check::<usage::UsagePurpose>(values),
                "StateCategory" => check::<issue::StateCategory>(values),
                "ProjectStatus" => check::<issue::ProjectStatus>(values),
                "IssueEventKind" => check::<issue::IssueEventKind>(values),
                "AuditAction" => check::<audit::AuditAction>(values),
                "CycleStatus" => check::<cycle::CycleStatus>(values),
                "DocumentStatus" => check::<knowledge::DocumentStatus>(values),
                "ChunkKind" => check::<knowledge::ChunkKind>(values),
                "NotificationKind" => check::<issue::NotificationKind>(values),
                "UsageScope" => check::<usage::UsageScope>(values),
                "AgentStatus" => check::<agent::AgentStatus>(values),
                "AgentRuntime" => check::<agent::AgentRuntime>(values),
                "PlatformAction" => check::<platform::PlatformAction>(values),
                "OntoMode" => check::<ontology::OntoMode>(values),
                "ProposalStatus" => check::<ontology::ProposalStatus>(values),
                "SearchKind" => check::<search::SearchKind>(values),
                "AccountEventKind" => check::<account::AccountEventKind>(values),
                "SpanKind" => check::<trace::SpanKind>(values),
                "SpanStatus" => check::<trace::SpanStatus>(values),
                "SessionStatus" => check::<session::SessionStatus>(values),
                "PayloadKind" => check::<recording::PayloadKind>(values),
                "DataLabel" => check::<capabilities::DataLabel>(values),
                "AnswerMode" => check::<crate::ontology::ask::AnswerMode>(values),
                "PolicyScope" => check::<policy_document::PolicyScope>(values),
                "RuleAction" => check::<policy_document::RuleAction>(values),
                "Severity" => check::<policy_document::Severity>(values),
                "PiiKind" => check::<policy_document::PiiKind>(values),
                "InjectionAction" => check::<policy_document::InjectionAction>(values),
                "EvidenceAction" => check::<policy_document::EvidenceAction>(values),
                "EvidenceVerdict" => check::<evidence::EvidenceVerdict>(values),
                "ApprovalStatus" => check::<approval::ApprovalStatus>(values),
                "AlertKind" => check::<alert::AlertKind>(values),
                "McpTransport" => check::<crate::domain::mcp::McpTransport>(values),
                "DestinationKind" => check::<alert_routing::DestinationKind>(values),
                "DeliveryStatus" => check::<alert_routing::DeliveryStatus>(values),
                "DeliveryEvent" => check::<alert_routing::DeliveryEvent>(values),
                "ExportFormat" => check::<handlers::exports::ExportFormat>(values),
                "ArtifactKind" => check::<artifact_file::ArtifactKind>(values),
                _ => continue,
            }
            checked += 1;
        }
        assert_eq!(checked, 51, "every string enum schema is covered");
    }
}
