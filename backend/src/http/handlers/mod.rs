//! Request handlers, one module per resource. Every handler is annotated
//! with `#[utoipa::path]` and every DTO derives `ToSchema`.

pub mod account;
pub mod agents;
pub mod artifacts;
pub mod auth;
pub mod cycles;
pub mod edges;
pub mod graphs;
pub mod health;
pub mod infrastructure;
pub mod insight;
pub mod issues;
pub mod knowledge;
pub mod memories;
pub mod nodes;
pub mod orchestrator;
pub mod plans;
pub mod platform;
pub mod realtime;
pub mod runs;
pub mod search;
pub mod settings;
pub mod teams;
pub mod templates;
pub mod transfer;
pub mod workspaces;
