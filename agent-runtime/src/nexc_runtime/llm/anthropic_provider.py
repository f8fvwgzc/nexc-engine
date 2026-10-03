"""Claude via the official Anthropic Python SDK (1.x).

* Streams every turn with `client.beta.messages.stream(...)` + `get_final_message()`.
* Adaptive thinking on models that support it; never sends temperature/top_p/budget_tokens.
* Server-side refusal fallback (`fallbacks="default"`) is enabled for models that offer it.
* `stop_reason` is checked before any content is used; tools are never run on a refused or
  truncated turn.
"""

from __future__ import annotations

import logging
from typing import Any, cast

import anthropic
import httpx2
from anthropic.types.beta import BetaMessage, BetaMessageParam, BetaToolParam

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

DEFAULT_MODEL = "claude-opus-5"
FALLBACK_BETA = "server-side-fallback-2026-07-01"
# Models with a server-defined default fallback chain for policy declines.
FALLBACK_MODELS = frozenset({"claude-opus-5", "claude-fable-5-1"})
# Models that predate adaptive thinking (they would reject `{"type": "adaptive"}`).
_LEGACY_THINKING_PREFIXES = (
    "claude-3",
    "claude-haiku-4-5",
    "claude-sonnet-4-5",
    "claude-sonnet-4-0",
    "claude-sonnet-4-1",
    "claude-opus-4-5",
    "claude-opus-4-1",
    "claude-opus-4-0",
)
# Block types that must not be echoed back when they precede a mid-output fallback boundary.
_PRE_FALLBACK_DROP = frozenset(
    {"thinking", "redacted_thinking", "tool_use", "server_tool_use", "mcp_tool_use"}
)
_CLIENT_ONLY_KEYS = ("parsed_output",)
_MAX_JSON_RETRIES = 2


def supports_adaptive_thinking(model: str) -> bool:
    return not model.startswith(_LEGACY_THINKING_PREFIXES)


class AnthropicProvider:
    name = "anthropic"

    def __init__(
        self,
        *,
        api_key: str,
        model: str | None = None,
        base_url: str | None = None,
        max_retries: int = 2,
        timeout_s: float = 600.0,
        http_client: httpx2.AsyncClient | None = None,
    ) -> None:
        if not api_key:
            raise LLMError(
                "no Anthropic API key: set one in Settings -> LLM or ANTHROPIC_API_KEY on the "
                "backend, or switch the provider to 'demo'",
                retryable=False,
            )
        self.model = model or DEFAULT_MODEL
        self._secret = api_key
        # The key is passed explicitly so the SDK never falls back to ambient credentials.
        self._client = anthropic.AsyncAnthropic(
            api_key=api_key,
            base_url=base_url or None,
            max_retries=max_retries,
            timeout=timeout_s,
            http_client=http_client,
        )

    # --- conversation -----------------------------------------------------------------------
    def new_conversation(self, system: str, prompt: str) -> Conversation:
        return Conversation(system=system, messages=[{"role": "user", "content": prompt}])

    def add_tool_results(self, conversation: Conversation, results: list[ToolResult]) -> None:
        # All results of one assistant turn go back in a single user message.
        conversation.messages.append(
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": r.tool_use_id,
                        "content": r.content,
                        "is_error": r.is_error,
                    }
                    for r in results
                ],
            }
        )

    def add_user_text(self, conversation: Conversation, text: str) -> None:
        conversation.messages.append({"role": "user", "content": text})

    async def aclose(self) -> None:
        await self._client.close()

    # --- one turn ---------------------------------------------------------------------------
    async def next_turn(
        self,
        conversation: Conversation,
        tools: list[ToolSpec],
        *,
        max_tokens: int,
        on_text: TextSink | None = None,
    ) -> Turn:
        for attempt in range(_MAX_JSON_RETRIES + 1):
            try:
                message = await self._stream(conversation, tools, max_tokens, on_text)
                break
            except ValueError as exc:
                # The SDK raises ValueError for tool-input JSON it cannot parse at all
                # (eager input streaming). There is no tool_use id to answer: re-issue.
                log.warning("unparseable tool input from model (attempt %d): %s", attempt + 1, exc)
                if attempt == _MAX_JSON_RETRIES:
                    raise LLMError(
                        "model produced unparseable tool input repeatedly", retryable=True
                    ) from None
            except anthropic.RateLimitError as exc:
                raise LLMError(
                    self._safe(f"rate limited by Anthropic: {exc}"), retryable=True
                ) from None
            except anthropic.APIStatusError as exc:
                retryable = exc.status_code >= 500
                raise LLMError(
                    self._safe(f"Anthropic API error {exc.status_code}: {exc.message}"),
                    retryable=retryable,
                ) from None
            except anthropic.APIConnectionError as exc:
                raise LLMError(
                    self._safe(f"cannot reach Anthropic: {exc}"), retryable=True
                ) from None
        return self._to_turn(conversation, message)

    async def _stream(
        self,
        conversation: Conversation,
        tools: list[ToolSpec],
        max_tokens: int,
        on_text: TextSink | None,
    ) -> BetaMessage:
        kwargs: dict[str, Any] = {
            "model": self.model,
            "max_tokens": max_tokens,
            "system": conversation.system,
            "messages": cast(list[BetaMessageParam], conversation.messages),
        }
        if tools:
            kwargs["tools"] = [_tool_param(t) for t in tools]
        if supports_adaptive_thinking(self.model):
            kwargs["thinking"] = {"type": "adaptive"}
        if self.model in FALLBACK_MODELS:
            kwargs["betas"] = [FALLBACK_BETA]
            kwargs["fallbacks"] = "default"

        async with self._client.beta.messages.stream(**kwargs) as stream:
            async for event in stream:
                if event.type == "text" and on_text is not None:
                    on_text(event.text)
            return await stream.get_final_message()

    def _to_turn(self, conversation: Conversation, message: BetaMessage) -> Turn:
        blocks = [_block_to_param(b) for b in message.content]
        echo = _echo_blocks(blocks)
        if echo:
            conversation.messages.append({"role": "assistant", "content": echo})

        text = "".join(b.get("text", "") for b in echo if b.get("type") == "text")
        calls = [
            ToolCall(id=b["id"], name=b["name"], input=_as_dict(b.get("input")))
            for b in echo
            if b.get("type") == "tool_use"
        ]
        usage = message.usage
        input_tokens = (
            usage.input_tokens
            + (usage.cache_creation_input_tokens or 0)
            + (usage.cache_read_input_tokens or 0)
        )
        stop, detail = _stop_kind(message)
        return Turn(
            text=text,
            tool_calls=calls,
            stop=stop,
            input_tokens=input_tokens,
            output_tokens=usage.output_tokens,
            detail=detail,
        )

    def _safe(self, text: str) -> str:
        return redact(text, self._secret)


# --- helpers ----------------------------------------------------------------------------------


def _tool_param(tool: ToolSpec) -> BetaToolParam:
    return {
        "name": tool.name,
        "description": tool.description,
        "input_schema": cast(Any, tool.input_schema),
        "strict": True,
        # Large inputs (file bodies, documents) stream as generated; we validate them ourselves.
        "eager_input_streaming": True,
    }


def _block_to_param(block: Any) -> dict[str, Any]:
    data = cast(dict[str, Any], block.to_dict())
    for key in _CLIENT_ONLY_KEYS:
        data.pop(key, None)
    return data


def _echo_blocks(blocks: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Blocks to send back as the assistant turn.

    After a mid-output fallback, model-internal blocks that precede the last `fallback` marker
    must be omitted; the marker itself is an audit-only block and is dropped.
    """
    boundary = max((i for i, b in enumerate(blocks) if b.get("type") == "fallback"), default=-1)
    echo: list[dict[str, Any]] = []
    for index, block in enumerate(blocks):
        kind = block.get("type")
        if kind == "fallback":
            continue
        if index < boundary and kind in _PRE_FALLBACK_DROP:
            continue
        echo.append(block)
    return echo


def _as_dict(value: Any) -> dict[str, Any]:
    return value if isinstance(value, dict) else {}


def _stop_kind(message: BetaMessage) -> tuple[StopKind, str | None]:
    reason = message.stop_reason
    if reason == "refusal":
        category = message.stop_details.category if message.stop_details else None
        return "refusal", category
    if reason in ("end_turn", "stop_sequence"):
        return "end_turn", None
    if reason in ("tool_use", "max_tokens", "pause_turn"):
        return reason, None
    if reason == "model_context_window_exceeded":
        return "max_tokens", "context window exceeded"
    return "other", reason
