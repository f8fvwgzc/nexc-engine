"""Provider-neutral types for the agent tool loop.

A provider owns the wire format of its conversation. The agent only ever:
  1. starts a conversation (`new_conversation`),
  2. asks for the next assistant turn (`next_turn`), and
  3. answers all tool calls of that turn in one go (`add_tool_results`).
"""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass, field
from typing import Any, Literal, Protocol

StopKind = Literal["end_turn", "tool_use", "max_tokens", "refusal", "pause_turn", "other"]
TextSink = Callable[[str], None]


@dataclass(frozen=True, slots=True)
class ToolSpec:
    """A tool as advertised to the model."""

    name: str
    description: str
    input_schema: dict[str, Any]


@dataclass(frozen=True, slots=True)
class ToolCall:
    id: str
    name: str
    input: dict[str, Any]


@dataclass(frozen=True, slots=True)
class ToolResult:
    tool_use_id: str
    content: str
    is_error: bool = False


@dataclass(slots=True)
class Turn:
    """One assistant turn, already appended to the conversation by the provider."""

    text: str
    tool_calls: list[ToolCall]
    stop: StopKind
    input_tokens: int
    output_tokens: int
    detail: str | None = None  # e.g. the refusal category


@dataclass(slots=True)
class Conversation:
    system: str
    messages: list[dict[str, Any]] = field(default_factory=list)


class LLMError(Exception):
    """A provider failure, safe to show to users (secrets already stripped)."""

    def __init__(self, message: str, *, retryable: bool) -> None:
        super().__init__(message)
        self.message = message
        self.retryable = retryable


class LLMProvider(Protocol):
    name: str
    model: str

    def new_conversation(self, system: str, prompt: str) -> Conversation: ...

    async def next_turn(
        self,
        conversation: Conversation,
        tools: list[ToolSpec],
        *,
        max_tokens: int,
        on_text: TextSink | None = None,
    ) -> Turn: ...

    def add_tool_results(self, conversation: Conversation, results: list[ToolResult]) -> None: ...

    def add_user_text(self, conversation: Conversation, text: str) -> None: ...

    async def aclose(self) -> None: ...


def redact(text: str, *secrets: str | None) -> str:
    """Remove secrets (e.g. the per-request API key) from a message before it leaves the process."""
    for secret in secrets:
        if secret and len(secret) >= 4:
            text = text.replace(secret, "[redacted]")
    return text
