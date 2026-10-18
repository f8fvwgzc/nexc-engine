//! The generated gRPC services this backend speaks beside onto-intelli's:
//! the agent runtime's stream (`nexc.runtime.v1`, the backend is the client)
//! and the Agent Firewall API (`nexc.firewall.v1`, the backend is the
//! server). Both are compiled by `build.rs` from `proto/` with protox.
#![forbid(unsafe_code)]

/// `nexc.runtime.v1`: the backend ⇄ runtime stream (CONTRACT §8).
#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    unreachable_pub,
    rustdoc::all
)]
pub mod runtime {
    tonic::include_proto!("nexc.runtime.v1");
}

/// `nexc.firewall.v1`: the Agent Firewall API.
#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    unreachable_pub,
    rustdoc::all
)]
pub mod firewall {
    tonic::include_proto!("nexc.firewall.v1");
}

/// The descriptor set of every vendored proto, for server reflection.
pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("nexc_descriptors");
