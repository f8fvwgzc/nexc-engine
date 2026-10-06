//! Request handlers, one module per resource. Every handler is annotated
//! with `#[utoipa::path]` and every DTO derives `ToSchema`.

pub mod agents;
pub mod artifacts;
pub mod auth;
pub mod edges;
pub mod graphs;
pub mod health;
pub mod issues;
pub mod memories;
pub mod nodes;
pub mod orchestrator;
pub mod plans;
pub mod realtime;
pub mod runs;
pub mod settings;
pub mod teams;
pub mod templates;
pub mod workspaces;
