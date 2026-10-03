// SPDX-License-Identifier: MIT
/*
 * sha256.c - FIPS 180-4 SHA-256, used for content-hash memoization of node
 * results. Streaming state is kept on the stack; the whole input is hashed in
 * one call.
 */
#include <string.h>

#include "nexc_kernel.h"

struct sha256_ctx {
	uint32_t state[8];
	uint8_t block[64];
	size_t block_len;
	uint64_t total_len;
};

static const uint32_t k_round[64] = {
	0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1,
	0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
	0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
	0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
	0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
	0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
	0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
	0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
	0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
	0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
	0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
};

static uint32_t rotr(uint32_t x, unsigned int n)
{
	return (x >> n) | (x << (32 - n));
}

static void sha256_compress(uint32_t state[8], const uint8_t block[64])
{
	uint32_t w[64];
	uint32_t a, b, c, d, e, f, g, h;
	int i;

	for (i = 0; i < 16; i++)
		w[i] = (uint32_t)block[i * 4] << 24 | (uint32_t)block[i * 4 + 1] << 16 |
		       (uint32_t)block[i * 4 + 2] << 8 | (uint32_t)block[i * 4 + 3];
	for (i = 16; i < 64; i++) {
		uint32_t s0 = rotr(w[i - 15], 7) ^ rotr(w[i - 15], 18) ^ (w[i - 15] >> 3);
		uint32_t s1 = rotr(w[i - 2], 17) ^ rotr(w[i - 2], 19) ^ (w[i - 2] >> 10);

		w[i] = w[i - 16] + s0 + w[i - 7] + s1;
	}

	a = state[0];
	b = state[1];
	c = state[2];
	d = state[3];
	e = state[4];
	f = state[5];
	g = state[6];
	h = state[7];

	for (i = 0; i < 64; i++) {
		uint32_t s1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
		uint32_t ch = (e & f) ^ (~e & g);
		uint32_t t1 = h + s1 + ch + k_round[i] + w[i];
		uint32_t s0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
		uint32_t maj = (a & b) ^ (a & c) ^ (b & c);
		uint32_t t2 = s0 + maj;

		h = g;
		g = f;
		f = e;
		e = d + t1;
		d = c;
		c = b;
		b = a;
		a = t1 + t2;
	}

	state[0] += a;
	state[1] += b;
	state[2] += c;
	state[3] += d;
	state[4] += e;
	state[5] += f;
	state[6] += g;
	state[7] += h;
}

static void sha256_init(struct sha256_ctx *ctx)
{
	static const uint32_t iv[8] = {
		0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
		0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
	};

	memcpy(ctx->state, iv, sizeof(iv));
	ctx->block_len = 0;
	ctx->total_len = 0;
}

static void sha256_update(struct sha256_ctx *ctx, const uint8_t *data, size_t len)
{
	size_t i;

	ctx->total_len += len;
	for (i = 0; i < len; i++) {
		ctx->block[ctx->block_len++] = data[i];
		if (ctx->block_len == sizeof(ctx->block)) {
			sha256_compress(ctx->state, ctx->block);
			ctx->block_len = 0;
		}
	}
}

static void sha256_final(struct sha256_ctx *ctx, uint8_t out[NEXC_SHA256_LEN])
{
	uint64_t bits = ctx->total_len * 8;
	uint8_t pad = 0x80;
	uint8_t zero = 0;
	uint8_t len_be[8];
	int i;

	sha256_update(ctx, &pad, 1);
	while (ctx->block_len != 56)
		sha256_update(ctx, &zero, 1);
	for (i = 0; i < 8; i++)
		len_be[i] = (uint8_t)(bits >> (56 - 8 * i));
	sha256_update(ctx, len_be, sizeof(len_be));

	for (i = 0; i < 8; i++) {
		out[i * 4] = (uint8_t)(ctx->state[i] >> 24);
		out[i * 4 + 1] = (uint8_t)(ctx->state[i] >> 16);
		out[i * 4 + 2] = (uint8_t)(ctx->state[i] >> 8);
		out[i * 4 + 3] = (uint8_t)ctx->state[i];
	}
}

int nexc_sha256(const uint8_t *data, size_t len, uint8_t out[NEXC_SHA256_LEN])
{
	struct sha256_ctx ctx;

	if (!out || (!data && len > 0))
		return -NEXC_EINVAL;
	sha256_init(&ctx);
	if (len > 0)
		sha256_update(&ctx, data, len);
	sha256_final(&ctx, out);
	return 0;
}
