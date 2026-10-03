//! Realtime updates: a per-graph hub feeding SSE streams (engine events)
//! and WebSockets (collaborative editing), fanned out across backend
//! replicas through PostgreSQL LISTEN/NOTIFY.
#![forbid(unsafe_code)]

pub mod events;
pub mod fanout;
pub mod hub;
pub mod sse;
pub mod ws;
