"""AgentSpec: the blueprint an agent is born from at request time."""

from __future__ import annotations

from pydantic import BaseModel, ConfigDict, Field


class AgentSpec(BaseModel):
    model_config = ConfigDict(extra="ignore")

    name: str = Field(default="agent", min_length=1, max_length=120)
    role: str = Field(default="agent", min_length=1, max_length=120)
    system_prompt: str = Field(default="", max_length=32_000)
    model: str | None = Field(default=None, max_length=200)
    budget_tokens: int = Field(default=200_000, ge=1_000, le=50_000_000)
