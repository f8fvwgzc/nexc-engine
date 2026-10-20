"""The runtime verifies agent tokens with the backend's public keys alone: configured, or
fetched from the backend's key set by `kid`, across a rotation (CONTRACT section 8)."""

from __future__ import annotations

import base64
import json
from collections.abc import Callable
from pathlib import Path
from typing import Any

import httpx
import pytest
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
from fastapi.testclient import TestClient
from pydantic import ValidationError

from nexc_runtime.api.app import create_app
from nexc_runtime.config import Settings
from nexc_runtime.keys import BadKey, KeyRing, key_id, keys_from_jwks, parse_public_key
from nexc_runtime.policy import TokenError, token_kid, verify_token

from .conftest import (
    AUTH,
    BACKEND_KEY,
    BACKEND_KID,
    BACKEND_X,
    TOKEN,
    FakeProvider,
    Step,
    call,
    execute_body,
    mint_token,
    parse_ndjson,
)

URL = "http://backend:8080/api/v1/agent-tokens/jwks.json"
NEXT_KEY = Ed25519PrivateKey.from_private_bytes(b"n" * 32)


def _x(key: Ed25519PrivateKey) -> str:
    raw = key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode()


def _jwk(key: Ed25519PrivateKey, **extra: Any) -> dict[str, Any]:
    x = _x(key)
    return {
        "kty": "OKP",
        "crv": "Ed25519",
        "x": x,
        "kid": key_id(parse_public_key(x)),
        "alg": "EdDSA",
        "use": "sig",
        **extra,
    }


class FakeBackend:
    """Serves a key set the test changes, counting the fetches."""

    def __init__(self, *keys: Ed25519PrivateKey) -> None:
        self.keys = list(keys)
        self.fail = False
        self.urls: list[str] = []

    async def __call__(self, url: str) -> Any:
        self.urls.append(url)
        if self.fail:
            raise httpx.ConnectError("backend unreachable")
        return {"keys": [_jwk(k) for k in self.keys]}


class Clock:
    def __init__(self) -> None:
        self.now = 1000.0

    def __call__(self) -> float:
        return self.now


async def _verify(ring: KeyRing, token: str) -> Any:
    await ring.ensure(token_kid(token))
    return verify_token(token, ring, run_id="run-1", node_id="node-1")


def test_the_key_id_is_the_rfc_7638_thumbprint_the_backend_computes() -> None:
    # RFC 8037 appendix A.3, and the id the backend's test asserts for the same key.
    x = parse_public_key("11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo")
    assert key_id(x) == "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k"
    assert parse_public_key(base64.b64encode(x).decode()) == x, "standard base64 too"
    for bad in ("AAAA", "not base64 at all!", ""):
        with pytest.raises(BadKey):
            parse_public_key(bad)


def test_a_key_set_yields_only_ed25519_signing_keys_under_their_own_id() -> None:
    good = _jwk(BACKEND_KEY)
    found = keys_from_jwks(
        {
            "keys": [
                good,
                _jwk(NEXT_KEY, kid=BACKEND_KID),  # filed under another key's id
                {**_jwk(NEXT_KEY), "crv": "X25519"},
                {**_jwk(NEXT_KEY), "use": "enc"},
                {"kty": "RSA", "n": "x", "e": "AQAB"},
                {**_jwk(NEXT_KEY), "x": "short"},
                "not a key",
            ]
        }
    )
    assert list(found) == [good["kid"]]
    with pytest.raises(BadKey):
        keys_from_jwks({"no": "keys"})


async def test_an_unknown_kid_fetches_the_published_set_once_across_a_rotation() -> None:
    backend = FakeBackend(BACKEND_KEY)
    clock = Clock()
    ring = KeyRing(jwks_url=URL, fetch=backend, clock=clock)
    # The first token fetches the set; the next ones use it.
    for _ in range(3):
        token = await _verify(ring, mint_token())
        assert token.on_behalf_of is not None
    assert backend.urls == [URL]
    # Step 1 of a rotation: the next key is published beside the current one. Nothing changes
    # here until a token names it.
    backend.keys = [BACKEND_KEY, NEXT_KEY]
    clock.now += 20
    await _verify(ring, mint_token())
    assert ring.fetches == 1
    # Step 2: the backend signs with the next key. Its first token makes the runtime fetch
    # again; tokens of both keys verify by their kid.
    await _verify(ring, mint_token(key=NEXT_KEY))
    assert ring.fetches == 2
    await _verify(ring, mint_token())
    await _verify(ring, mint_token(key=NEXT_KEY))
    assert ring.fetches == 2
    # Step 3: the old key is retired. Once the set is stale it is fetched again, and the old
    # key's tokens stop verifying once it has gone unlisted for the grace period.
    backend.keys = [NEXT_KEY]
    clock.now += 901
    with pytest.raises(TokenError, match="does not know"):
        await _verify(ring, mint_token())
    await _verify(ring, mint_token(key=NEXT_KEY))
    assert ring.fetches == 3


async def test_fetches_are_bounded_and_a_failed_fetch_keeps_the_keys() -> None:
    backend = FakeBackend(BACKEND_KEY)
    clock = Clock()
    ring = KeyRing(jwks_url=URL, fetch=backend, clock=clock)
    await _verify(ring, mint_token())
    # Tokens naming made-up keys: one fetch per interval, however many arrive.
    stranger = Ed25519PrivateKey.from_private_bytes(b"x" * 32)
    clock.now += 11
    for _ in range(50):
        with pytest.raises(TokenError, match="does not know"):
            await _verify(ring, mint_token(key=stranger))
    assert ring.fetches == 2
    # The backend is away when the set goes stale: the keys known stay usable.
    backend.fail = True
    clock.now += 400
    await _verify(ring, mint_token())
    assert ring.fetches == 3
    # A set that is not one leaves them too.
    ring_bad = KeyRing(jwks_url=URL, fetch=lambda _url: _json("[]"), clock=clock)
    with pytest.raises(TokenError, match="does not know"):
        await _verify(ring_bad, mint_token())


async def test_after_a_failed_fetch_the_next_try_comes_soon() -> None:
    backend = FakeBackend(BACKEND_KEY)
    backend.fail = True
    clock = Clock()
    ring = KeyRing(jwks_url=URL, fetch=backend, clock=clock)
    with pytest.raises(TokenError, match="does not know"):
        await _verify(ring, mint_token())
    # Within the second: no fetch. The backend's retry, seconds later: a fetch, and it works.
    with pytest.raises(TokenError, match="does not know"):
        await _verify(ring, mint_token())
    assert ring.fetches == 1
    backend.fail = False
    clock.now += 2
    await _verify(ring, mint_token())
    assert ring.fetches == 2


async def test_replicas_answering_with_the_old_set_do_not_drop_the_new_key() -> None:
    backend = FakeBackend(BACKEND_KEY, NEXT_KEY)
    clock = Clock()
    ring = KeyRing(jwks_url=URL, fetch=backend, clock=clock)
    await _verify(ring, mint_token(key=NEXT_KEY))
    # Replicas not yet rolled answer the next fetches with the set before the rotation.
    backend.keys = [BACKEND_KEY]
    for _ in range(2):
        clock.now += 301
        await _verify(ring, mint_token())
        await _verify(ring, mint_token(key=NEXT_KEY))
    assert ring.fetches == 3
    # Until the key has gone unlisted for the grace period.
    clock.now += 301
    await _verify(ring, mint_token())
    with pytest.raises(TokenError, match="does not know"):
        await _verify(ring, mint_token(key=NEXT_KEY))


async def _json(text: str) -> Any:
    return json.loads(text)


async def test_configured_keys_need_no_fetch() -> None:
    ring = KeyRing.from_config(f" {BACKEND_X} , {_x(NEXT_KEY)}", None)
    assert set(ring.kids) == {BACKEND_KID, key_id(parse_public_key(_x(NEXT_KEY)))}
    await _verify(ring, mint_token())
    await _verify(ring, mint_token(key=NEXT_KEY))
    assert ring.fetches == 0


def test_the_runtime_needs_public_keys_and_refuses_the_old_shared_key(
    workspace_root: Path,
) -> None:
    base: dict[str, Any] = {"nexc_runtime_token": TOKEN, "runtime_workspace": workspace_root}
    with pytest.raises(ValidationError, match="NEXC_AGENT_TOKEN_JWKS_URL"):
        Settings(**base)
    with pytest.raises(ValidationError, match="no longer read"):
        Settings(**base, nexc_agent_token_jwks_url=URL, nexc_agent_token_key="k" * 40)
    with pytest.raises(ValidationError, match="Ed25519 public key"):
        Settings(**base, nexc_agent_token_public_keys="AAAA")
    with pytest.raises(ValidationError, match="http"):
        Settings(**base, nexc_agent_token_jwks_url="file:///etc/keys.json")
    # Pinned keys and a fetched set together would let the fetch add keys to the pin.
    with pytest.raises(ValidationError, match="not both"):
        Settings(**base, nexc_agent_token_public_keys=BACKEND_X, nexc_agent_token_jwks_url=URL)
    assert Settings(**base, nexc_agent_token_jwks_url=URL).key_ring().jwks_url == URL
    # Nothing secret about agent tokens is left in the runtime's settings.
    ok = Settings(**base, nexc_agent_token_public_keys=BACKEND_X)
    assert "agent_token_key" not in ok.model_dump()


def test_a_run_whose_token_names_a_rotated_key_fetches_it_first(
    make_settings: Callable[..., Settings],
) -> None:
    backend = FakeBackend(BACKEND_KEY, NEXT_KEY)
    provider = FakeProvider([Step(calls=[call("finish", answer="Done.")])])
    # The key set lists the current key and the next one the backend signs with now.
    settings = make_settings(nexc_agent_token_public_keys=None, nexc_agent_token_jwks_url=URL)
    app = create_app(settings, provider_factory=lambda _r, _s: provider)
    app.state.keys._fetch = backend
    with TestClient(app) as client:
        body = execute_body(token=mint_token(key=NEXT_KEY))
        events = parse_ndjson(client.post("/v1/execute", json=body, headers=AUTH).content)
    assert events[-1]["type"] == "result", events[-1]
    assert backend.urls == [URL]


def test_a_key_not_known_yet_is_refused_as_worth_a_retry(
    make_settings: Callable[..., Settings],
) -> None:
    backend = FakeBackend(BACKEND_KEY)
    backend.fail = True
    provider = FakeProvider([Step(calls=[call("finish", answer="never")])])
    app = create_app(
        make_settings(nexc_agent_token_public_keys=None, nexc_agent_token_jwks_url=URL),
        provider_factory=lambda _r, _s: provider,
    )
    app.state.keys._fetch = backend
    with TestClient(app) as client:
        # Signed with a key the backend published while its set could not be fetched.
        body = execute_body(token=mint_token(key=NEXT_KEY))
        events = parse_ndjson(client.post("/v1/execute", json=body, headers=AUTH).content)
        assert events[-1]["type"] == "error" and events[-1]["retryable"] is True
        assert "does not know" in events[-1]["message"]
        # The backend is back (and the retry interval has passed).
        backend.fail = False
        app.state.keys._tried_at = None
        # A forged signature under a known key is final.
        forged = mint_token(kid=BACKEND_KID, key=NEXT_KEY)
        events = parse_ndjson(
            client.post("/v1/execute", json=execute_body(token=forged), headers=AUTH).content
        )
        assert events[-1]["retryable"] is False and "signature" in events[-1]["message"]
    assert provider.seen_tools == []
