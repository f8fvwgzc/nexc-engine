//! Compiles the C kernels in `csrc/` into a static library linked into `nexc`.
//! Only a C11 compiler is required (picked up by the `cc` crate from `CC`).

const SOURCES: [&str; 4] = [
    "csrc/hash.c",
    "csrc/embed.c",
    "csrc/minhash.c",
    "csrc/sha256.c",
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
}
