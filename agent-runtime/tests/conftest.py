"""Shared fixtures: settings, a scripted fake LLM provider and an app wired to it."""

from __future__ import annotations

import json
from collections.abc import Callable, Iterator
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import pytest
from fastapi.testclient import TestClient

from nexc_runtime.api.app import create_app
from nexc_runtime.api.schemas import ExecuteRequest
from nexc_runtime.config import Settings
from nexc_runtime.llm import Conversation, LLMError, ToolCall, ToolResult, ToolSpec, Turn
from nexc_runtime.llm.base import StopKind, TextSink

TOKEN = "t" * 40
AUTH = {"Authorization": f"Bearer {TOKEN}"}


@dataclass
class Step:
    """One scripted assistant turn."""

    text: str = ""
    calls: list[ToolCall] = field(default_factory=list)
    stop: StopKind = "end_turn"
    tokens_in: int = 100
    tokens_out: int = 20
    raise_error: LLMError | None = None


class FakeProvider:
    """Plays back `Step`s in order (shared across agents of one run). No network."""

    name = "fake"
    model = "fake-model"

    def __init__(self, steps: list[Step]) -> None:
        self.steps = list(steps)
        self.tool_results: list[ToolResult] = []
        self.seen_tools: list[list[str]] = []
        self.closed = False

    def new_conversation(self, system: str, prompt: str) -> Conversation:
        return Conversation(system=system, messages=[{"role": "user", "content": prompt}])

    async def next_turn(
        self,
        conversation: Conversation,
        tools: list[ToolSpec],
        *,
        max_tokens: int,
        on_text: TextSink | None = None,
    ) -> Turn:
        self.seen_tools.append([t.name for t in tools])
        if not self.steps:
            return Turn("done", [], "end_turn", 1, 1)
        step = self.steps.pop(0)
        if step.raise_error is not None:
            raise step.raise_error
        if on_text and step.text:
            on_text(step.text)
        conversation.messages.append({"role": "assistant", "content": step.text})
        stop: StopKind = "tool_use" if step.calls and step.stop == "end_turn" else step.stop
        return Turn(step.text, step.calls, stop, step.tokens_in, step.tokens_out)

    def add_tool_results(self, conversation: Conversation, results: list[ToolResult]) -> None:
        self.tool_results.extend(results)
        conversation.messages.append({"role": "user", "content": [r.content for r in results]})

    def add_user_text(self, conversation: Conversation, text: str) -> None:
        conversation.messages.append({"role": "user", "content": text})

    async def aclose(self) -> None:
        self.closed = True


def call(name: str, call_id: str | None = None, **arguments: Any) -> ToolCall:
    return ToolCall(id=call_id or f"call_{name}", name=name, input=arguments)


@pytest.fixture
def workspace_root(tmp_path: Path) -> Path:
    return tmp_path / "workspaces"


@pytest.fixture
def make_settings(workspace_root: Path) -> Callable[..., Settings]:
    def factory(**overrides: Any) -> Settings:
        values: dict[str, Any] = {
            "nexc_runtime_token": TOKEN,
            "runtime_workspace": workspace_root,
        }
        values.update(overrides)
        return Settings(**values)

    return factory


@pytest.fixture
def settings(make_settings: Callable[..., Settings]) -> Settings:
    return make_settings()


@pytest.fixture
def client_for(
    make_settings: Callable[..., Settings],
) -> Iterator[Callable[..., tuple[TestClient, FakeProvider]]]:
    clients: list[TestClient] = []

    def factory(steps: list[Step], **overrides: Any) -> tuple[TestClient, FakeProvider]:
        provider = FakeProvider(steps)

        def provider_factory(_: ExecuteRequest, __: Settings) -> FakeProvider:
            return provider

        app = create_app(make_settings(**overrides), provider_factory=provider_factory)
        client = TestClient(app)
        clients.append(client)
        return client, provider

    yield factory
    for client in clients:
        client.close()


def execute_body(**overrides: Any) -> dict[str, Any]:
    body: dict[str, Any] = {
        "run_id": "run-1",
        "node_id": "node-1",
        "agent": {"name": "writer", "role": "writer", "budget_tokens": 200_000},
        "task": {"title": "Quarterly report", "content": "Summarise Q3.", "kind": "document"},
        "context": {"goal": "Inform the board", "upstream": [], "memories": []},
        "llm": {"provider": "anthropic", "api_key": "sk-ant-secret-value", "model": None},
        "limits": {"max_turns": 8, "timeout_s": 60, "allow_code_exec": False},
    }
    for key, value in overrides.items():
        if isinstance(value, dict) and isinstance(body.get(key), dict):
            body[key] = {**body[key], **value}
        else:
            body[key] = value
    return body


def parse_ndjson(raw: bytes) -> list[dict[str, Any]]:
    assert raw.endswith(b"\n"), "every NDJSON line must be newline-terminated"
    return [json.loads(line) for line in raw.decode("utf-8").splitlines() if line]
