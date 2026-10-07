"""Tool plumbing: definition, execution context, strict JSON schemas and validation."""

from __future__ import annotations

import copy
from collections.abc import Awaitable, Callable
from dataclasses import dataclass, field
from typing import Any

from pydantic import BaseModel, ConfigDict, ValidationError

from ..config import Settings
from ..llm.base import ToolSpec
from ..streaming import EventStream
from .workspace import PathError, Workspace

SpawnFn = Callable[[str, str], Awaitable[str]]


class ToolArgs(BaseModel):
    """Base for tool argument models: unknown fields are rejected."""

    model_config = ConfigDict(extra="forbid", str_strip_whitespace=False)


class ToolError(Exception):
    """A tool failed in a way the model should see (returned as an `is_error` tool result)."""


@dataclass(slots=True)
class ToolContext:
    workspace: Workspace
    events: EventStream
    settings: Settings
    agent_name: str
    spawn: SpawnFn | None = None
    final_answer: str | None = None
    notes: list[str] = field(default_factory=list)
    # The node's type says it delivers a file; `finish` then needs one in the workspace.
    produces_artifact: bool = False


@dataclass(frozen=True, slots=True)
class Tool[A: BaseModel]:
    name: str
    description: str
    args: type[A]
    handler: Callable[[ToolContext, A], Awaitable[str]]

    def spec(self) -> ToolSpec:
        return ToolSpec(self.name, self.description, strict_schema(self.args))

    async def invoke(self, ctx: ToolContext, raw: dict[str, Any]) -> str:
        """Validate `raw` against the argument model, then run the handler."""
        try:
            args = self.args.model_validate(raw)
        except ValidationError as exc:
            problems = "; ".join(
                f"{'.'.join(str(p) for p in err['loc']) or 'input'}: {err['msg']}"
                for err in exc.errors(include_input=False, include_url=False)
            )
            raise ToolError(f"invalid arguments for {self.name}: {problems}") from None
        try:
            return await self.handler(ctx, args)
        except PathError as exc:
            raise ToolError(str(exc)) from None


def strict_schema(model: type[BaseModel]) -> dict[str, Any]:
    """JSON schema for a pydantic model, flattened and tightened for strict tool use.

    `$ref`s are inlined, `title`/`default` noise is dropped and every object gets
    `additionalProperties: false`.
    """
    schema = model.model_json_schema()
    defs = schema.pop("$defs", {})

    def walk(node: Any) -> Any:
        if isinstance(node, dict):
            if "$ref" in node:
                name = node["$ref"].rsplit("/", 1)[-1]
                return walk(copy.deepcopy(defs[name]))
            out: dict[str, Any] = {}
            for key, value in node.items():
                if key == "properties":  # property names are data, not schema keywords
                    out[key] = {name: walk(sub) for name, sub in value.items()}
                elif key not in ("title", "default"):
                    out[key] = walk(value)
            if out.get("type") == "object":
                out["additionalProperties"] = False
                out.setdefault("properties", {})
            return out
        if isinstance(node, list):
            return [walk(item) for item in node]
        return node

    result: dict[str, Any] = walk(schema)
    return result
