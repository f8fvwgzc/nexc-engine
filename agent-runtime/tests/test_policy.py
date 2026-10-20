"""The per-run token and the policy it carries: nothing starts without a valid token, a tool
outside the allow-list is refused with a `policy` span the model sees, a sub-agent is never wider
than its parent, and the decisions take microseconds (CONTRACT section 8)."""

from __future__ import annotations

import asyncio
import hashlib
import json
import os
import statistics
import time
from collections.abc import Callable
from typing import Any

import httpx
import pytest
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from fastapi.testclient import TestClient

from nexc_runtime import policy as policy_module
from nexc_runtime.api.app import create_app
from nexc_runtime.api.schemas import ExecuteRequest
from nexc_runtime.config import Settings
from nexc_runtime.keys import keys_from
from nexc_runtime.policy import (
    AgentToken,
    Capabilities,
    Rules,
    TokenError,
    decide_model,
    decide_model_with,
    decide_spawn,
    decide_tool,
    decide_tool_with,
    verify_token,
)
from nexc_runtime.runner import execute
from nexc_runtime.streaming import EventStream

from .conftest import (
    AUTH,
    BACKEND_KID,
    BACKEND_PUBLIC,
    HUMAN,
    TOKEN,
    FakeProvider,
    Step,
    call,
    execute_body,
    hs256_token,
    mint_token,
    parse_ndjson,
    sign_token,
    token_claims,
)

# The runtime's keys in these tests: the backend's public key alone.
KEYS = keys_from([BACKEND_PUBLIC])
# Another Ed25519 key: whoever holds it is not the backend.
STRANGER = Ed25519PrivateKey.from_private_bytes(b"s" * 32)
# The HS256 key the backend and the runtime used to share, rebuilt from the bearer secret.
OLD_SHARED_KEY = hashlib.sha256(b"nexc-agent-token-v1:" + TOKEN.encode()).digest()

ClientFactory = Callable[..., tuple[TestClient, FakeProvider]]


def _spans(events: list[dict[str, Any]], kind: str | None = None) -> list[dict[str, Any]]:
    return [e for e in events if e["type"] == "span" and (kind is None or e["kind"] == kind)]


def _verified(**overrides: Any) -> AgentToken:
    return verify_token(mint_token(**overrides), KEYS, run_id="run-1", node_id="node-1")


def test_the_recording_claim_is_read_and_passed_to_sub_agents() -> None:
    token = _verified()
    assert not token.recording.on
    token = _verified(recording={"prompts": True, "tool_io": True})
    assert token.recording.on
    assert token.recording.prompts and token.recording.tool_io
    assert not token.recording.completions
    child = token.child("writer/researcher-1")
    assert child.recording == token.recording
    with pytest.raises(TokenError):
        _verified(recording="yes")


# --- tokens -------------------------------------------------------------------------------------


def test_a_valid_token_carries_the_capabilities_and_the_human() -> None:
    token = _verified(
        capabilities={"tools": ["write_file", "finish"], "delegation": {"max_depth": 1}}
    )
    assert token.principal == "writer" and token.policy_version == 1 and token.depth == 0
    assert token.capabilities.tools == ("write_file", "finish")
    assert token.capabilities.max_depth == 1 and token.capabilities.max_fan_out == 4
    assert token.capabilities.labels == ("public", "internal")
    assert token.on_behalf_of is not None
    assert (token.on_behalf_of.user_id, token.on_behalf_of.workspace_role) == (
        HUMAN["user_id"],
        "admin",
    )
    personal = _verified(on_behalf_of={"user_id": HUMAN["user_id"], "workspace_role": None})
    assert personal.on_behalf_of is not None and personal.on_behalf_of.workspace_role is None


def _forged(claims: dict[str, Any]) -> list[tuple[str, str]]:
    """What someone without the backend's private key can produce, and why each is refused."""
    return [
        # HS256 under the key the runtime could derive from its own bearer secret, or under the
        # bearer secret itself: the hole this design closes. Naming the backend's kid changes
        # nothing.
        (hs256_token(claims, OLD_SHARED_KEY), "not EdDSA"),
        (hs256_token(claims, TOKEN.encode()), "not EdDSA"),
        (hs256_token(claims, OLD_SHARED_KEY, kid=None), "not EdDSA"),
        # HS256 keyed with the public key (algorithm confusion).
        (hs256_token(claims, BACKEND_PUBLIC), "not EdDSA"),
        # Another Ed25519 key, under its own id or under the backend's.
        (sign_token(claims, STRANGER), "does not know"),
        (sign_token(claims, STRANGER, kid=BACKEND_KID), "signature"),
        # No key id at all.
        (sign_token(claims, header={"alg": "EdDSA"}), "names no key"),
        # `alg: none`.
        (sign_token(claims, header={"alg": "none", "kid": BACKEND_KID}), "not EdDSA"),
    ]


@pytest.mark.parametrize(("token", "reason"), _forged(token_claims()))
def test_a_token_signed_with_anything_but_the_backends_key_is_refused(
    token: str, reason: str
) -> None:
    with pytest.raises(TokenError, match=reason):
        verify_token(token, KEYS, run_id="run-1", node_id="node-1")


@pytest.mark.parametrize(
    ("token", "reason"),
    [
        (None, "no agent token"),
        ("", "no agent token"),
        ("not.a.jwt.at.all", "not a JWT"),
        (mint_token(now=int(time.time()) - 3600, ttl_s=60), "expired"),
        (mint_token(now=int(time.time()) + 3600), "not valid yet"),
        (mint_token(run_id="run-2"), "another run or node"),
        (mint_token(node_id="node-2"), "another run or node"),
        (mint_token(capabilities={"tools": "write_file"}), "not a list of strings"),
        (mint_token(on_behalf_of=None), "acts for"),
        (mint_token(on_behalf_of={"workspace_role": "owner"}), "user id"),
        (mint_token(on_behalf_of={"user_id": "u", "workspace_role": 3}), "not a role"),
    ],
)
def test_expired_foreign_and_incomplete_tokens_are_refused(token: str | None, reason: str) -> None:
    with pytest.raises(TokenError, match=reason):
        verify_token(token, KEYS, run_id="run-1", node_id="node-1")


def test_a_tampered_payload_does_not_verify() -> None:
    good = mint_token(capabilities={"tools": ["finish"]})
    header, payload, signature = good.split(".")
    # The same signature over a payload that allows everything.
    claims = json.loads(__import__("base64").urlsafe_b64decode(payload + "=="))
    claims["capabilities"] = {}
    widened = sign_token(claims, STRANGER).split(".")[1]
    with pytest.raises(TokenError, match="signature"):
        verify_token(f"{header}.{widened}.{signature}", KEYS, run_id="run-1", node_id="node-1")
    # A token for another runtime, signed with the right key.
    foreign = json.loads(__import__("base64").urlsafe_b64decode(payload + "=="))
    foreign["aud"] = "nexc-api"
    with pytest.raises(TokenError, match="not for this runtime"):
        verify_token(sign_token(foreign), KEYS, run_id="run-1", node_id="node-1")


def test_the_run_refuses_to_start_without_a_valid_token(client_for: ClientFactory) -> None:
    client, provider = client_for([Step(calls=[call("finish", answer="never")])])
    forged = [token for token, _ in _forged(token_claims())]
    for token in (None, "garbage", mint_token(run_id="other"), *forged):
        body = execute_body()
        body["token"] = token
        events = parse_ndjson(client.post("/v1/execute", json=body, headers=AUTH).content)
        assert events[-1]["type"] == "error" and events[-1]["retryable"] is False
        assert events[-1]["message"].startswith("agent token refused")
        assert not _spans(events), "no agent was born"
    assert provider.seen_tools == [], "the model was never called"


# --- decisions ----------------------------------------------------------------------------------


def test_decisions_name_their_rule() -> None:
    caps = Capabilities.from_dict(
        {
            "tools": ["write_file", "finish"],
            "counterparts": ["researcher"],
            "delegation": {"max_depth": 1, "max_fan_out": 2},
            "models": ["claude-opus-5"],
        }
    )
    assert decide_tool(caps, "finish").allowed
    denied = decide_tool(caps, "make_docx")
    assert (denied.allowed, denied.rule, denied.verdict) == (False, "tools.allow", "deny")
    assert decide_spawn(caps, "researcher", depth=1, children=0).allowed
    assert decide_spawn(caps, "researcher", depth=2, children=0).rule == "delegation.max_depth"
    assert decide_spawn(caps, "researcher", depth=1, children=2).rule == "delegation.max_fan_out"
    assert decide_spawn(caps, "editor", depth=1, children=0).rule == "counterparts"
    assert decide_model(caps, "claude-opus-5").allowed
    assert decide_model(caps, "gpt").rule == "models.allow"
    everything = Capabilities.from_dict({})
    assert decide_tool(everything, "anything").allowed
    assert decide_model(everything, "anything").allowed


def test_narrowing_only_removes() -> None:
    parent = Capabilities.from_dict(
        {
            "tools": ["write_file", "finish"],
            "data": {"labels": ["public", "internal", "restricted"]},
        }
    )
    greedy = Capabilities.from_dict(
        {"tools": ["write_file", "run_python"], "delegation": {"max_depth": 8, "max_fan_out": 1}}
    )
    child = parent.narrow(greedy)
    assert child.tools == ("write_file",)
    assert child.labels == ("public", "internal")
    assert (child.max_depth, child.max_fan_out) == (2, 1)
    assert child.is_within(parent) and not greedy.is_within(parent)
    assert Capabilities.from_dict(child.to_dict()) == child, "the document round-trips"


# --- enforcement in the tool loop -----------------------------------------------------------------


def test_a_tool_outside_the_allow_list_is_refused_and_recorded(client_for: ClientFactory) -> None:
    sections = [{"heading": "Intro", "paragraphs": ["Hello"], "bullets": [], "table_rows": []}]
    client, provider = client_for(
        [
            Step(calls=[call("make_docx", "c1", filename="r.docx", title="R", sections=sections)]),
            Step(calls=[call("write_file", "c2", path="r.md", content="# R\n")]),
            Step(calls=[call("finish", answer="Wrote r.md instead.")]),
        ]
    )
    body = execute_body(capabilities={"tools": ["write_file", "read_file", "finish"]})
    events = parse_ndjson(client.post("/v1/execute", json=body, headers=AUTH).content)
    # The model saw the refusal and went on.
    refused = provider.tool_results[0]
    assert refused.is_error and "tools.allow" in refused.content
    assert events[-1] == {
        "type": "result",
        "output": "Wrote r.md instead.",
        "tokens_in": events[-1]["tokens_in"],
        "tokens_out": events[-1]["tokens_out"],
        "tokens_cached": 0,
    }
    # The refusal is a `policy` span under the turn that asked; the tool never ran.
    (policy,) = _spans(events, "policy")
    turns = _spans(events, "llm_call")
    assert policy["parent_id"] == turns[0]["id"]
    assert policy["name"] == "make_docx" and policy["status"] == "error"
    assert policy["attributes"] == {
        "decision": "deny",
        "rule": "tools.allow",
        "reason": "tool make_docx is not allowed",
        "action": "tool_call",
        "subject": "make_docx",
        "policy_version": 1,
    }
    assert [s["name"] for s in _spans(events, "tool_call")] == ["write_file", "finish"]
    assert all(s["attributes"]["policy_version"] == 1 for s in _spans(events, "tool_call"))
    paths = [e["path"] for e in events if e["type"] == "artifact"]
    assert paths == ["r.md"], "no .docx was made"
    assert any(e["type"] == "log" and "refused by policy" in e["message"] for e in events)


def test_a_sub_agent_is_never_wider_than_its_parent(client_for: ClientFactory) -> None:
    client, provider = client_for(
        [
            # The parent delegates; the child tries the parent's forbidden tool, then finishes.
            Step(calls=[call("spawn_subagent", role="researcher", task="Find facts.")]),
            Step(calls=[call("make_docx", "c1", filename="x.docx", title="X", sections=[])]),
            Step(text="Child done without the docx."),
            # The parent may not delegate to an editor; a second researcher is fine...
            Step(calls=[call("spawn_subagent", role="editor", task="Polish.")]),
            Step(calls=[call("spawn_subagent", role="researcher", task="More facts.")]),
            Step(text="Second child done."),
            # ...but a third sub-agent is one more than the fan-out allows.
            Step(calls=[call("spawn_subagent", role="researcher", task="Even more.")]),
            Step(calls=[call("finish", answer="Parent done.")]),
        ]
    )
    body = execute_body(
        agent={"budget_tokens": 101_000},
        task={"kind": "task"},
        capabilities={
            "tools": ["write_file", "spawn_subagent", "finish"],
            "counterparts": ["researcher"],
            "delegation": {"max_depth": 2, "max_fan_out": 2},
        },
    )
    events = parse_ndjson(client.post("/v1/execute", json=body, headers=AUTH).content)
    assert events[-1]["output"] == "Parent done."
    policies = _spans(events, "policy")
    assert [(p["agent"], p["name"], p["attributes"]["rule"]) for p in policies] == [
        ("writer/researcher-1", "make_docx", "tools.allow"),
        ("writer", "editor", "counterparts"),
        ("writer", "researcher", "delegation.max_fan_out"),
    ]
    assert policies[1]["attributes"]["action"] == "spawn"
    assert (policies[1]["attributes"]["depth"], policies[1]["attributes"]["children"]) == (1, 1)
    assert policies[2]["attributes"]["children"] == 2
    # Each refusal reached the model that asked, as a tool error naming the rule.
    results = provider.tool_results
    assert results[0].is_error and "tools.allow" in results[0].content
    assert results[1].content == "Child done without the docx."
    assert results[2].is_error and "counterparts" in results[2].content
    assert results[3].content == "Second child done."
    assert results[4].is_error and "max_fan_out" in results[4].content
    spawns = _spans(events, "spawn")
    assert [s["name"] for s in spawns] == ["writer/researcher-1", "writer/researcher-2"]
    assert all(s["attributes"]["policy_version"] == 1 for s in spawns)
    # A child at depth 1 may still delegate under a depth limit of 2.
    assert "spawn_subagent" in provider.seen_tools[1]


def test_a_child_token_is_narrowed_in_process_and_never_signed() -> None:
    parent = _verified(capabilities={"tools": ["finish"], "delegation": {"max_depth": 1}})
    child = parent.child("writer/researcher-1")
    assert child.depth == 1 and child.principal == "writer/researcher-1"
    assert child.claims["sub"] == "writer/researcher-1" and child.claims["depth"] == 1
    assert child.capabilities.is_within(parent.capabilities)
    assert child.expires_at == parent.expires_at
    assert child.on_behalf_of == parent.on_behalf_of, "the same human"
    # The runtime holds no key that signs: the child's only signed token is its root's.
    assert child.raw == parent.raw
    again = verify_token(child.raw, KEYS, run_id="run-1", node_id="node-1")
    assert again.depth == 0 and again.principal == "writer"
    assert not any(hasattr(policy_module, name) for name in ("encode_token", "derive_key"))
    # The depth limit means the grandchild may not be born.
    assert (
        decide_spawn(child.capabilities, "qa", depth=2, children=0).rule == "delegation.max_depth"
    )


def test_a_model_outside_the_allow_list_never_runs(client_for: ClientFactory) -> None:
    client, provider = client_for([Step(calls=[call("finish", answer="never")])])
    body = execute_body(capabilities={"models": ["claude-opus-5"]})
    events = parse_ndjson(client.post("/v1/execute", json=body, headers=AUTH).content)
    assert events[-1]["type"] == "error" and events[-1]["retryable"] is False
    assert "model fake-model is not allowed" in events[-1]["message"]
    assert provider.seen_tools == []


def test_spans_are_attributed_to_the_tokens_principal(client_for: ClientFactory) -> None:
    client, _ = client_for([Step(calls=[call("finish", answer="ok")])])
    body = execute_body()
    body["token"] = mint_token(principal="agent://ws/Writer")
    events = parse_ndjson(client.post("/v1/execute", json=body, headers=AUTH).content)
    assert {s["agent"] for s in _spans(events)} == {"agent://ws/Writer"}
    assert any(e["type"] == "log" and "as agent://ws/Writer" in e["message"] for e in events)


def test_the_demo_provider_is_refused_make_docx_and_still_delivers(
    make_settings: Callable[..., Any],
) -> None:
    """The demo: an agent whose policy excludes `make_docx` is refused it, the refusal is a
    `policy` span, and the run still ends with a result and no document it may not make."""
    with TestClient(create_app(make_settings())) as client:
        body = execute_body(
            llm={"provider": "demo", "api_key": None},
            capabilities={"tools": ["write_file", "read_file", "list_files", "finish"]},
        )
        events = parse_ndjson(client.post("/v1/execute", json=body, headers=AUTH).content)
    assert events[-1]["type"] == "result"
    (policy,) = _spans(events, "policy")
    assert (policy["name"], policy["attributes"]["rule"]) == ("make_docx", "tools.allow")
    assert "make_docx" not in [s["name"] for s in _spans(events, "tool_call")]
    assert not any(e["type"] == "artifact" and e["path"].endswith(".docx") for e in events)
    assert any(e["type"] == "log" and "does not allow make_docx" in e["message"] for e in events)


# --- rules: deny-lists, approvals and the pace (AgentSecOps phase 2) -----------------------------

SECTIONS = [{"heading": "Intro", "paragraphs": ["Hello"], "bullets": [], "table_rows": []}]
DOCX = call("make_docx", "c1", filename="r.docx", title="R", sections=SECTIONS)


def test_rules_are_read_from_the_token_and_decide_with_the_capabilities() -> None:
    token = _verified(
        rules={
            "tools_denied": ["run_python"],
            "models_denied": ["gpt-x"],
            "require_approval": {
                "tools": ["make_docx"],
                "labels": ["confidential"],
                "timeout_s": 30,
            },
            "limits": {"tokens_per_run": 5000, "tool_calls_per_minute": 2},
            "injection": {"action": "block", "model_check": True},
        },
        labels=["internal", "confidential"],
    )
    rules = token.rules
    assert rules.tools_denied == ("run_python",)
    assert rules.approval_tools == ("make_docx",) and rules.approval_timeout_s == 30
    assert rules.tokens_per_run == 5000 and rules.tool_calls_per_minute == 2
    assert rules.injection_action == "block" and rules.injection_model_check
    assert token.labels == ("internal", "confidential")
    caps = token.capabilities
    denied = decide_tool_with(caps, rules, token.labels, "run_python")
    assert (denied.allowed, denied.rule, denied.verdict) == (False, "tools.deny", "deny")
    held = decide_tool_with(caps, rules, token.labels, "make_docx")
    assert held.needs_approval and held.rule == "approvals.tools"
    assert held.verdict == "require_approval" and not held.allowed
    labelled = decide_tool_with(caps, rules, token.labels, "write_file")
    assert labelled.needs_approval and labelled.rule == "approvals.labels"
    assert "confidential" in labelled.reason
    assert decide_tool_with(caps, rules, token.labels, "read_file").allowed
    assert decide_tool_with(caps, rules, (), "write_file").allowed, "no labelled data: no hold"
    assert decide_model_with(caps, rules, "gpt-x").rule == "models.deny"
    assert decide_model_with(caps, rules, "claude").allowed
    # A token without rules reads as the defaults; a child keeps its parent's rules and labels.
    plain = _verified()
    assert plain.rules == Rules() and plain.labels == ()
    child = token.child("writer/researcher-1")
    assert child.rules == rules and child.labels == token.labels
    assert child.rules.approval_tools == ("make_docx",)
    with pytest.raises(TokenError):
        _verified(rules={"limits": {"tokens_per_run": "lots"}})
    with pytest.raises(TokenError):
        _verified(rules={"injection": {"action": "shrug"}})


def test_a_denied_tool_and_a_hurried_agent_are_refused_by_their_rule(
    client_for: ClientFactory,
) -> None:
    client, provider = client_for(
        [
            Step(calls=[DOCX]),
            Step(calls=[call("write_file", "c2", path="a.md", content="a")]),
            Step(calls=[call("write_file", "c3", path="b.md", content="b")]),
            Step(calls=[call("finish", answer="Done.")]),
        ]
    )
    body = execute_body(
        rules={"tools_denied": ["make_docx"], "limits": {"tool_calls_per_minute": 1}}
    )
    events = parse_ndjson(client.post("/v1/execute", json=body, headers=AUTH).content)
    rules = [s["attributes"]["rule"] for s in _spans(events, "policy")]
    assert rules == ["tools.deny", "limits.tool_calls_per_minute"]
    assert provider.tool_results[0].is_error and "tools.deny" in provider.tool_results[0].content
    assert not provider.tool_results[1].is_error, "the first call of the minute runs"
    assert provider.tool_results[2].is_error, "the second is refused"
    assert "tool_calls_per_minute" in provider.tool_results[2].content
    assert [s["name"] for s in _spans(events, "tool_call")] == ["write_file", "finish"]
    paths = [e["path"] for e in events if e["type"] == "artifact"]
    assert paths == ["a.md"]


async def _start(
    make_settings: Callable[..., Settings], steps: list[Step], body: dict[str, Any]
) -> tuple[Any, FakeProvider, EventStream, asyncio.Task[None]]:
    """Runs a request on the runtime itself, with decisions arriving through its HTTP API."""
    provider = FakeProvider(steps)
    app = create_app(make_settings(), provider_factory=lambda _r, _s: provider)
    events = EventStream()
    task = asyncio.create_task(
        execute(
            ExecuteRequest.model_validate(body),
            settings=app.state.settings,
            census=app.state.census,
            events=events,
            keys=app.state.keys,
            provider_factory=app.state.provider_factory,
            approvals=app.state.approvals,
        )
    )
    return app, provider, events, task


async def _until_request(lines: Any) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    seen: list[dict[str, Any]] = []
    async for raw in lines:
        event = json.loads(raw)
        seen.append(event)
        if event["type"] == "span" and event["kind"] == "policy":
            return seen, event
    raise AssertionError(f"no approval was asked for: {seen}")


async def test_a_held_tool_waits_for_the_decision_and_runs_once_approved(
    make_settings: Callable[..., Settings],
) -> None:
    body = execute_body(rules={"require_approval": {"tools": ["make_docx"], "timeout_s": 30}})
    app, provider, events, task = await _start(
        make_settings, [Step(calls=[DOCX]), Step(calls=[call("finish", answer="Done.")])], body
    )
    lines = events.lines()
    seen, request = await _until_request(lines)
    assert request["status"] == "ok" and request["name"] == "make_docx"
    attrs = request["attributes"]
    assert attrs["decision"] == "require_approval" and attrs["rule"] == "approvals.tools"
    assert attrs["action"] == "tool_call" and attrs["subject"] == "make_docx"
    assert len(attrs["args_sha256"]) == 64 and attrs["policy_version"] == 1
    approval_id = attrs["approval_id"]
    assert app.state.approvals.pending == 1
    assert any(e["type"] == "log" and "waits for an approval" in e["message"] for e in seen)
    assert not provider.tool_results, "the tool has not run"
    transport = httpx.ASGITransport(app=app)
    async with httpx.AsyncClient(transport=transport, base_url="http://runtime") as http:
        decision = {"decision": "approved", "reason": "fine", "decided_by": "Ada"}
        assert (await http.post(f"/v1/approvals/{approval_id}", json=decision)).status_code == 401
        unknown = await http.post("/v1/approvals/not-waiting", json=decision, headers=AUTH)
        assert unknown.status_code == 404
        bad = await http.post(
            f"/v1/approvals/{approval_id}", json={"decision": "maybe"}, headers=AUTH
        )
        assert bad.status_code == 422
        delivered = await http.post(f"/v1/approvals/{approval_id}", json=decision, headers=AUTH)
        assert delivered.status_code == 200 and delivered.json() == {"status": "delivered"}
        again = await http.post(f"/v1/approvals/{approval_id}", json=decision, headers=AUTH)
        assert again.status_code == 404, "a decision is delivered once"
    async for raw in lines:
        seen.append(json.loads(raw))
    await task
    assert seen[-1]["type"] == "result" and seen[-1]["output"] == "Done."
    assert app.state.approvals.pending == 0
    (docx,) = [s for s in _spans(seen, "tool_call") if s["name"] == "make_docx"]
    assert docx["attributes"]["approval_id"] == approval_id, "the call names its approval"
    assert docx["status"] == "ok"
    assert [e["path"] for e in seen if e["type"] == "artifact"] == ["r.docx"]
    assert any(e["type"] == "log" and "approved by Ada" in e["message"] for e in seen)


async def test_a_denied_call_is_refused_with_the_reason_and_the_run_goes_on(
    make_settings: Callable[..., Settings],
) -> None:
    body = execute_body(
        rules={"require_approval": {"tools": ["make_docx"], "timeout_s": 30}},
        task={"kind": "task"},
    )
    app, provider, events, task = await _start(
        make_settings,
        [Step(calls=[DOCX]), Step(calls=[call("finish", answer="Wrote nothing.")])],
        body,
    )
    lines = events.lines()
    seen, request = await _until_request(lines)
    approval_id = request["attributes"]["approval_id"]
    transport = httpx.ASGITransport(app=app)
    async with httpx.AsyncClient(transport=transport, base_url="http://runtime") as http:
        decision = {"decision": "denied", "reason": "not this quarter", "decided_by": "Ada"}
        assert (
            await http.post(f"/v1/approvals/{approval_id}", json=decision, headers=AUTH)
        ).status_code == 200
    async for raw in lines:
        seen.append(json.loads(raw))
    await task
    assert seen[-1] == {
        "type": "result",
        "output": "Wrote nothing.",
        "tokens_in": seen[-1]["tokens_in"],
        "tokens_out": seen[-1]["tokens_out"],
        "tokens_cached": 0,
    }
    refused = provider.tool_results[0]
    assert refused.is_error and refused.content == "make_docx was denied by Ada: not this quarter"
    assert not any(s["name"] == "make_docx" for s in _spans(seen, "tool_call"))
    assert not any(e["type"] == "artifact" for e in seen)


async def test_a_call_nobody_decides_fails_the_node_when_the_wait_runs_out(
    make_settings: Callable[..., Settings],
) -> None:
    body = execute_body(rules={"require_approval": {"tools": ["make_docx"], "timeout_s": 1}})
    app, provider, events, task = await _start(make_settings, [Step(calls=[DOCX])], body)
    seen = [json.loads(raw) async for raw in events.lines()]
    await task
    assert seen[-1] == {
        "type": "error",
        "message": "approval for make_docx not given within 1s",
        "retryable": False,
    }
    assert app.state.approvals.pending == 0, "the wait is forgotten"
    assert not provider.tool_results


def test_the_document_fallback_never_makes_what_waits_for_an_approval(
    client_for: ClientFactory,
) -> None:
    client, _ = client_for([Step(text="# Report\n\nAll done in text.")])
    body = execute_body(rules={"require_approval": {"tools": ["make_docx"]}})
    events = parse_ndjson(client.post("/v1/execute", json=body, headers=AUTH).content)
    assert events[-1]["type"] == "result"
    assert not any(e["type"] == "artifact" for e in events), "no .docx behind the approval's back"
    assert any(
        e["type"] == "log" and "approvals.tools" in e["message"]
        for e in events
        if e["type"] == "log"
    )


# --- the measurement behind `make prove-identity` -------------------------------------------------


@pytest.mark.prove
def test_prove_identity_decisions_take_microseconds() -> None:
    """Prints the in-process decision latency over 100,000 decisions (p50, p99, max)."""
    caps = Capabilities.from_dict(
        {
            "tools": ["write_file", "read_file", "list_files", "make_docx", "finish"],
            "counterparts": ["researcher", "editor"],
            "models": ["claude-opus-5"],
        }
    )
    tools = ("write_file", "run_python", "finish", "make_docx", "spawn_subagent")
    samples: list[float] = []
    for i in range(100_000):
        tool = tools[i % len(tools)]
        start = time.perf_counter_ns()
        decide_tool(caps, tool)
        decide_spawn(caps, "researcher", depth=1, children=i % 5)
        samples.append((time.perf_counter_ns() - start) / 1_000)
    samples.sort()
    p50 = statistics.median(samples)
    p99 = samples[int(len(samples) * 0.99)]
    print(
        f"\nprove-identity (runtime, Python): {len(samples)} decision pairs: "
        f"p50 {p50:.2f} µs, p99 {p99:.2f} µs, max {samples[-1]:.2f} µs"
    )
    budget = float(os.environ.get("NEXC_PROVE_P99_US", "100"))
    assert p99 < budget, f"p99 {p99:.2f} µs is over the {budget} µs target"


@pytest.mark.prove
def test_prove_policy_decisions_under_rules_take_microseconds() -> None:
    """Prints the in-process decision latency under resolved rules over 100,000 decisions."""
    caps = Capabilities.from_dict(
        {"tools": ["write_file", "read_file", "list_files", "make_docx", "finish"]}
    )
    rules = Rules.from_dict(
        {
            "tools_denied": ["run_python"],
            "egress_denied": ["*.evil.example"],
            "require_approval": {"tools": ["make_docx"], "labels": ["confidential"]},
            "limits": {"tokens_per_run": 30000, "tool_calls_per_minute": 30},
        }
    )
    labels = ("internal", "confidential")
    tools = ("write_file", "run_python", "finish", "make_docx", "read_file")
    samples: list[float] = []
    for i in range(100_000):
        tool = tools[i % len(tools)]
        start = time.perf_counter_ns()
        decide_tool_with(caps, rules, labels, tool)
        samples.append((time.perf_counter_ns() - start) / 1_000)
    samples.sort()
    p50 = statistics.median(samples)
    p99 = samples[int(len(samples) * 0.99)]
    print(
        f"\nprove-policy (runtime, Python): {len(samples)} decisions under rules: "
        f"p50 {p50:.2f} µs, p99 {p99:.2f} µs, max {samples[-1]:.2f} µs"
    )
    budget = float(os.environ.get("NEXC_PROVE_P99_US", "100"))
    assert p99 < budget, f"p99 {p99:.2f} µs is over the {budget} µs target"
