// SPDX-License-Identifier: MIT
/*
 * minhash.c - MinHash signatures over word shingles.
 *
 * Shingles are adjacent word pairs (a single word when the text has only one
 * token). Signature slot i keeps the minimum of mix64(shingle ^ seed_i) over
 * all shingles; the fraction of equal slots between two signatures is an
 * unbiased estimate of the Jaccard similarity of their shingle sets.
 */
#include "nexc_kernel.h"

#define SEED_STEP 0x9e3779b97f4a7c15ULL

static void update_signature(uint64_t *sig, size_t k, uint64_t shingle)
{
	size_t i;

	for (i = 0; i < k; i++) {
		uint64_t v = nexc_mix64(shingle ^ nexc_mix64((uint64_t)(i + 1) * SEED_STEP));

		if (v < sig[i])
			sig[i] = v;
	}
}

int nexc_minhash(const uint8_t *text, size_t len, uint64_t *sig, size_t k)
{
	uint8_t tok[NEXC_MAX_TOKEN];
	uint64_t prev = 0;
	size_t tokens = 0;
	size_t pos = 0;
	size_t tok_len;
	size_t i;

	if (!sig || k == 0 || (!text && len > 0))
		return -NEXC_EINVAL;
	for (i = 0; i < k; i++)
		sig[i] = UINT64_MAX;

	while (pos < len) {
		uint64_t h;

		pos = nexc_next_token(text, len, pos, tok, &tok_len);
		if (tok_len == 0)
			break;
		h = nexc_hash64(tok, tok_len, 0);
		if (tokens > 0)
			update_signature(sig, k, nexc_mix64(prev * 31 + h));
		prev = h;
		tokens++;
	}
	if (tokens == 1)
		update_signature(sig, k, prev);
	return 0;
}

double nexc_jaccard_estimate(const uint64_t *a, const uint64_t *b, size_t k)
{
	size_t equal = 0;
	size_t i;

	if (!a || !b || k == 0)
		return 0.0;
	for (i = 0; i < k; i++) {
		/* Two empty signatures are not evidence of similarity. */
		if (a[i] == b[i] && a[i] != UINT64_MAX)
			equal++;
	}
	return (double)equal / (double)k;
}
