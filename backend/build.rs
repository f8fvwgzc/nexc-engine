//! Compiles the C kernels in `csrc/` into a static library linked into `nexc`,
//! and the vendored protos into tonic clients and servers: onto-intelli (the
//! server only serves the test stub), the agent runtime's stream (the client;
//! the server serves the test stand-in) and the Agent Firewall API (the
//! server, with its descriptor set for reflection). Only a C11 compiler is
//! required (picked up by the `cc` crate from `CC`): the protos are compiled
//! by protox, in Rust, so no `protoc` is needed on build machines.

use std::path::PathBuf;

const SOURCES: [&str; 4] = [
    "csrc/hash.c",
    "csrc/embed.c",
    "csrc/minhash.c",
    "csrc/sha256.c",
];

const PROTOS: [&str; 3] = [
    "onto_intelli/v1/onto_intelli.proto",
    "nexc/runtime/v1/runtime.proto",
    "nexc/firewall/v1/firewall.proto",
];

fn main() {
    println!("cargo:rerun-if-changed=csrc");
    let mut build = cc::Build::new();
    build
        .files(SOURCES)
        .include("csrc/include")
        .std("c11")
        .warnings(true)
        .extra_warnings(true)
        .flag_if_supported("-Wvla")
        .flag_if_supported("-Werror=vla")
        .flag_if_supported("-fno-strict-aliasing")
        .opt_level(2);
    build.compile("nexc_kernel");
    if !cfg!(target_os = "windows") {
        println!("cargo:rustc-link-lib=m");
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("proto");
    for proto in PROTOS {
        println!("cargo:rerun-if-changed={}", root.join(proto).display());
    }
    let descriptors = protox::compile(PROTOS, [&root]).expect("the vendored protos compile");
    // The descriptor set, for gRPC server reflection on the Agent Firewall API.
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let encoded = prost::Message::encode_to_vec(&descriptors);
    std::fs::write(out.join("nexc_descriptors.bin"), encoded)
        .expect("the descriptor set is written");
    tonic_prost_build::configure()
        .build_client(true)
        .build_server(true)
        .compile_fds(descriptors)
        .expect("the services generate");
}
