"""Request/response models of the internal runtime API (CONTRACT section 8)."""

from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, ConfigDict, Field, SecretStr

from ..agents.spec import AgentSpec

NodeKind = Literal["topic", "task", "research", "code", "document", "output"]
ProviderName = Literal["anthropic", "openai_compatible", "demo", "claude_code"]


class _Model(BaseModel):
    model_config = ConfigDict(extra="ignore")


class TaskIn(_Model):
    title: str = Field(min_length=1, max_length=200)
    content: str = Field(default="", max_length=64 * 1024)
    kind: NodeKind = "task"


class UpstreamIn(_Model):
    node_id: str = Field(default="", max_length=64)
    title: str = Field(default="", max_length=200)
    output: str = Field(default="", max_length=512 * 1024)


class ContextIn(_Model):
    goal: str = Field(default="", max_length=16 * 1024)
    upstream: list[UpstreamIn] = Field(default_factory=list, max_length=100)
    memories: list[str] = Field(default_factory=list, max_length=100)


class LlmIn(_Model):
    provider: ProviderName = "anthropic"
    # SecretStr keeps the key out of reprs, logs and validation error payloads.
    api_key: SecretStr | None = None
    model: str | None = Field(default=None, max_length=200)
    base_url: str | None = Field(default=None, max_length=2048)


class LimitsIn(_Model):
    max_turns: int = Field(default=12, ge=1, le=200)
    timeout_s: int = Field(default=600, ge=1, le=24 * 3600)
    allow_code_exec: bool = False


class ExecuteRequest(_Model):
    run_id: str = Field(min_length=1, max_length=64)
    node_id: str = Field(min_length=1, max_length=64)
    agent: AgentSpec = Field(default_factory=AgentSpec)
    task: TaskIn
    context: ContextIn = Field(default_factory=ContextIn)
    llm: LlmIn = Field(default_factory=LlmIn)
    limits: LimitsIn = Field(default_factory=LimitsIn)


class Health(BaseModel):
    status: Literal["ok"] = "ok"
    version: str
    agents_loaded: int
