"""Tool plumbing: definition, execution context, strict JSON schemas and validation."""

from __future__ import annotations

import asyncio
import copy
import json
import time
from collections import deque
from collections.abc import Awaitable, Callable
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Any, NoReturn

from pydantic import BaseModel, ConfigDict, ValidationError

from ..approvals import ApprovalOutcome, ApprovalRegistry, ApprovalTimeout
from ..config import Settings
from ..evidence import Sources

if TYPE_CHECKING:
    from ..control import RunControl
from ..llm.base import ToolSpec
from ..policy import AgentToken, Decision, decide_tool_with
from ..streaming import EventStream
from ..trace import args_digest, new_span_id, now_ns, rfc3339_ns
from .workspace import PathError, Workspace

# While a call waits for its approval the stream would otherwise be silent.
APPROVAL_HEARTBEAT_S = 20.0

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
    # Where decisions on held calls arrive (None in unit tests of single tools).
    approvals: ApprovalRegistry | None = None
    # When the agent's recent tool calls started, against `limits.tool_calls_per_minute`.
    recent_calls: deque[float] = field(default_factory=deque)
    # The backend's hold on the run over the gRPC stream (None on the NDJSON wire and in
    # unit tests): where gateway results and token updates arrive.
    control: RunControl | None = None
    # The tool being invoked, and the approval it runs under, for handlers that need them.
    current_tool: str | None = None
    pending_approval_id: str | None = None
    # What the agent was given, by citation id: deliverables that cite ids get a Sources section.
    sources: Sources = field(default_factory=Sources)

    def __post_init__(self) -> None:
        if not self.principal:
            self.principal = self.agent_name

    def over_pace(self) -> Decision | None:
        """The refusal when the agent already made its tool calls for this minute."""
        if self.token is None or self.token.rules.tool_calls_per_minute is None:
            return None
        limit = self.token.rules.tool_calls_per_minute
        now = time.monotonic()
        while self.recent_calls and now - self.recent_calls[0] > 60.0:
            self.recent_calls.popleft()
        if len(self.recent_calls) >= limit:
            return Decision(
                False,
                "limits.tool_calls_per_minute",
                f"already made {limit} tool call(s) this minute",
            )
        self.recent_calls.append(now)
        return None


@dataclass(frozen=True, slots=True)
class Tool[A: BaseModel]:
    name: str
    description: str
    args: type[A]
    handler: Callable[[ToolContext, A], Awaitable[str]]
    # A schema given as is (a gateway tool's, as its server published it) instead of the
    # argument model's.
    input_schema: dict[str, Any] | None = None
    # Whether `invoke` reports the call as a `tool_call` span; the backend records the spans
    # of the calls it performs itself.
    traced: bool = True

    def spec(self) -> ToolSpec:
        schema = self.input_schema if self.input_schema is not None else strict_schema(self.args)
        return ToolSpec(self.name, self.description, schema)

    async def invoke(self, ctx: ToolContext, raw: dict[str, Any]) -> str:
        """Decide the call against the agent's token, validate `raw` against the argument model,
        then run the handler.

        A call the policy refuses never runs: it is reported as a `policy` span (the rule and
        the reason) and the model sees the refusal as a tool error. Every call that runs is
        reported as a `tool_call` span: the tool, a digest of the arguments (never the
        arguments), the size of the result, whether it failed, and how long it took.
        """
        approval_id: str | None = None
        if ctx.token is not None:
            token = ctx.token
            decision = decide_tool_with(token.capabilities, token.rules, token.labels, self.name)
            if decision.needs_approval:
                approval_id = await wait_for_approval(ctx, token, self.name, raw, decision)
            elif not decision.allowed:
                refuse(ctx, action="tool_call", subject=self.name, decision=decision)
            # Finishing is not an action on the world: the pace does not hold it back.
            paced = ctx.over_pace() if self.name != "finish" else None
            if paced is not None:
                refuse(ctx, action="tool_call", subject=self.name, decision=paced)
        span_id = new_span_id()
        started = now_ns()
        clock = time.perf_counter_ns()
        digest, size = args_digest(raw)
        # What the call got and gave, when the workspace records tool calls (the backend records
        # the calls it performs itself).
        record_io = self.traced and ctx.token is not None and ctx.token.recording.tool_io
        if record_io:
            try:
                arguments = json.dumps(raw, ensure_ascii=False, sort_keys=True, indent=2)
            except (TypeError, ValueError):
                arguments = repr(raw)
            ctx.events.payload(span_id=span_id, kind="tool_input", content=arguments)
        output: str | None = None
        failure: str | None = None
        ctx.current_tool = self.name
        ctx.pending_approval_id = approval_id
        try:
            output = await self._run(ctx, raw)
            return output
        except ToolError as exc:
            failure = str(exc)
            raise
        finally:
            if record_io:
                ctx.events.payload(
                    span_id=span_id,
                    kind="tool_output",
                    content=output if output is not None else f"[error] {failure or 'failed'}",
                )
            ctx.current_tool = None
            ctx.pending_approval_id = None
            # A gateway tool's spans are the backend's to record: it performed the call.
            if self.traced:
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
                if approval_id is not None:
                    attributes["approval_id"] = approval_id
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


async def wait_for_approval(
    ctx: ToolContext, token: AgentToken, tool: str, raw: dict[str, Any], decision: Decision
) -> str:
    """Asks for an approval of the call and waits for the decision.

    The request is a `policy` span (decision `require_approval`, the approval id, a digest of
    the arguments) the backend turns into an inbox item. Returns the approval id once a human
    said yes; raises `ToolError` with the reason when they said no (the model sees it), and
    `ApprovalTimeout` when nobody decided in time (the node fails).
    """
    approval_id = new_span_id()
    digest, _ = args_digest(raw)
    timeout = token.rules.approval_timeout_s
    ctx.events.log(
        "info", f"{ctx.agent_name}: {tool} waits for an approval ({decision.rule}, {timeout}s)"
    )
    now = rfc3339_ns(now_ns())
    request_span_id = new_span_id()
    ctx.events.span(
        span_id=request_span_id,
        parent_id=ctx.turn_span_id,
        kind="policy",
        name=tool,
        agent=ctx.principal,
        started_at=now,
        ended_at=now,
        status="ok",
        attributes={
            "decision": "require_approval",
            "rule": decision.rule,
            "reason": decision.reason,
            "action": "tool_call",
            "subject": tool,
            "approval_id": approval_id,
            "args_sha256": digest,
            "policy_version": token.policy_version,
        },
    )
    if ctx.approvals is None:
        raise ApprovalTimeout(f"{tool} needs an approval and this runtime cannot receive one")
    future = ctx.approvals.register(approval_id)
    if ctx.control is not None:
        # The gRPC wire asks explicitly; the decision comes back as a command.
        ctx.events.approval_request(
            approval_id=approval_id,
            tool=tool,
            rule=decision.rule,
            reason=decision.reason,
            args_sha256=digest,
            span_id=request_span_id,
            agent=ctx.principal,
            turn_span_id=ctx.turn_span_id,
        )
    deadline = time.monotonic() + timeout
    try:
        while True:
            left = deadline - time.monotonic()
            if left <= 0:
                raise ApprovalTimeout(f"approval for {tool} not given within {timeout}s")
            done, _ = await asyncio.wait({future}, timeout=min(APPROVAL_HEARTBEAT_S, left))
            if done:
                break
            ctx.events.log("info", f"{ctx.agent_name}: still waiting for the approval of {tool}")
    finally:
        ctx.approvals.forget(approval_id)
    outcome: ApprovalOutcome = future.result()
    who = outcome.decided_by or "an approver"
    if outcome.approved:
        ctx.events.log("info", f"{ctx.agent_name}: {tool} approved by {who}")
        return approval_id
    ctx.events.log("warning", f"{ctx.agent_name}: {tool} denied by {who}: {outcome.reason}")
    raise ToolError(f"{tool} was denied by {who}: {outcome.reason}")


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
