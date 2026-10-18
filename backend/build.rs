//! Compiles the C kernels in `csrc/` into a static library linked into `nexc`,
//! and the vendored onto-intelli proto into a tonic client and server (the
//! server only serves the test stub). Only a C11 compiler is required (picked
//! up by the `cc` crate from `CC`): the proto is compiled by protox, in Rust,
//! so no `protoc` is needed on build machines.

use std::path::PathBuf;

const SOURCES: [&str; 4] = [
    "csrc/hash.c",
    "csrc/embed.c",
    "csrc/minhash.c",
    "csrc/sha256.c",
];

const PROTO: &str = "onto_intelli/v1/onto_intelli.proto";

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
    println!("cargo:rerun-if-changed={}", root.join(PROTO).display());
    let descriptors = protox::compile([PROTO], [&root]).expect("the vendored proto compiles");
    tonic_prost_build::configure()
        .build_client(true)
        .build_server(true)
        .compile_fds(descriptors)
        .expect("the onto-intelli services generate");
}
