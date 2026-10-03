"""HTTP surface: auth, health, validation hygiene, body limits and NDJSON streams."""

from __future__ import annotations

import base64
import io
from collections.abc import Callable
from typing import Any

import docx
import pytest
from fastapi.testclient import TestClient

from nexc_runtime.api.app import create_app
from nexc_runtime.llm import LLMError

from .conftest import AUTH, FakeProvider, Step, call, execute_body, parse_ndjson

ClientFactory = Callable[..., tuple[TestClient, FakeProvider]]
TERMINAL = {"result", "error"}


def test_healthz_is_public(client_for: ClientFactory) -> None:
    client, _ = client_for([])
    response = client.get("/healthz")
    assert response.status_code == 200
    body = response.json()
    assert body["status"] == "ok"
    assert isinstance(body["version"], str)
    assert body["agents_loaded"] == 0


@pytest.mark.parametrize(
    "headers",
    [
        {},
        {"Authorization": "Bearer wrong-token-wrong-token-wrong-token!!"},
        {"Authorization": "Basic " + "t" * 40},
        {"Authorization": "Bearer"},
    ],
)
def test_execute_requires_valid_bearer(client_for: ClientFactory, headers: dict[str, str]) -> None:
    client, provider = client_for([Step(text="hi")])
    response = client.post("/v1/execute", json=execute_body(), headers=headers)
    assert response.status_code == 401
    assert response.headers["www-authenticate"] == "Bearer"
    assert provider.steps, "the model must not be called without auth"


def test_validation_errors_never_echo_the_api_key(client_for: ClientFactory) -> None:
    client, _ = client_for([])
    body = execute_body(task={"title": "", "kind": "not-a-kind"})
    response = client.post("/v1/execute", json=body, headers=AUTH)
    assert response.status_code == 422
    assert "sk-ant-secret-value" not in response.text


def test_oversized_body_is_rejected(client_for: ClientFactory) -> None:
    client, _ = client_for([], runtime_max_request_bytes=2048)
    body = execute_body(task={"content": "x" * 10_000})
    response = client.post("/v1/execute", json=body, headers=AUTH)
    assert response.status_code == 413


def _run(client: TestClient, **overrides: Any) -> list[dict[str, Any]]:
    response = client.post("/v1/execute", json=execute_body(**overrides), headers=AUTH)
    assert response.status_code == 200
    assert response.headers["content-type"].startswith("application/x-ndjson")
    return parse_ndjson(response.content)


def test_stream_shape_and_ordering(client_for: ClientFactory) -> None:
    sections = [{"heading": "Intro", "paragraphs": ["Hello"], "bullets": [], "table_rows": []}]
    client, provider = client_for(
        [
            Step(
                text="Drafting. ",
                calls=[call("make_docx", filename="r.docx", title="R", sections=sections)],
            ),
            Step(calls=[call("write_file", path="notes/summary.md", content="# Summary\n")]),
            Step(calls=[call("finish", answer="All done.")]),
        ]
    )
    events = _run(client)

    types = [e["type"] for e in events]
    assert types[-1] == "result"
    assert sum(t in TERMINAL for t in types) == 1
    assert types[0] == "log"
    assert "delta" in types and "tokens" in types
    # artifacts come after all agent work and right before the result
    first_artifact = types.index("artifact")
    assert all(t in ("artifact", "log", "result") for t in types[first_artifact:])

    allowed = {
        "log": {"type", "level", "message"},
        "delta": {"type", "text"},
        "tokens": {"type", "input", "output"},
        "spawn": {"type", "agent"},
        "artifact": {"type", "path", "mime", "content_b64"},
        "result": {"type", "output", "tokens_in", "tokens_out"},
        "error": {"type", "message", "retryable"},
    }
    for event in events:
        assert set(event) == allowed[event["type"]], event

    artifacts = {e["path"]: e for e in events if e["type"] == "artifact"}
    assert set(artifacts) == {"r.docx", "notes/summary.md"}
    assert artifacts["notes/summary.md"]["mime"] == "text/markdown"
    assert artifacts["r.docx"]["mime"].endswith("wordprocessingml.document")
    opened = docx.Document(io.BytesIO(base64.b64decode(artifacts["r.docx"]["content_b64"])))
    assert opened.paragraphs[0].text == "R"

    result = events[-1]
    assert result["output"] == "All done."
    assert result["tokens_in"] == 300 and result["tokens_out"] == 60
    tokens = [e for e in events if e["type"] == "tokens"]
    assert sum(e["input"] for e in tokens) == result["tokens_in"]
    assert provider.closed


def test_llm_error_ends_stream_with_single_retryable_error(client_for: ClientFactory) -> None:
    client, _ = client_for([Step(raise_error=LLMError("rate limited", retryable=True))])
    events = _run(client)
    assert events[-1] == {"type": "error", "message": "rate limited", "retryable": True}
    assert sum(e["type"] in TERMINAL for e in events) == 1
    assert not any(e["type"] == "artifact" for e in events)


def test_refusal_is_a_non_retryable_error(client_for: ClientFactory) -> None:
    client, _ = client_for([Step(text="", stop="refusal")])
    events = _run(client)
    assert events[-1]["type"] == "error"
    assert events[-1]["retryable"] is False


def test_truncated_tool_input_is_not_executed(client_for: ClientFactory) -> None:
    client, provider = client_for(
        [
            Step(calls=[call("write_file", path="big.txt", content="partial")], stop="max_tokens"),
            Step(text="Recovered answer."),
        ]
    )
    events = _run(client)
    assert events[-1]["output"] == "Recovered answer."
    assert provider.tool_results[0].is_error
    # The truncated write never happened; only the safety-net .docx of the document task exists.
    assert [e["path"] for e in events if e["type"] == "artifact"] == ["quarterly-report.docx"]


def test_tool_errors_are_reported_to_the_model(client_for: ClientFactory) -> None:
    client, provider = client_for(
        [
            Step(
                calls=[
                    call("write_file", path="../escape.txt", content="x"),
                    call("read_file", "c2", path="missing.txt"),
                    call("no_such_tool", "c3"),
                ]
            ),
            Step(calls=[call("finish", answer="ok")]),
        ]
    )
    events = _run(client)
    assert events[-1]["type"] == "result"
    # all results of one turn are returned together, each flagged as an error
    assert [r.is_error for r in provider.tool_results[:3]] == [True, True, True]
    assert "'..'" in provider.tool_results[0].content


def test_missing_api_key_for_anthropic_is_reported(make_settings: Callable[..., Any]) -> None:
    with TestClient(create_app(make_settings())) as client:
        body = execute_body(llm={"provider": "anthropic", "api_key": None})
        response = client.post("/v1/execute", json=body, headers=AUTH)
    events = parse_ndjson(response.content)
    assert events[-1]["type"] == "error"
    assert events[-1]["retryable"] is False
    assert "API key" in events[-1]["message"]
