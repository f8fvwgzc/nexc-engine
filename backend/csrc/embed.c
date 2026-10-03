// SPDX-License-Identifier: MIT
/*
 * embed.c - feature-hashing ("hashing trick") text embeddings.
 *
 * Every unigram adds +-1.0 and every adjacent word bigram adds +-0.5 to the
 * bucket selected by its hash; the sign comes from the top hash bit so that
 * collisions cancel out in expectation. The vector is L2 normalised, which
 * makes nexc_dot() of two embeddings their cosine similarity.
 */
#include <math.h>
#include <string.h>

#include "nexc_kernel.h"

#define UNIGRAM_WEIGHT 1.0f
#define BIGRAM_WEIGHT 0.5f
#define BIGRAM_SEED 0x9e3779b97f4a7c15ULL

static void add_feature(float *out, size_t dim, uint64_t h, float weight)
{
	size_t idx = (size_t)(h % dim);
	float sign = (h >> 63) ? -1.0f : 1.0f;

	out[idx] += sign * weight;
}

static void l2_normalize(float *v, size_t n)
{
	double sum = 0.0;
	float inv;
	size_t i;

	for (i = 0; i < n; i++)
		sum += (double)v[i] * (double)v[i];
	if (sum <= 0.0)
		return;
	inv = (float)(1.0 / sqrt(sum));
	for (i = 0; i < n; i++)
		v[i] *= inv;
}

int nexc_embed(const uint8_t *text, size_t len, float *out, size_t dim)
{
	uint8_t tok[NEXC_MAX_TOKEN];
	uint64_t prev = 0;
	int have_prev = 0;
	size_t pos = 0;
	size_t tok_len;

	if (!out || dim == 0 || (!text && len > 0))
		return -NEXC_EINVAL;
	memset(out, 0, dim * sizeof(*out));

	while (pos < len) {
		uint64_t h;

		pos = nexc_next_token(text, len, pos, tok, &tok_len);
		if (tok_len == 0)
			break;
		h = nexc_hash64(tok, tok_len, 0);
		add_feature(out, dim, h, UNIGRAM_WEIGHT);
		if (have_prev)
			add_feature(out, dim, nexc_mix64(prev * 31 + h + BIGRAM_SEED),
				    BIGRAM_WEIGHT);
		prev = h;
		have_prev = 1;
	}
	l2_normalize(out, dim);
	return 0;
}

float nexc_dot(const float *a, const float *b, size_t n)
{
	double sum = 0.0;
	size_t i;

	if (!a || !b)
		return 0.0f;
	for (i = 0; i < n; i++)
		sum += (double)a[i] * (double)b[i];
	return (float)sum;
}

float nexc_cosine(const float *a, const float *b, size_t n)
{
	double dot = 0.0, na = 0.0, nb = 0.0;
	size_t i;

	if (!a || !b)
		return 0.0f;
	for (i = 0; i < n; i++) {
		dot += (double)a[i] * (double)b[i];
		na += (double)a[i] * (double)a[i];
		nb += (double)b[i] * (double)b[i];
	}
	if (na <= 0.0 || nb <= 0.0)
		return 0.0f;
	return (float)(dot / (sqrt(na) * sqrt(nb)));
}
