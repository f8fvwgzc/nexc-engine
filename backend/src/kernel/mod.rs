//! Safe wrappers over the C kernels (hashing, embeddings, MinHash, SHA-256).
//!
//! This is the only module allowed to contain `unsafe` code. Each call passes
//! pointers derived from live Rust slices together with their exact lengths;
//! the C side never retains pointers past the call and never allocates.
#![allow(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

mod ffi;
#[cfg(test)]
mod reference;

/// Dimension of [`embed`] vectors (`NEXC_EMBED_DIM`).
pub const EMBED_DIM: usize = 256;
/// Number of slots in a [`minhash`] signature (`NEXC_MINHASH_K`).
pub const MINHASH_K: usize = 64;

/// A feature-hashing text embedding (L2 normalised).
pub type Embedding = [f32; EMBED_DIM];
/// A MinHash signature.
pub type Signature = [u64; MINHASH_K];

/// FNV-1a 64 + splitmix64 finalizer of `data` (non-cryptographic).
pub fn hash64(data: &[u8], seed: u64) -> u64 {
    // SAFETY: `data` is a valid slice for `data.len()` bytes; C only reads it.
    unsafe { ffi::nexc_hash64(data.as_ptr(), data.len(), seed) }
}

/// Embeds `text` into a normalised [`EMBED_DIM`]-dimensional vector.
pub fn embed(text: &str) -> Embedding {
    let mut out = [0f32; EMBED_DIM];
    // SAFETY: `text` is readable for `len` bytes and `out` is writable for
    // exactly `EMBED_DIM` floats, the dimension passed to C.
    let rc = unsafe { ffi::nexc_embed(text.as_ptr(), text.len(), out.as_mut_ptr(), EMBED_DIM) };
    debug_assert_eq!(rc, 0, "nexc_embed rejected valid arguments");
    out
}

/// Dot product of two embeddings (their cosine similarity, since both are normalised).
pub fn dot(a: &Embedding, b: &Embedding) -> f32 {
    // SAFETY: both arrays hold exactly `EMBED_DIM` floats.
    unsafe { ffi::nexc_dot(a.as_ptr(), b.as_ptr(), EMBED_DIM) }
}

/// Cosine similarity of two vectors of equal length (0 for mismatched lengths).
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    // SAFETY: both slices are readable for `a.len()` floats (lengths checked above).
    unsafe { ffi::nexc_cosine(a.as_ptr(), b.as_ptr(), a.len()) }
}

/// MinHash signature of the word-bigram shingles of `text`.
pub fn minhash(text: &str) -> Signature {
    let mut sig = [0u64; MINHASH_K];
    // SAFETY: `text` is readable for `len` bytes; `sig` is writable for `MINHASH_K` slots.
    let rc = unsafe { ffi::nexc_minhash(text.as_ptr(), text.len(), sig.as_mut_ptr(), MINHASH_K) };
    debug_assert_eq!(rc, 0, "nexc_minhash rejected valid arguments");
    sig
}

/// Estimated Jaccard similarity of the shingle sets behind two signatures.
pub fn jaccard(a: &Signature, b: &Signature) -> f64 {
    // SAFETY: both arrays hold exactly `MINHASH_K` slots.
    unsafe { ffi::nexc_jaccard_estimate(a.as_ptr(), b.as_ptr(), MINHASH_K) }
}

/// SHA-256 digest of `data`.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    // SAFETY: `data` is readable for `len` bytes; `out` is writable for the 32
    // bytes the C function writes.
    let rc = unsafe { ffi::nexc_sha256(data.as_ptr(), data.len(), out.as_mut_ptr()) };
    debug_assert_eq!(rc, 0, "nexc_sha256 rejected valid arguments");
    out
}

/// Hex-encoded SHA-256 of `data`.
pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(sha256(data))
}

/// Serialises an embedding as little-endian `f32` bytes (database BLOB format).
pub fn embedding_to_bytes(e: &Embedding) -> Vec<u8> {
    e.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Parses a little-endian `f32` BLOB; `None` when the length is wrong.
pub fn embedding_from_bytes(bytes: &[u8]) -> Option<Embedding> {
    if bytes.len() != EMBED_DIM * 4 {
        return None;
    }
    let mut out = [0f32; EMBED_DIM];
    let (chunks, _) = bytes.as_chunks::<4>();
    for (slot, chunk) in out.iter_mut().zip(chunks) {
        *slot = f32::from_le_bytes(*chunk);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest;

    const SAMPLES: [&str; 6] = [
        "",
        "hello",
        "Write a research DOCX about Rust async runtimes",
        "naïve café — unicode tokens stay whole, ÄÖÜ",
        "a b c d e f g h i j k l m n o p q r s t u v w x y z 0123456789",
        "Repeated repeated REPEATED words words",
    ];

    #[test]
    fn hash64_matches_reference() {
        for s in SAMPLES {
            for seed in [0, 1, 0xdead_beef] {
                assert_eq!(
                    hash64(s.as_bytes(), seed),
                    reference::hash64(s.as_bytes(), seed)
                );
            }
        }
    }

    #[test]
    fn embed_matches_reference_and_is_normalised() {
        for s in SAMPLES {
            let c = embed(s);
            let r = reference::embed(s);
            for (a, b) in c.iter().zip(r.iter()) {
                assert!((a - b).abs() < 1e-5, "{s}: {a} vs {b}");
            }
            if !s.is_empty() {
                assert!((dot(&c, &c) - 1.0).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn similar_texts_score_higher() {
        let a = embed("research the history of the printing press");
        let b = embed("printing press history research notes");
        let c = embed("kubernetes ingress controller configuration");
        assert!(cosine(&a, &b) > cosine(&a, &c));
        assert!((cosine(&a, &b) - dot(&a, &b)).abs() < 1e-5);
    }

    #[test]
    fn minhash_matches_reference() {
        for s in SAMPLES {
            assert_eq!(minhash(s), reference::minhash(s));
        }
        let a = minhash("the quick brown fox jumps over the lazy dog");
        let b = minhash("the quick brown fox jumps over the lazy cat");
        let c = minhash("completely unrelated sentence about databases");
        assert!(jaccard(&a, &b) > jaccard(&a, &c));
        assert!((jaccard(&a, &a) - 1.0).abs() < f64::EPSILON);
        assert_eq!(jaccard(&minhash(""), &minhash("")), 0.0);
    }

    #[test]
    fn sha256_matches_sha2_crate() {
        let long = "x".repeat(1000);
        for s in SAMPLES.iter().copied().chain([long.as_str()]) {
            let expected: [u8; 32] = sha2::Sha256::digest(s.as_bytes()).into();
            assert_eq!(sha256(s.as_bytes()), expected);
        }
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn embedding_blob_round_trip() {
        let e = embed("round trip");
        assert_eq!(embedding_from_bytes(&embedding_to_bytes(&e)), Some(e));
        assert_eq!(embedding_from_bytes(&[0u8; 3]), None);
    }
}
