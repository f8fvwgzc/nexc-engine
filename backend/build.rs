//! Compiles the C kernels in `csrc/` into a static library linked into `nexc`,
//! and the vendored protos into tonic clients and servers: onto-intelli (the
//! server only serves the test stub), the agent runtime's stream (the client;
//! the server serves the test stand-in) and the Agent Firewall API (the
//! server, with its descriptor set for reflection). Only a C11 compiler is
//! required (picked up by the `cc` crate from `CC`): the protos are compiled
//! by protox, in Rust, so no `protoc` is needed on build machines.
//!
//! It also lists the agent catalog for embedding: every skill in `skills/*.md`
//! and every role template in `templates/agent-*.json`, so that adding one is
//! adding a file (`src/domain/catalog.rs` includes the generated list).

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
    catalog();
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

/// Writes `$OUT_DIR/catalog.rs`: the skill files and the role templates, by
/// file name, each embedded with `include_str!`, in file-name order.
fn catalog() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let list = |dir: &str, prefix: &str, suffix: &str| -> Vec<(String, PathBuf)> {
        let dir = manifest.join(dir);
        println!("cargo:rerun-if-changed={}", dir.display());
        let mut files: Vec<(String, PathBuf)> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("{} is readable: {e}", dir.display()))
            .filter_map(|entry| {
                let path = entry.expect("a directory entry").path();
                let name = path.file_name()?.to_str()?.to_owned();
                (name.starts_with(prefix) && name.ends_with(suffix)).then_some((name, path))
            })
            .collect();
        files.sort();
        files
    };
    let entries = |files: Vec<(String, PathBuf)>| -> String {
        files
            .iter()
            .map(|(name, path)| {
                println!("cargo:rerun-if-changed={}", path.display());
                format!(
                    "    ({name:?}, include_str!({:?})),\n",
                    path.display().to_string()
                )
            })
            .collect()
    };
    let generated = format!(
        "/// Skill files (`skills/*.md`), by file name.\n\
         pub const SKILL_SOURCES: &[(&str, &str)] = &[\n{}];\n\
         /// Role templates (`templates/agent-*.json`), by file name.\n\
         pub const AGENT_TEMPLATE_SOURCES: &[(&str, &str)] = &[\n{}];\n",
        entries(list("skills", "", ".md")),
        entries(list("templates", "agent-", ".json")),
    );
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    std::fs::write(out.join("catalog.rs"), generated).expect("the catalog list is written");
}
