//! Pure-Rust reference implementations of the C kernels, used only by tests to
//! cross-check the C results bit for bit (floats within a tiny epsilon).

use super::{EMBED_DIM, Embedding, MINHASH_K, Signature};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const GOLDEN: u64 = 0x9e37_79b9_7f4a_7c15;
const MAX_TOKEN: usize = 64;

fn fnv1a64(data: &[u8]) -> u64 {
    data.iter().fold(FNV_OFFSET, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(FNV_PRIME)
    })
}

fn mix64(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

pub fn hash64(data: &[u8], seed: u64) -> u64 {
    mix64(fnv1a64(data) ^ mix64(seed))
}

fn is_token_byte(c: u8) -> bool {
    c >= 0x80 || c.is_ascii_alphanumeric()
}

fn tokens(text: &str) -> Vec<Vec<u8>> {
    text.as_bytes()
        .split(|&c| !is_token_byte(c))
        .filter(|t| !t.is_empty())
        .map(|t| {
            t.iter()
                .take(MAX_TOKEN)
                .map(u8::to_ascii_lowercase)
                .collect()
        })
        .collect()
}

fn add_feature(out: &mut Embedding, h: u64, weight: f32) {
    let idx = (h % EMBED_DIM as u64) as usize;
    let sign = if h >> 63 == 1 { -1.0 } else { 1.0 };
    out[idx] += sign * weight;
}

pub fn embed(text: &str) -> Embedding {
    let mut out = [0f32; EMBED_DIM];
    let mut prev: Option<u64> = None;
    for tok in tokens(text) {
        let h = hash64(&tok, 0);
        add_feature(&mut out, h, 1.0);
        if let Some(p) = prev {
            add_feature(
                &mut out,
                mix64(p.wrapping_mul(31).wrapping_add(h).wrapping_add(GOLDEN)),
                0.5,
            );
        }
        prev = Some(h);
    }
    let norm: f64 = out.iter().map(|&v| f64::from(v) * f64::from(v)).sum();
    if norm > 0.0 {
        let inv = (1.0 / norm.sqrt()) as f32;
        out.iter_mut().for_each(|v| *v *= inv);
    }
    out
}

fn update(sig: &mut Signature, shingle: u64) {
    for (i, slot) in sig.iter_mut().enumerate() {
        let seed = mix64((i as u64 + 1).wrapping_mul(GOLDEN));
        *slot = (*slot).min(mix64(shingle ^ seed));
    }
}

pub fn minhash(text: &str) -> Signature {
    let mut sig = [u64::MAX; MINHASH_K];
    let hashes: Vec<u64> = tokens(text).iter().map(|t| hash64(t, 0)).collect();
    for pair in hashes.windows(2) {
        update(
            &mut sig,
            mix64(pair[0].wrapping_mul(31).wrapping_add(pair[1])),
        );
    }
    if let [only] = hashes.as_slice() {
        update(&mut sig, *only);
    }
    sig
}
