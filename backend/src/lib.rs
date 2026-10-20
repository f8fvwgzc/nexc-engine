//! nexc-engine backend: an axum monolith that plans, schedules and executes
//! graphs of LLM / agent tasks. The `nexc` binary is a thin CLI over this
//! library; integration tests use it directly.
#![deny(unsafe_code)]

pub mod a2a;
pub mod alerting;
pub mod app;
pub mod cli;
pub mod config;
pub mod domain;
pub mod dsa;
pub mod egress_proxy;
pub mod engine;
pub mod firewall;
pub mod gateway;
pub mod http;
pub mod kernel;
pub mod llm;
pub mod memory;
pub mod observability;
pub mod ontology;
pub mod orchestrator;
pub mod otlp;
pub mod realtime;
pub mod repo;
pub mod rpc;
pub mod security;
