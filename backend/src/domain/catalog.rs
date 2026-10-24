//! The embedded agent catalog: every skill file (`backend/skills/*.md`) and
//! every role template (`backend/templates/agent-*.json`), listed by
//! `build.rs` so that adding one to the catalog is adding a file.

include!(concat!(env!("OUT_DIR"), "/catalog.rs"));
