"""LLM provider layer: Anthropic (default), OpenAI-compatible (local models), the local Claude Code
CLI (`claude_code`, uses its own login) and the offline demo."""

from __future__ import annotations

from .anthropic_provider import AnthropicProvider
from .base import (
    Conversation,
    LLMError,
    LLMProvider,
    ToolCall,
    ToolResult,
    ToolSpec,
    Turn,
    redact,
)
from .claude_code import ClaudeCodeProvider
from .demo import DemoBrief, DemoProvider
from .openai_compatible import OpenAICompatibleProvider


def build_provider(
    *,
    provider: str,
    model: str,
    api_key: str | None,
    base_url: str | None,
    brief: DemoBrief,
    max_retries: int,
    timeout_s: float,
    claude_bin: str = "claude",
) -> LLMProvider:
    """Instantiate the provider named in the request."""
    if provider == "demo":
        return DemoProvider(brief)
    if provider == "claude_code":
        return ClaudeCodeProvider(model=model, binary=claude_bin, timeout_s=timeout_s)
    if provider == "openai_compatible":
        return OpenAICompatibleProvider(
            model=model, base_url=base_url, api_key=api_key, timeout_s=timeout_s
        )
    if provider == "anthropic":
        return AnthropicProvider(
            api_key=api_key or "",
            model=model,
            base_url=base_url,
            max_retries=max_retries,
            timeout_s=timeout_s,
        )
    raise LLMError(f"unknown LLM provider {provider!r}", retryable=False)


__all__ = [
    "AnthropicProvider",
    "ClaudeCodeProvider",
    "Conversation",
    "DemoBrief",
    "DemoProvider",
    "LLMError",
    "LLMProvider",
    "OpenAICompatibleProvider",
    "ToolCall",
    "ToolResult",
    "ToolSpec",
    "Turn",
    "build_provider",
    "redact",
]
