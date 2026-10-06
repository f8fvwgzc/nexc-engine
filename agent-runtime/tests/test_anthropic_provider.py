"""Anthropic provider against a mocked transport (never the real API)."""

from __future__ import annotations

import json
from typing import Any

import httpx2
import pytest

from nexc_runtime.llm import AnthropicProvider, LLMError, ToolResult, ToolSpec
from nexc_runtime.llm.anthropic_provider import _echo_blocks, supports_adaptive_thinking

KEY = "sk-ant-test-key-do-not-leak"


def _sse(events: list[tuple[str, dict[str, Any]]]) -> bytes:
    return "".join(f"event: {name}\ndata: {json.dumps(data)}\n\n" for name, data in events).encode()


def _message_stream(content_events: list[tuple[str, dict[str, Any]]], stop: str) -> bytes:
    start = {
        "type": "message_start",
        "message": {
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-5",
            "content": [],
            "stop_reason": None,
            "stop_sequence": None,
            "usage": {"input_tokens": 50, "output_tokens": 1},
        },
    }
    delta = {
        "type": "message_delta",
        "delta": {"stop_reason": stop, "stop_sequence": None},
        "usage": {"output_tokens": 30},
    }
    return _sse(
        [
            ("message_start", start),
            *content_events,
            ("message_delta", delta),
            ("message_stop", {"type": "message_stop"}),
        ]
    )


def _provider(handler: Any) -> AnthropicProvider:
    client = httpx2.AsyncClient(transport=httpx2.MockTransport(handler))
    return AnthropicProvider(api_key=KEY, model="claude-opus-5", max_retries=0, http_client=client)


TOOLS = [
    ToolSpec(
        "finish",
        "done",
        {
            "type": "object",
            "properties": {"answer": {"type": "string"}},
            "required": ["answer"],
            "additionalProperties": False,
        },
    )
]


async def test_stream_turn_with_tool_use_and_request_shape() -> None:
    seen: dict[str, Any] = {}

    def handler(request: httpx2.Request) -> httpx2.Response:
        seen["body"] = json.loads(request.content)
        seen["beta"] = request.headers.get("anthropic-beta")
        body = _message_stream(
            [
                (
                    "content_block_start",
                    {
                        "type": "content_block_start",
                        "index": 0,
                        "content_block": {"type": "text", "text": ""},
                    },
                ),
                (
                    "content_block_delta",
                    {
                        "type": "content_block_delta",
                        "index": 0,
                        "delta": {"type": "text_delta", "text": "Hello"},
                    },
                ),
                ("content_block_stop", {"type": "content_block_stop", "index": 0}),
                (
                    "content_block_start",
                    {
                        "type": "content_block_start",
                        "index": 1,
                        "content_block": {
                            "type": "tool_use",
                            "id": "toolu_1",
                            "name": "finish",
                            "input": {},
                        },
                    },
                ),
                (
                    "content_block_delta",
                    {
                        "type": "content_block_delta",
                        "index": 1,
                        "delta": {"type": "input_json_delta", "partial_json": '{"answer": "ok"}'},
                    },
                ),
                ("content_block_stop", {"type": "content_block_stop", "index": 1}),
            ],
            "tool_use",
        )
        return httpx2.Response(200, content=body, headers={"content-type": "text/event-stream"})

    provider = _provider(handler)
    conversation = provider.new_conversation("system prompt", "do it")
    deltas: list[str] = []
    turn = await provider.next_turn(conversation, TOOLS, max_tokens=4096, on_text=deltas.append)
    await provider.aclose()

    assert deltas == ["Hello"]
    assert turn.stop == "tool_use"
    assert turn.tool_calls[0].name == "finish" and turn.tool_calls[0].input == {"answer": "ok"}
    assert (turn.input_tokens, turn.output_tokens) == (50, 30)

    body = seen["body"]
    assert body["thinking"] == {"type": "adaptive"}
    assert body["fallbacks"] == "default"
    assert "server-side-fallback-2026-07-01" in seen["beta"]
    assert body["tools"][0]["strict"] is True
    assert body["tools"][0]["eager_input_streaming"] is True
    assert body["cache_control"] == {"type": "ephemeral"}, "the tool loop caches its history"
    for forbidden in ("temperature", "top_p", "budget_tokens"):
        assert forbidden not in body
    assert conversation.messages[-1]["role"] == "assistant"

    provider.add_tool_results(
        conversation, [ToolResult("toolu_1", "a"), ToolResult("x", "b", True)]
    )
    last = conversation.messages[-1]
    assert last["role"] == "user" and len(last["content"]) == 2  # one message for all results
    assert last["content"][1]["is_error"] is True


async def test_refusal_stop_reason_surfaces_category() -> None:
    def handler(request: httpx2.Request) -> httpx2.Response:
        body = _message_stream([], "refusal")
        return httpx2.Response(200, content=body, headers={"content-type": "text/event-stream"})

    provider = _provider(handler)
    turn = await provider.next_turn(provider.new_conversation("s", "p"), [], max_tokens=1024)
    assert turn.stop == "refusal"
    assert turn.tool_calls == []


@pytest.mark.parametrize(
    ("status", "retryable"), [(429, True), (500, True), (529, True), (400, False), (401, False)]
)
async def test_status_errors_are_classified_and_redacted(status: int, retryable: bool) -> None:
    def handler(request: httpx2.Request) -> httpx2.Response:
        error = {"type": "error", "error": {"type": "x", "message": f"bad key {KEY}"}}
        return httpx2.Response(status, json=error)

    provider = _provider(handler)
    with pytest.raises(LLMError) as info:
        await provider.next_turn(provider.new_conversation("s", "p"), [], max_tokens=1024)
    assert info.value.retryable is retryable
    assert KEY not in info.value.message


async def test_connection_errors_are_retryable() -> None:
    def handler(request: httpx2.Request) -> httpx2.Response:
        raise httpx2.ConnectError("boom", request=request)

    provider = _provider(handler)
    with pytest.raises(LLMError) as info:
        await provider.next_turn(provider.new_conversation("s", "p"), [], max_tokens=1024)
    assert info.value.retryable is True


def test_missing_key_is_not_retryable() -> None:
    with pytest.raises(LLMError) as info:
        AnthropicProvider(api_key="")
    assert info.value.retryable is False


def test_fallback_echo_drops_pre_boundary_internal_blocks() -> None:
    blocks: list[dict[str, Any]] = [
        {"type": "thinking", "thinking": "", "signature": "s"},
        {"type": "text", "text": "partial "},
        {"type": "tool_use", "id": "t0", "name": "x", "input": {}},
        {"type": "fallback", "from": {"model": "a"}, "to": {"model": "b"}},
        {"type": "thinking", "thinking": "", "signature": "s2"},
        {"type": "text", "text": "rest"},
        {"type": "tool_use", "id": "t1", "name": "y", "input": {}},
    ]
    kinds = [
        (b["type"], b.get("id") or b.get("signature") or b.get("text"))
        for b in _echo_blocks(blocks)
    ]
    assert kinds == [("text", "partial "), ("thinking", "s2"), ("text", "rest"), ("tool_use", "t1")]


def test_adaptive_thinking_model_gate() -> None:
    assert supports_adaptive_thinking("claude-opus-5")
    assert supports_adaptive_thinking("claude-sonnet-5")
    assert not supports_adaptive_thinking("claude-haiku-4-5")
