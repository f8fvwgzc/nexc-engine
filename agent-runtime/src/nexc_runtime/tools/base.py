"""Tool plumbing: definition, execution context, strict JSON schemas and validation."""

from __future__ import annotations

import copy
import time
from collections.abc import Awaitable, Callable
from dataclasses import dataclass, field
from typing import Any, NoReturn

from pydantic import BaseModel, ConfigDict, ValidationError

from ..config import Settings
from ..llm.base import ToolSpec
from ..policy import AgentToken, Decision, decide_tool
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
    # The per-run token of the acting agent: every call is decided against its capabilities.
    # None only in unit tests of single tools; a run always has one.
    token: AgentToken | None = None
    # The principal the agent's spans are attributed to (defaults to the name).
    principal: str = ""

    def __post_init__(self) -> None:
        if not self.principal:
            self.principal = self.agent_name


@dataclass(frozen=True, slots=True)
class Tool[A: BaseModel]:
    name: str
    description: str
    args: type[A]
    handler: Callable[[ToolContext, A], Awaitable[str]]

    def spec(self) -> ToolSpec:
        return ToolSpec(self.name, self.description, strict_schema(self.args))

    async def invoke(self, ctx: ToolContext, raw: dict[str, Any]) -> str:
        """Decide the call against the agent's token, validate `raw` against the argument model,
        then run the handler.

        A call the policy refuses never runs: it is reported as a `policy` span (the rule and
        the reason) and the model sees the refusal as a tool error. Every call that runs is
        reported as a `tool_call` span: the tool, a digest of the arguments (never the
        arguments), the size of the result, whether it failed, and how long it took.
        """
        if ctx.token is not None:
            decision = decide_tool(ctx.token.capabilities, self.name)
            if not decision.allowed:
                refuse(ctx, action="tool_call", subject=self.name, decision=decision)
        span_id = new_span_id()
        started = now_ns()
        clock = time.perf_counter_ns()
        digest, size = args_digest(raw)
        output: str | None = None
        try:
            output = await self._run(ctx, raw)
            return output
        finally:
            attributes: dict[str, Any] = {
                "tool": self.name,
                "args_sha256": digest,
                "args_bytes": size,
                "result_bytes": len(output.encode("utf-8")) if output is not None else 0,
                "is_error": output is None,
                "duration_ns": time.perf_counter_ns() - clock,
            }
            if ctx.token is not None:
                attributes["policy_version"] = ctx.token.policy_version
            ctx.events.span(
                span_id=span_id,
                parent_id=ctx.turn_span_id,
                kind="tool_call",
                name=self.name,
                agent=ctx.principal,
                started_at=rfc3339_ns(started),
                ended_at=rfc3339_ns(now_ns()),
                status="ok" if output is not None else "error",
                attributes=attributes,
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


def refuse(
    ctx: ToolContext,
    *,
    action: str,
    subject: str,
    decision: Decision,
    extra: dict[str, Any] | None = None,
) -> NoReturn:
    """Puts a refusal on the record as a `policy` span and raises it for the model to see."""
    now = rfc3339_ns(now_ns())
    attributes: dict[str, Any] = {
        "decision": decision.verdict,
        "rule": decision.rule,
        "reason": decision.reason,
        "action": action,
        "subject": subject,
        "policy_version": ctx.token.policy_version if ctx.token is not None else 0,
    }
    if extra:
        attributes.update(extra)
    ctx.events.span(
        span_id=new_span_id(),
        parent_id=ctx.turn_span_id,
        kind="policy",
        name=subject,
        agent=ctx.principal,
        started_at=now,
        ended_at=now,
        status="error",
        attributes=attributes,
    )
    ctx.events.log(
        "warning", f"{ctx.agent_name}: {action} {subject} refused by policy ({decision.rule})"
    )
    raise ToolError(f"not allowed by this agent's policy ({decision.rule}): {decision.reason}")


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
