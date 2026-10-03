/* SPDX-License-Identifier: MIT */
/*
 * nexc_kernel.h - small, dependency free compute kernels used by the
 * nexc-engine backend: hashing, feature-hashing text embeddings, MinHash
 * signatures and SHA-256.
 *
 * Every function is pure (no global state, no allocation) and validates its
 * arguments. Functions that can fail return 0 on success and a negative
 * errno-style value on failure.
 */
#ifndef NEXC_KERNEL_H
#define NEXC_KERNEL_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Dimension of the feature-hashing embedding produced by nexc_embed(). */
#define NEXC_EMBED_DIM 256

/* Number of hash functions in a MinHash signature. */
#define NEXC_MINHASH_K 64

/* Longest token (in bytes) considered by the tokenizer; longer runs are cut. */
#define NEXC_MAX_TOKEN 64

/* Size of a SHA-256 digest in bytes. */
#define NEXC_SHA256_LEN 32

#define NEXC_EINVAL 22

/* hash.c */
uint64_t nexc_fnv1a64(const uint8_t *data, size_t len);
uint64_t nexc_mix64(uint64_t x);
uint64_t nexc_hash64(const uint8_t *data, size_t len, uint64_t seed);

/* embed.c */
int nexc_embed(const uint8_t *text, size_t len, float *out, size_t dim);
float nexc_dot(const float *a, const float *b, size_t n);
float nexc_cosine(const float *a, const float *b, size_t n);

/* minhash.c */
int nexc_minhash(const uint8_t *text, size_t len, uint64_t *sig, size_t k);
double nexc_jaccard_estimate(const uint64_t *a, const uint64_t *b, size_t k);

/* sha256.c */
int nexc_sha256(const uint8_t *data, size_t len, uint8_t out[NEXC_SHA256_LEN]);

/*
 * Shared tokenizer used by embed.c and minhash.c.
 *
 * Tokens are maximal runs of ASCII alphanumerics or non-ASCII bytes (so UTF-8
 * words stay intact); ASCII letters are lowercased. Each call writes the next
 * token into @buf (at most NEXC_MAX_TOKEN bytes), stores its length in
 * @tok_len and returns the offset just past the token, or @len when no token
 * is left (in which case @tok_len is 0).
 */
size_t nexc_next_token(const uint8_t *text, size_t len, size_t pos,
		       uint8_t buf[NEXC_MAX_TOKEN], size_t *tok_len);

#ifdef __cplusplus
}
#endif

#endif /* NEXC_KERNEL_H */
