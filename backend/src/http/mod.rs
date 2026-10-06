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
        license(name = "Apache-2.0", identifier = "Apache-2.0")
    ),
    modifiers(&BearerAuth),
    components(schemas(
        problem::Problem,
        crate::realtime::events::SseEvent,
        crate::realtime::events::WsMessage,
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
        (name = "artifacts"), (name = "templates"), (name = "agents"), (name = "memories"),
        (name = "orchestrator"), (name = "realtime"), (name = "health"),
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
        .routes(routes!(auth::register))
        .routes(routes!(auth::login))
        .routes(routes!(auth::refresh))
        .routes(routes!(auth::logout))
        .routes(routes!(auth::me))
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
        .routes(routes!(workspaces::audit_log))
        .routes(routes!(infrastructure::status))
        .routes(routes!(infrastructure::check))
        .routes(routes!(transfer::list, transfer::start))
        .routes(routes!(insight::timeline))
        .routes(routes!(insight::days))
        .routes(routes!(insight::map))
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
        .routes(routes!(artifacts::list))
        .routes(routes!(artifacts::zip))
        .routes(routes!(artifacts::download))
        .routes(routes!(agents::list, agents::create))
        .routes(routes!(agents::update, agents::delete))
        .routes(
            routes!(knowledge::upload)
                .layer(DefaultBodyLimit::max(DOCUMENT_MAX_BYTES + UPLOAD_SLACK)),
        )
        .routes(routes!(knowledge::list))
        .routes(routes!(knowledge::get, knowledge::delete))
        .routes(routes!(knowledge::search))
        .routes(routes!(knowledge::topics))
        .routes(routes!(knowledge::rebuild_topics))
        .routes(routes!(knowledge::settings, knowledge::put_settings))
        .routes(routes!(memories::list))
        .routes(routes!(memories::topics))
        .routes(routes!(memories::rebuild_topics))
        .routes(routes!(memories::get, memories::delete))
        .routes(routes!(orchestrator::status))
        .routes(routes!(realtime::ticket))
        .routes(routes!(realtime::events))
        .routes(routes!(realtime::websocket))
}

/// Every documented route: `/api/v1/*` plus `/metrics`.
pub fn api_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .nest("/api/v1", v1_routes())
        .routes(routes!(health::metrics))
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
            "/api/v1/graphs/{gid}",
            "/api/v1/graphs/{gid}/plans/{pid}/apply",
            "/api/v1/runs/{rid}/artifacts.zip",
            "/api/v1/graphs/from-template",
            "/api/v1/graphs/{gid}/events",
            "/metrics",
        ] {
            assert!(paths.contains_key(p), "missing {p}");
        }
        let node = &spec["components"]["schemas"]["GraphNode"]["properties"];
        assert!(node["agent_role"].is_object() && node["executor"].is_object());
        assert!(spec["components"]["schemas"]["ProposedNode"]["properties"]["ref"].is_object());
        assert!(spec["components"]["securitySchemes"]["bearer"].is_object());
    }

    /// Every enum value the spec documents must be a value the API really
    /// accepts and emits; generated client types depend on it.
    #[test]
    fn spec_enum_values_match_the_wire_format() {
        use crate::domain::{
            agent, audit, cycle, graph, issue, knowledge, memory, plan, run, settings, usage, user,
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
                _ => continue,
            }
            checked += 1;
        }
        assert_eq!(checked, 25, "every string enum schema is covered");
    }
}
