//! Raw declarations of the C kernels in `csrc/` (see `csrc/include/nexc_kernel.h`).

unsafe extern "C" {
    pub fn nexc_hash64(data: *const u8, len: usize, seed: u64) -> u64;
    pub fn nexc_embed(text: *const u8, len: usize, out: *mut f32, dim: usize) -> i32;
    pub fn nexc_dot(a: *const f32, b: *const f32, n: usize) -> f32;
    pub fn nexc_cosine(a: *const f32, b: *const f32, n: usize) -> f32;
    pub fn nexc_minhash(text: *const u8, len: usize, sig: *mut u64, k: usize) -> i32;
    pub fn nexc_jaccard_estimate(a: *const u64, b: *const u64, k: usize) -> f64;
    pub fn nexc_sha256(data: *const u8, len: usize, out: *mut u8) -> i32;
}
