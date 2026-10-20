"""The public keys this runtime checks agent tokens with (CONTRACT section 8).

Only the backend signs agent tokens (EdDSA, Ed25519); this runtime never holds a key that could
mint one. It verifies with public keys: the ones configured in `NEXC_AGENT_TOKEN_PUBLIC_KEYS`,
and the set the backend publishes at `NEXC_AGENT_TOKEN_JWKS_URL`
(`GET /api/v1/agent-tokens/jwks.json`). A token names its key by `kid`, the key's RFC 7638
thumbprint, which is computed here from the key itself: a published `kid` that disagrees is
ignored, so nobody can file one key under another's id.

The set is fetched when a token names a key not seen yet (the backend rotated) and again once
it is older than `MAX_AGE_S`, never more often than every `MIN_REFRESH_S` after a fetch that
worked (`MIN_RETRY_S` after one that failed), so a stream of tokens with made-up ids cannot turn
into a stream of fetches. A failed fetch keeps the keys already known, and a key the set stops
listing keeps verifying for `RETIRE_GRACE_S` after it was last listed: replicas of the backend
answering with the sets before and after a rotation do not make the runtime flip between them.
"""

from __future__ import annotations

import asyncio
import base64
import hashlib
import json
import logging
import time
from collections.abc import Awaitable, Callable, Iterable, Mapping
from typing import Any

import httpx
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

log = logging.getLogger(__name__)

# How long a fetched key set is trusted before it is fetched again.
MAX_AGE_S = 300.0
# The shortest interval between two fetches, after one that worked and after one that failed (the
# backend retries a refused node within seconds).
MIN_REFRESH_S = 10.0
MIN_RETRY_S = 1.0
# How long a key the published set stopped listing still verifies: longer than `MAX_AGE_S`, so one
# refresh answered by a replica not yet rolled does not drop it.
RETIRE_GRACE_S = 900.0
FETCH_TIMEOUT_S = 3.0
# A key set is a few hundred bytes; anything this large is not one.
MAX_JWKS_BYTES = 64 * 1024

Fetch = Callable[[str], Awaitable[Any]]


class BadKey(Exception):
    """A configured or published key is not an Ed25519 public key."""


def _b64url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode("ascii")


def parse_public_key(raw: str) -> bytes:
    """A public key as configured: base64url (a JWK's `x`) or base64 of its 32 bytes."""
    text = raw.strip().rstrip("=")
    altchars = b"-_" if ("-" in text or "_" in text) else None
    try:
        decoded = base64.b64decode(text + "=" * (-len(text) % 4), altchars=altchars, validate=True)
    except ValueError as exc:
        raise BadKey(f"`{raw.strip()}` is not base64") from exc
    if len(decoded) != 32:
        raise BadKey(f"`{raw.strip()}` is not 32 bytes")
    return decoded


def key_id(public: bytes) -> str:
    """The RFC 7638 thumbprint of an Ed25519 key, as the backend names it in `kid`."""
    canonical = json.dumps(
        {"crv": "Ed25519", "kty": "OKP", "x": _b64url(public)}, separators=(",", ":")
    )
    return _b64url(hashlib.sha256(canonical.encode("ascii")).digest())


def keys_from(raws: Iterable[bytes]) -> dict[str, Ed25519PublicKey]:
    """Public keys by their id."""
    return {key_id(raw): Ed25519PublicKey.from_public_bytes(raw) for raw in raws}


def keys_from_jwks(document: Any) -> dict[str, Ed25519PublicKey]:
    """The Ed25519 signing keys of a JSON Web Key Set, by the id computed from each; anything
    else in the set (another curve, an encryption key, a `kid` that is not the key's) is
    skipped."""
    if not isinstance(document, Mapping) or not isinstance(document.get("keys"), list):
        raise BadKey("the key set has no `keys` list")
    found: dict[str, Ed25519PublicKey] = {}
    for jwk in document["keys"]:
        if not isinstance(jwk, Mapping):
            continue
        if jwk.get("kty") != "OKP" or jwk.get("crv") != "Ed25519":
            continue
        if jwk.get("alg", "EdDSA") != "EdDSA" or jwk.get("use", "sig") != "sig":
            continue
        x = jwk.get("x")
        if not isinstance(x, str):
            continue
        try:
            raw = parse_public_key(x)
        except BadKey:
            continue
        kid = key_id(raw)
        if jwk.get("kid", kid) != kid:
            log.warning("the agent token key set lists %s under another id; ignored", kid)
            continue
        found[kid] = Ed25519PublicKey.from_public_bytes(raw)
    return found


async def fetch_json(url: str) -> Any:
    """GETs a key set: no proxy (it is the backend, on the runtime's own network), no
    redirects, a short timeout and a size limit."""
    async with (
        httpx.AsyncClient(
            timeout=FETCH_TIMEOUT_S, trust_env=False, follow_redirects=False
        ) as client,
        client.stream("GET", url, headers={"Accept": "application/json"}) as response,
    ):
        response.raise_for_status()
        body = bytearray()
        async for chunk in response.aiter_bytes():
            body.extend(chunk)
            if len(body) > MAX_JWKS_BYTES:
                raise BadKey("the key set is too large")
        return json.loads(bytes(body))


class KeyRing:
    """The public keys agent tokens verify with, by `kid`: the configured ones, and the
    published ones when a URL is set."""

    def __init__(
        self,
        configured: Mapping[str, Ed25519PublicKey] | None = None,
        jwks_url: str | None = None,
        *,
        fetch: Fetch = fetch_json,
        clock: Callable[[], float] = time.monotonic,
        max_age_s: float = MAX_AGE_S,
        min_refresh_s: float = MIN_REFRESH_S,
    ) -> None:
        self._configured = dict(configured or {})
        # Each published key with when the set last listed it.
        self._published: dict[str, tuple[Ed25519PublicKey, float]] = {}
        self.jwks_url = jwks_url
        self._fetch = fetch
        self._clock = clock
        self._max_age_s = max_age_s
        self._min_refresh_s = min_refresh_s
        self._fetched_at: float | None = None
        self._tried_at: float | None = None
        self._last_failed = False
        self._lock = asyncio.Lock()
        self.fetches = 0

    @classmethod
    def from_config(cls, verify_keys: str | None, jwks_url: str | None) -> KeyRing:
        raws = [parse_public_key(p) for p in (verify_keys or "").split(",") if p.strip()]
        return cls(keys_from(raws), jwks_url)

    def get(self, kid: str) -> Ed25519PublicKey | None:
        """The key a token names, if this runtime knows it now."""
        if kid in self._configured:
            return self._configured[kid]
        published = self._published.get(kid)
        return published[0] if published is not None else None

    @property
    def kids(self) -> list[str]:
        return sorted({*self._configured, *self._published})

    def _stale(self) -> bool:
        return self._fetched_at is None or self._clock() - self._fetched_at >= self._max_age_s

    async def ensure(self, kid: str | None) -> None:
        """Fetches the published set when `kid` is not known yet or the set is stale, unless
        one was tried in the last `MIN_REFRESH_S`. Never raises: a failed fetch is logged and
        the keys known before stay."""
        if self.jwks_url is None:
            return
        if (kid is None or self.get(kid) is not None) and not self._stale():
            return
        async with self._lock:
            # Another request may have fetched while this one waited.
            if (kid is None or self.get(kid) is not None) and not self._stale():
                return
            now = self._clock()
            interval = MIN_RETRY_S if self._last_failed else self._min_refresh_s
            if self._tried_at is not None and now - self._tried_at < interval:
                return
            self._tried_at = now
            self.fetches += 1
            try:
                published = keys_from_jwks(await self._fetch(self.jwks_url))
            except Exception as exc:  # unreachable backend, bad JSON, too large, ...
                self._last_failed = True
                log.warning(
                    "agent token keys not refreshed from %s: %s: %s",
                    self.jwks_url,
                    type(exc).__name__,
                    str(exc)[:200],
                )
                return
            self._last_failed = False
            listed_at = self._clock()
            kept = {
                kid: entry
                for kid, entry in self._published.items()
                if kid not in published and listed_at - entry[1] < RETIRE_GRACE_S
            }
            self._published = {kid: (key, listed_at) for kid, key in published.items()} | kept
            self._fetched_at = listed_at
            log.info("agent token keys refreshed: %s", ", ".join(self.kids) or "none")
