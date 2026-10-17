"""Tool plumbing: definition, execution context, strict JSON schemas and validation."""

from __future__ import annotations

import copy
import time
from collections.abc import Awaitable, Callable
from dataclasses import dataclass, field
from typing import Any

from pydantic import BaseModel, ConfigDict, ValidationError

from ..config import Settings
from ..llm.base import ToolSpec
from ..streaming import EventStream
from ..trace import args_digest, new_span_id, now_ns, rfc3339_ns
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
    # The `llm_call` span of the turn whose tool calls are running: their spans' parent.
    turn_span_id: str | None = None


@dataclass(frozen=True, slots=True)
class Tool[A: BaseModel]:
    name: str
    description: str
    args: type[A]
    handler: Callable[[ToolContext, A], Awaitable[str]]

    def spec(self) -> ToolSpec:
        return ToolSpec(self.name, self.description, strict_schema(self.args))

    async def invoke(self, ctx: ToolContext, raw: dict[str, Any]) -> str:
        """Validate `raw` against the argument model, then run the handler.

        Every call is reported as a `tool_call` span: the tool, a digest of the arguments (never
        the arguments), the size of the result, whether it failed, and how long it took.
        """
        span_id = new_span_id()
        started = now_ns()
        clock = time.perf_counter_ns()
        digest, size = args_digest(raw)
        output: str | None = None
        try:
            output = await self._run(ctx, raw)
            return output
        finally:
            ctx.events.span(
                span_id=span_id,
                parent_id=ctx.turn_span_id,
                kind="tool_call",
                name=self.name,
                agent=ctx.agent_name,
                started_at=rfc3339_ns(started),
                ended_at=rfc3339_ns(now_ns()),
                status="ok" if output is not None else "error",
                attributes={
                    "tool": self.name,
                    "args_sha256": digest,
                    "args_bytes": size,
                    "result_bytes": len(output.encode("utf-8")) if output is not None else 0,
                    "is_error": output is None,
                    "duration_ns": time.perf_counter_ns() - clock,
                },
            )

    async def _run(self, ctx: ToolContext, raw: dict[str, Any]) -> str:
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
