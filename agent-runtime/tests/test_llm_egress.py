"""A model endpoint the runtime's egress proxy does not let through fails once, saying which
setting to change, instead of being retried as a network hiccup."""

from __future__ import annotations

import httpx
import httpx2
import pytest

from nexc_runtime.llm import LLMError
from nexc_runtime.llm.anthropic_provider import AnthropicProvider
from nexc_runtime.llm.base import egress_refused
from nexc_runtime.llm.openai_compatible import OpenAICompatibleProvider


def test_a_refusal_is_recognised_through_the_sdk_wrapping() -> None:
    try:
        try:
            raise httpx.ProxyError("403 Forbidden")
        except httpx.ProxyError as inner:
            raise RuntimeError("Connection error.") from inner
    except RuntimeError as outer:
        refused = egress_refused(outer, "https://llm.example.com")
    assert refused is not None and refused.retryable is False
    assert "https://llm.example.com" in refused.message
    assert "NEXC_RUNTIME_EGRESS_ALLOW" in refused.message
    assert egress_refused(httpx.ProxyError("502 Bad Gateway"), "x") is None
    assert egress_refused(httpx.ConnectError("refused"), "x") is None


async def test_the_openai_compatible_provider_reports_it_as_final() -> None:
    def refuse(_request: httpx.Request) -> httpx.Response:
        raise httpx.ProxyError("403 Forbidden")

    provider = OpenAICompatibleProvider(
        model="m", base_url="https://llm.example.com/v1", transport=httpx.MockTransport(refuse)
    )
    conversation = provider.new_conversation("system", "hi")
    with pytest.raises(LLMError) as error:
        await provider.next_turn(conversation, [], max_tokens=16)
    assert error.value.retryable is False
    assert "egress proxy refused https://llm.example.com/v1" in error.value.message
    await provider.aclose()


async def test_the_anthropic_provider_reports_it_as_final() -> None:
    def refuse(_request: httpx2.Request) -> httpx2.Response:
        raise httpx2.ProxyError("403 Forbidden")

    client = httpx2.AsyncClient(transport=httpx2.MockTransport(refuse))
    provider = AnthropicProvider(api_key="sk-test", max_retries=0, http_client=client)
    conversation = provider.new_conversation("system", "hi")
    with pytest.raises(LLMError) as error:
        await provider.next_turn(conversation, [], max_tokens=16)
    assert error.value.retryable is False
    assert "egress proxy refused" in error.value.message
    await provider.aclose()
