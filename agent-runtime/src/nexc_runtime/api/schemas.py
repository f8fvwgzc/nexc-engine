"""Request/response models of the internal runtime API (CONTRACT section 8)."""

from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, ConfigDict, Field, SecretStr

from ..agents.spec import AgentSpec

# A node kind is the key of a node type in the graph's ontology: any slug, not a fixed list.
NODE_KIND_PATTERN = r"^[a-z][a-z0-9_]{0,39}$"
ProviderName = Literal["anthropic", "openai_compatible", "demo", "claude_code"]


class _Model(BaseModel):
    model_config = ConfigDict(extra="ignore")


class TaskIn(_Model):
    title: str = Field(min_length=1, max_length=200)
    content: str = Field(default="", max_length=64 * 1024)
    kind: str = Field(default="task", pattern=NODE_KIND_PATTERN)
    # What the ontology says about the node's type.
    kind_description: str = Field(default="", max_length=500)
    # None: the backend did not say (older backends); fall back to the well-known kinds.
    produces_artifact: bool | None = None


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


class ParseBlock(BaseModel):
    """One block of a parsed document, in reading order (`POST /v1/parse`)."""

    kind: Literal["heading", "text", "table"]
    level: int | None = Field(default=None, ge=1, le=6)  # headings only
    text: str = ""  # empty for tables
    page: int | None = Field(default=None, ge=1)  # page / slide / sheet, 1-based
    rows: list[list[str]] | None = None  # tables only: a rectangular matrix
    header_rows: int | None = Field(default=None, ge=0)  # tables only: leading header rows


class ParseResponse(BaseModel):
    pages: int | None = None  # pages / slides / sheets when the format has them
    blocks: list[ParseBlock]


class Health(BaseModel):
    status: Literal["ok"] = "ok"
    version: str
    agents_loaded: int
