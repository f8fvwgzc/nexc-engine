// SPDX-License-Identifier: MIT
/*
 * hash.c - FNV-1a 64 with a strong 64-bit finalizer.
 *
 * FNV-1a alone has weak avalanche in the high bits, so nexc_hash64() runs the
 * result through the splitmix64 finalizer (the same mixer used by
 * xxHash/murmur style hashes). The output is used for feature hashing,
 * MinHash and ETags - never for security.
 */
#include "nexc_kernel.h"

#define FNV_OFFSET 0xcbf29ce484222325ULL
#define FNV_PRIME 0x00000100000001b3ULL

uint64_t nexc_fnv1a64(const uint8_t *data, size_t len)
{
	uint64_t h = FNV_OFFSET;
	size_t i;

	if (!data)
		return h;
	for (i = 0; i < len; i++) {
		h ^= data[i];
		h *= FNV_PRIME;
	}
	return h;
}

uint64_t nexc_mix64(uint64_t x)
{
	x ^= x >> 30;
	x *= 0xbf58476d1ce4e5b9ULL;
	x ^= x >> 27;
	x *= 0x94d049bb133111ebULL;
	x ^= x >> 31;
	return x;
}

uint64_t nexc_hash64(const uint8_t *data, size_t len, uint64_t seed)
{
	return nexc_mix64(nexc_fnv1a64(data, len) ^ nexc_mix64(seed));
}

static int is_token_byte(uint8_t c)
{
	if (c >= 0x80)
		return 1;
	if (c >= '0' && c <= '9')
		return 1;
	if (c >= 'a' && c <= 'z')
		return 1;
	return c >= 'A' && c <= 'Z';
}

size_t nexc_next_token(const uint8_t *text, size_t len, size_t pos,
		       uint8_t buf[NEXC_MAX_TOKEN], size_t *tok_len)
{
	size_t n = 0;

	*tok_len = 0;
	if (!text)
		return len;
	while (pos < len && !is_token_byte(text[pos]))
		pos++;
	while (pos < len && is_token_byte(text[pos])) {
		uint8_t c = text[pos++];

		if (c >= 'A' && c <= 'Z')
			c = (uint8_t)(c - 'A' + 'a');
		if (n < NEXC_MAX_TOKEN)
			buf[n++] = c;
	}
	*tok_len = n;
	return pos;
}
