"""The per-run token and the policy it carries: nothing starts without a valid token, a tool
outside the allow-list is refused with a `policy` span the model sees, a sub-agent is never wider
than its parent, and the decisions take microseconds (CONTRACT section 8)."""

from __future__ import annotations

import json
import os
import statistics
import time
from collections.abc import Callable
from typing import Any

import pytest
from fastapi.testclient import TestClient

from nexc_runtime.api.app import create_app
from nexc_runtime.policy import (
    AgentToken,
    Capabilities,
    TokenError,
    decide_model,
    decide_spawn,
    decide_tool,
    derive_key,
    encode_token,
    verify_token,
)

from .conftest import (
    AGENT_KEY,
    AUTH,
    FakeProvider,
    Step,
    call,
    execute_body,
    mint_token,
    parse_ndjson,
)

ClientFactory = Callable[..., tuple[TestClient, FakeProvider]]


def _spans(events: list[dict[str, Any]], kind: str | None = None) -> list[dict[str, Any]]:
    return [e for e in events if e["type"] == "span" and (kind is None or e["kind"] == kind)]


def _verified(**overrides: Any) -> AgentToken:
    return verify_token(mint_token(**overrides), AGENT_KEY, run_id="run-1", node_id="node-1")


# --- tokens -------------------------------------------------------------------------------------


def test_a_valid_token_carries_the_capabilities() -> None:
    token = _verified(
        capabilities={"tools": ["write_file", "finish"], "delegation": {"max_depth": 1}}
    )
    assert token.principal == "writer" and token.policy_version == 1 and token.depth == 0
    assert token.capabilities.tools == ("write_file", "finish")
    assert token.capabilities.max_depth == 1 and token.capabilities.max_fan_out == 4
    assert token.capabilities.labels == ("public", "internal")
    assert derive_key("t" * 40) == AGENT_KEY and len(AGENT_KEY) == 32


@pytest.mark.parametrize(
    ("token", "reason"),
    [
        (None, "no agent token"),
        ("", "no agent token"),
        ("not.a.jwt.at.all", "not a JWT"),
        (mint_token(key=b"another-key-another-key-another!"), "signature"),
        (mint_token(now=int(time.time()) - 3600, ttl_s=60), "expired"),
        (mint_token(now=int(time.time()) + 3600), "not valid yet"),
        (mint_token(run_id="run-2"), "another run or node"),
        (mint_token(node_id="node-2"), "another run or node"),
        (mint_token(capabilities={"tools": "write_file"}), "not a list of strings"),
    ],
)
def test_forged_expired_and_foreign_tokens_are_refused(token: str | None, reason: str) -> None:
    with pytest.raises(TokenError, match=reason):
        verify_token(token, AGENT_KEY, run_id="run-1", node_id="node-1")


def test_a_tampered_payload_does_not_verify() -> None:
    good = mint_token(capabilities={"tools": ["finish"]})
    header, payload, signature = good.split(".")
    # The same signature over a payload that allows everything.
    claims = json.loads(__import__("base64").urlsafe_b64decode(payload + "=="))
    claims["capabilities"] = {}
    widened = encode_token(claims, AGENT_KEY).split(".")[1]
    with pytest.raises(TokenError, match="signature"):
        verify_token(f"{header}.{widened}.{signature}", AGENT_KEY, run_id="run-1", node_id="node-1")
    # `alg: none` is not an algorithm.
    none_header = __import__("base64").urlsafe_b64encode(b'{"alg":"none"}').rstrip(b"=").decode()
    with pytest.raises(TokenError, match="HS256"):
        verify_token(f"{none_header}.{payload}.", AGENT_KEY, run_id="run-1", node_id="node-1")
    # A token for another runtime, signed with the right key.
    foreign = json.loads(__import__("base64").urlsafe_b64decode(payload + "=="))
    foreign["aud"] = "nexc-api"
    with pytest.raises(TokenError, match="not for this runtime"):
        verify_token(encode_token(foreign, AGENT_KEY), AGENT_KEY, run_id="run-1", node_id="node-1")


def test_the_run_refuses_to_start_without_a_valid_token(client_for: ClientFactory) -> None:
    client, provider = client_for([Step(calls=[call("finish", answer="never")])])
    for token in (None, "garbage", mint_token(key=b"x" * 32), mint_token(run_id="other")):
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


def test_a_child_token_is_narrowed_and_signed() -> None:
    parent = _verified(capabilities={"tools": ["finish"], "delegation": {"max_depth": 1}})
    child = parent.child("writer/researcher-1", AGENT_KEY)
    assert child.depth == 1 and child.principal == "writer/researcher-1"
    assert child.capabilities.is_within(parent.capabilities)
    assert child.expires_at == parent.expires_at
    again = verify_token(child.raw, AGENT_KEY, run_id="run-1", node_id="node-1")
    assert again.capabilities == child.capabilities and again.depth == 1
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
