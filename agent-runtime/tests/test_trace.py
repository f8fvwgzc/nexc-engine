"""Span lines: one per model turn, tool call and spawn, linked to their parents, never carrying
tool arguments or secrets (CONTRACT section 8)."""

from __future__ import annotations

import json
import re
import uuid
from collections.abc import Callable
from datetime import datetime
from typing import Any

from fastapi.testclient import TestClient

from nexc_runtime.api.app import create_app
from nexc_runtime.llm import LLMError
from nexc_runtime.trace import args_digest, new_span_id, rfc3339_ns

from .conftest import AUTH, FakeProvider, Step, call, execute_body, parse_ndjson

ClientFactory = Callable[..., tuple[TestClient, FakeProvider]]
RFC3339_NS = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{9}Z$")
SECRET_CONTENT = "the quarterly numbers are confidential: 4.2M"


def _spans(events: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [e for e in events if e["type"] == "span"]


def test_span_ids_are_ordered_uuid7_and_times_carry_nanoseconds() -> None:
    ids = [new_span_id() for _ in range(5000)]
    assert len(set(ids)) == len(ids)
    assert ids == sorted(ids), "ids made in order sort in order"
    assert all(uuid.UUID(i).version == 7 for i in ids)
    stamp = rfc3339_ns(1_792_238_858_123_456_789)
    assert RFC3339_NS.match(stamp)
    assert stamp.endswith(".123456789Z")
    digest, size = args_digest({"b": 1, "a": "é"})
    assert digest == args_digest({"a": "é", "b": 1})[0], "the digest ignores key order"
    canonical = json.dumps({"a": "é", "b": 1}, separators=(",", ":"), ensure_ascii=False)
    assert size == len(canonical.encode())


def test_turns_and_tool_calls_are_spans(client_for: ClientFactory) -> None:
    client, _ = client_for(
        [
            Step(
                calls=[
                    call("write_file", "c1", path="notes.md", content=SECRET_CONTENT),
                    call("read_file", "c2", path="missing.md"),
                ],
                tokens_in=120,
                tokens_out=30,
            ),
            Step(calls=[call("finish", answer="Done.")]),
        ]
    )
    raw = client.post("/v1/execute", json=execute_body(), headers=AUTH).content
    events = parse_ndjson(raw)
    spans = _spans(events)
    assert [(s["kind"], s["name"]) for s in spans] == [
        ("llm_call", "fake-model"),
        ("tool_call", "write_file"),
        ("tool_call", "read_file"),
        ("llm_call", "fake-model"),
        ("tool_call", "finish"),
    ]
    first_turn, write, read, second_turn, finish = spans
    assert first_turn["parent_id"] is None and first_turn["agent"] == "writer"
    assert write["parent_id"] == read["parent_id"] == first_turn["id"]
    assert finish["parent_id"] == second_turn["id"]
    assert first_turn["attributes"] == {
        "provider": "fake",
        "model": "fake-model",
        "turn": 1,
        "tokens_in": 120,
        "tokens_out": 30,
        "tokens_cached": 0,
        "stop_reason": "tool_use",
        "tool_calls": 2,
        "policy_version": 1,
    }
    assert write["status"] == "ok" and write["attributes"]["result_bytes"] > 0
    assert (
        write["attributes"]["args_sha256"]
        == args_digest({"path": "notes.md", "content": SECRET_CONTENT})[0]
    )
    assert read["status"] == "error" and read["attributes"]["is_error"] is True
    for span in spans:
        assert uuid.UUID(span["id"]).version == 7
        assert RFC3339_NS.match(span["started_at"]) and RFC3339_NS.match(span["ended_at"])
        start = datetime.fromisoformat(span["started_at"].replace("Z", "+00:00")[:26] + "+00:00")
        end = datetime.fromisoformat(span["ended_at"].replace("Z", "+00:00")[:26] + "+00:00")
        assert start <= end
    # Spans are agent work: every one of them comes before the artifacts and the result.
    types = [e["type"] for e in events]
    assert max(i for i, t in enumerate(types) if t == "span") < types.index("artifact")
    # Neither the tool arguments nor the request's API key ever travel in a span.
    span_text = json.dumps(spans)
    assert SECRET_CONTENT not in span_text and "notes.md" not in span_text
    assert "sk-ant-secret-value" not in span_text


def test_a_spawn_hangs_under_its_turn_and_the_child_under_the_spawn(
    client_for: ClientFactory,
) -> None:
    client, _ = client_for(
        [
            Step(
                calls=[call("spawn_subagent", role="researcher", task="Find three facts.")],
                tokens_in=1_000,
                tokens_out=0,
            ),
            Step(calls=[call("write_file", path="facts.md", content="# Facts\n")]),
            Step(text="Child done."),
            Step(calls=[call("finish", answer="Parent done.")]),
        ]
    )
    body = execute_body(agent={"budget_tokens": 101_000})
    spans = _spans(parse_ndjson(client.post("/v1/execute", json=body, headers=AUTH).content))
    by_kind = [(s["kind"], s["agent"]) for s in spans]
    assert by_kind == [
        ("llm_call", "writer"),
        ("spawn", "writer"),
        ("llm_call", "writer/researcher-1"),
        ("tool_call", "writer/researcher-1"),
        ("llm_call", "writer/researcher-1"),
        ("tool_call", "writer"),  # spawn_subagent ends when the child does
        ("llm_call", "writer"),
        ("tool_call", "writer"),
    ]
    parent_turn, spawn, child_turn, child_tool, child_last, spawn_call, _, _ = spans
    assert spawn["parent_id"] == parent_turn["id"]
    assert spawn["started_at"] == spawn["ended_at"], "a birth is an instant"
    assert spawn["attributes"] == {
        "child": "writer/researcher-1",
        "role": "researcher",
        "depth": 1,
        "budget_tokens": 50_000,
        "policy_version": 1,
    }
    assert child_turn["parent_id"] == child_last["parent_id"] == spawn["id"]
    assert child_tool["parent_id"] == child_turn["id"]
    assert spawn_call["name"] == "spawn_subagent" and spawn_call["parent_id"] == parent_turn["id"]
    assert "Find three facts" not in json.dumps(spans), "the sub-task is an argument"


def test_a_failed_model_call_is_an_error_span(client_for: ClientFactory) -> None:
    client, _ = client_for([Step(raise_error=LLMError("rate limited", retryable=True))])
    events = parse_ndjson(client.post("/v1/execute", json=execute_body(), headers=AUTH).content)
    (turn,) = _spans(events)
    assert turn["kind"] == "llm_call" and turn["status"] == "error"
    assert "tokens_in" not in turn["attributes"]
    assert events[-1]["type"] == "error"


def test_the_demo_provider_reports_spans(make_settings: Callable[..., Any]) -> None:
    with TestClient(create_app(make_settings())) as client:
        body = execute_body(llm={"provider": "demo", "api_key": None})
        events = parse_ndjson(client.post("/v1/execute", json=body, headers=AUTH).content)
    kinds = [(s["kind"], s["name"]) for s in _spans(events)]
    assert kinds[0] == ("llm_call", "nexc-demo")
    assert ("tool_call", "make_docx") in kinds
