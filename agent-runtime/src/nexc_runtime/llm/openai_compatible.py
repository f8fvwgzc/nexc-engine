"""OpenAI-compatible Chat Completions provider (Ollama, vLLM, LM Studio, llama.cpp server...).

Uses plain httpx with SSE streaming and assembles streamed tool-call fragments.
"""

from __future__ import annotations

import json
import logging
from typing import Any

import httpx

from .base import (
    Conversation,
    LLMError,
    StopKind,
    TextSink,
    ToolCall,
    ToolResult,
    ToolSpec,
    Turn,
    redact,
)

log = logging.getLogger(__name__)

DEFAULT_BASE_URL = "http://localhost:11434/v1"  # Ollama


class OpenAICompatibleProvider:
    name = "openai_compatible"

    def __init__(
        self,
        *,
        model: str,
        base_url: str | None = None,
        api_key: str | None = None,
        timeout_s: float = 600.0,
        transport: httpx.AsyncBaseTransport | None = None,
    ) -> None:
        self.model = model
        self._secret = api_key
        headers = {"Accept": "text/event-stream"}
        if api_key:
            headers["Authorization"] = f"Bearer {api_key}"
        self._client = httpx.AsyncClient(
            base_url=(base_url or DEFAULT_BASE_URL).rstrip("/"),
            headers=headers,
            timeout=httpx.Timeout(timeout_s, connect=10.0),
            transport=transport,
            follow_redirects=False,
        )

    def new_conversation(self, system: str, prompt: str) -> Conversation:
        return Conversation(
            system=system,
            messages=[{"role": "system", "content": system}, {"role": "user", "content": prompt}],
        )

    def add_tool_results(self, conversation: Conversation, results: list[ToolResult]) -> None:
        for r in results:
            content = f"ERROR: {r.content}" if r.is_error else r.content
            conversation.messages.append(
                {"role": "tool", "tool_call_id": r.tool_use_id, "content": content}
            )

    def add_user_text(self, conversation: Conversation, text: str) -> None:
        conversation.messages.append({"role": "user", "content": text})

    async def aclose(self) -> None:
        await self._client.aclose()

    async def next_turn(
        self,
        conversation: Conversation,
        tools: list[ToolSpec],
        *,
        max_tokens: int,
        on_text: TextSink | None = None,
    ) -> Turn:
        body: dict[str, Any] = {
            "model": self.model,
            "messages": conversation.messages,
            "max_tokens": max_tokens,
            "stream": True,
            "stream_options": {"include_usage": True},
        }
        if tools:
            body["tools"] = [
                {
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    },
                }
                for t in tools
            ]
        try:
            async with self._client.stream("POST", "/chat/completions", json=body) as response:
                if response.status_code >= 400:
                    detail = (await response.aread()).decode("utf-8", "replace")[:500]
                    retryable = response.status_code == 429 or response.status_code >= 500
                    raise LLMError(
                        self._safe(f"LLM endpoint returned {response.status_code}: {detail}"),
                        retryable=retryable,
                    )
                state = _StreamState()
                async for line in response.aiter_lines():
                    state.feed(line, on_text)
        except httpx.TimeoutException:
            raise LLMError("LLM endpoint timed out", retryable=True) from None
        except httpx.TransportError as exc:
            raise LLMError(
                self._safe(f"cannot reach LLM endpoint: {exc}"), retryable=True
            ) from None
        return self._finish(conversation, state)

    def _finish(self, conversation: Conversation, state: _StreamState) -> Turn:
        calls: list[ToolCall] = []
        wire_calls: list[dict[str, Any]] = []
        for index in sorted(state.calls):
            raw = state.calls[index]
            call_id = raw.get("id") or f"call_{index}"
            arguments = raw.get("arguments", "")
            try:
                parsed = json.loads(arguments) if arguments else {}
            except json.JSONDecodeError:
                parsed = {"__invalid_json__": arguments}
            calls.append(
                ToolCall(
                    id=call_id,
                    name=raw.get("name", ""),
                    input=parsed if isinstance(parsed, dict) else {},
                )
            )
            wire_calls.append(
                {
                    "id": call_id,
                    "type": "function",
                    "function": {"name": raw.get("name", ""), "arguments": arguments or "{}"},
                }
            )
        assistant: dict[str, Any] = {"role": "assistant", "content": state.text or None}
        if wire_calls:
            assistant["tool_calls"] = wire_calls
        conversation.messages.append(assistant)

        stop: StopKind
        if calls:
            stop = "max_tokens" if state.finish_reason == "length" else "tool_use"
        elif state.finish_reason == "length":
            stop = "max_tokens"
        elif state.finish_reason == "content_filter":
            stop = "refusal"
        else:
            stop = "end_turn"
        return Turn(
            text=state.text,
            tool_calls=calls,
            stop=stop,
            input_tokens=state.prompt_tokens,
            output_tokens=state.completion_tokens,
        )

    def _safe(self, text: str) -> str:
        return redact(text, self._secret)


class _StreamState:
    """Accumulates one streamed chat completion."""

    def __init__(self) -> None:
        self.text = ""
        self.calls: dict[int, dict[str, str]] = {}
        self.finish_reason: str | None = None
        self.prompt_tokens = 0
        self.completion_tokens = 0

    def feed(self, line: str, on_text: TextSink | None) -> None:
        if not line.startswith("data:"):
            return
        payload = line[5:].strip()
        if not payload or payload == "[DONE]":
            return
        try:
            chunk = json.loads(payload)
        except json.JSONDecodeError:
            log.debug("ignoring malformed SSE chunk")
            return
        usage = chunk.get("usage") or {}
        self.prompt_tokens = int(usage.get("prompt_tokens") or self.prompt_tokens)
        self.completion_tokens = int(usage.get("completion_tokens") or self.completion_tokens)
        for choice in chunk.get("choices") or []:
            delta = choice.get("delta") or {}
            if text := delta.get("content"):
                self.text += text
                if on_text is not None:
                    on_text(text)
            for fragment in delta.get("tool_calls") or []:
                slot = self.calls.setdefault(int(fragment.get("index", 0)), {})
                if fragment.get("id"):
                    slot["id"] = fragment["id"]
                function = fragment.get("function") or {}
                if function.get("name"):
                    slot["name"] = function["name"]
                if function.get("arguments"):
                    slot["arguments"] = slot.get("arguments", "") + function["arguments"]
            if choice.get("finish_reason"):
                self.finish_reason = choice["finish_reason"]
