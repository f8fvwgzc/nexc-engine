"""The Agent: one persona running the model <-> tools loop inside the run's workspace."""

from __future__ import annotations

import asyncio
import json
import logging
import time
from collections.abc import Awaitable
from dataclasses import dataclass, field
from typing import Any

from ..approvals import ApprovalRegistry, ApprovalTimeout
from ..config import Settings
from ..control import RunControl
from ..evidence import Sources
from ..injection import confirm_with_model, screen, withheld_notice
from ..llm import LLMProvider, ToolCall, ToolResult, Turn
from ..policy import AgentToken, decide_spawn
from ..prompts import build_subagent_prompt, build_system_prompt
from ..streaming import EventStream
from ..tools import Tool, ToolContext, ToolError, Workspace, build_toolset
from ..tools.base import refuse
from ..trace import new_span_id, now_ns, rfc3339_ns
from .registry import AgentRecord, AgentRegistry, Budget, SpawnRefused

log = logging.getLogger(__name__)

MAX_OUTPUT_TOKENS = 64_000
MIN_OUTPUT_TOKENS = 1_024
TRUNCATED_TOOL_INPUT = (
    "Not executed: your response hit the output token limit, so this tool input was truncated. "
    "Retry with smaller pieces (for example split a large file into several files)."
)


# While a model turn is in flight the NDJSON stream would otherwise be silent; providers that only
# answer at the end of a turn (the Claude Code CLI) can take minutes. A periodic log line keeps
# the backend's read timeout from firing and shows progress in the UI.
HEARTBEAT_EVERY_S = 20.0


async def with_heartbeat[T](work: Awaitable[T], events: EventStream, label: str) -> T:
    """Awaits `work`, emitting a `still working` log line every `HEARTBEAT_EVERY_S` seconds."""
    task = asyncio.ensure_future(work)
    started = time.monotonic()
    try:
        while True:
            done, _ = await asyncio.wait({task}, timeout=HEARTBEAT_EVERY_S)
            if done:
                return task.result()
            elapsed = int(time.monotonic() - started)
            events.log("info", f"{label} still working ({elapsed}s)")
    except asyncio.CancelledError:
        task.cancel()
        raise


def completion_text(turn: Turn) -> str:
    """What the model answered, as the recording keeps it: its text and the tool calls it asked
    for, each with its arguments as JSON."""
    parts = [turn.text] if turn.text.strip() else []
    for call in turn.tool_calls:
        try:
            arguments = json.dumps(call.input, ensure_ascii=False, sort_keys=True, indent=2)
        except (TypeError, ValueError):
            arguments = repr(call.input)
        parts.append(f"[tool call {call.name}]\n{arguments}")
    return "\n\n".join(parts)


class AgentFailure(Exception):
    def __init__(self, message: str, *, retryable: bool = False) -> None:
        super().__init__(message)
        self.retryable = retryable


@dataclass(slots=True)
class Meter:
    """Token totals for the whole run (root agent + all sub-agents)."""

    tokens_in: int = 0
    tokens_out: int = 0
    tokens_cached: int = 0


@dataclass(slots=True)
class RunScope:
    """Everything the agents of one run share."""

    provider: LLMProvider
    registry: AgentRegistry
    workspace: Workspace
    events: EventStream
    settings: Settings
    meter: Meter
    kind: str
    max_turns: int
    allow_code_exec: bool
    # Signs the narrowed tokens sub-agents are born with.
    token_key: bytes
    kind_description: str = ""
    produces_artifact: bool = False
    # Where the backend delivers decisions on held tool calls (None: nothing can wait).
    approvals: ApprovalRegistry | None = None
    # The backend's hold on the run over the gRPC stream (None on the NDJSON wire).
    control: RunControl | None = None
    # Tools the backend performs for the agents: `(name, description, input schema)`.
    gateway_tools: tuple[tuple[str, str, dict[str, Any]], ...] = ()
    # What the node was given, by citation id, with a title each.
    sources: Sources = field(default_factory=Sources)


class Agent:
    def __init__(
        self,
        *,
        scope: RunScope,
        record: AgentRecord,
        budget: Budget,
        token: AgentToken,
        system_prompt: str = "",
        stream_text: bool = False,
        parent_span_id: str | None = None,
    ) -> None:
        self.scope = scope
        self.record = record
        self.budget = budget
        # What this agent may do, signed by the backend (narrowed by the parent for a sub-agent).
        self.token = token
        self.stream_text = stream_text
        # The spawn span this agent was born in (None for the root): its turns' parent.
        self.parent_span_id = parent_span_id
        # The turn in progress, the parent of whatever this agent spawns during it.
        self._turn_span_id: str | None = None
        # The tool is offered where the token still allows a level below; a token that
        # forbids the tool itself leaves it visible and refuses it when called.
        can_spawn = AgentRegistry.can_spawn(record) and record.depth < token.capabilities.max_depth
        self.tools: dict[str, Tool[Any]] = build_toolset(
            can_spawn=can_spawn,
            allow_code_exec=scope.allow_code_exec,
            gateway_tools=scope.gateway_tools,
        )
        self.system = build_system_prompt(
            name=record.name,
            role=record.role,
            custom_prompt=system_prompt,
            kind=scope.kind if record.depth == 0 else "subtask",
            kind_description=scope.kind_description if record.depth == 0 else "",
            produces_artifact=scope.produces_artifact and record.depth == 0,
            can_spawn=can_spawn,
            code_exec=scope.allow_code_exec,
        )
        self._child_seq = 0

    async def run(self, prompt: str) -> str:
        scope = self.scope
        provider = scope.provider
        specs = [tool.spec() for tool in self.tools.values()]
        conversation = provider.new_conversation(self.system, prompt)
        ctx = ToolContext(
            workspace=scope.workspace,
            events=scope.events,
            settings=scope.settings,
            agent_name=self.record.name,
            spawn=self._spawn if "spawn_subagent" in self.tools else None,
            produces_artifact=scope.produces_artifact,
            token=self.token,
            principal=self.record.principal,
            approvals=scope.approvals,
            control=scope.control,
            sources=scope.sources,
        )
        self._ctx = ctx
        last_text = ""
        # What the next turn is given, as the recording keeps it: the system prompt and the
        # task first, then whatever was added since the last turn (tool results, nudges).
        pending_input: list[str] = [f"[system]\n{self.system}", f"[user]\n{prompt}"]

        for number in range(1, scope.max_turns + 1):
            if self.budget.exhausted:
                return self._give_up(last_text, "token budget exhausted")
            self._take_policy_update(ctx)
            max_tokens = max(MIN_OUTPUT_TOKENS, min(MAX_OUTPUT_TOKENS, self.budget.remaining))
            turn_id = new_span_id()
            started = now_ns()
            if self.token.recording.prompts:
                scope.events.payload(
                    span_id=turn_id, kind="prompt", content="\n\n".join(pending_input)
                )
            pending_input = []
            try:
                turn = await with_heartbeat(
                    provider.next_turn(
                        conversation,
                        specs,
                        max_tokens=max_tokens,
                        on_text=scope.events.delta if self.stream_text else None,
                    ),
                    scope.events,
                    label=self.record.name,
                )
            except Exception:
                self._turn_span(turn_id, started, number, None)
                raise
            self._turn_span(turn_id, started, number, turn)
            if self.token.recording.completions:
                scope.events.payload(
                    span_id=turn_id, kind="completion", content=completion_text(turn)
                )
            self._turn_span_id = turn_id
            ctx.turn_span_id = turn_id
            self._account(turn.input_tokens, turn.output_tokens, turn.cached_tokens)
            if turn.text.strip():
                last_text = turn.text

            if turn.stop == "refusal":
                reason = f" ({turn.detail})" if turn.detail else ""
                raise AgentFailure(f"the model declined this task{reason}", retryable=False)

            if turn.tool_calls:
                if turn.stop == "max_tokens":
                    results = [
                        ToolResult(c.id, TRUNCATED_TOOL_INPUT, True) for c in turn.tool_calls
                    ]
                else:
                    results = [await self._call_tool(ctx, call) for call in turn.tool_calls]
                provider.add_tool_results(conversation, results)
                pending_input.extend(
                    f"[tool result {call.name}{' (error)' if result.is_error else ''}]\n"
                    f"{result.content}"
                    for call, result in zip(turn.tool_calls, results, strict=False)
                )
                if ctx.final_answer is not None:
                    return ctx.final_answer
                continue

            if turn.stop == "pause_turn":
                continue
            if turn.stop == "max_tokens":
                scope.events.log("warning", f"{self.record.name}: answer truncated at output limit")
            return turn.text or last_text

        return self._give_up(last_text, f"reached the limit of {scope.max_turns} turns")

    # --- internals --------------------------------------------------------------------------
    def _take_policy_update(self, ctx: ToolContext) -> None:
        """The root agent decides with the token the backend sent last (`policy_update`); a
        sub-agent keeps the narrowed token it was born with."""
        control = self.scope.control
        if control is None or self.record.depth != 0:
            return
        fresh = control.take_token(self.token)
        if fresh is not self.token:
            self.token = fresh
            ctx.token = fresh
            self.scope.events.log(
                "info",
                f"{self.record.name}: policy updated to version {fresh.policy_version}",
            )

    def _turn_span(self, span_id: str, started: int, number: int, turn: Turn | None) -> None:
        """Reports one model turn as an `llm_call` span (`turn` is None when the call failed)."""
        provider = self.scope.provider
        attributes: dict[str, Any] = {
            "provider": provider.name,
            "model": provider.model,
            "turn": number,
            "policy_version": self.token.policy_version,
        }
        if turn is not None:
            attributes.update(
                tokens_in=turn.input_tokens,
                tokens_out=turn.output_tokens,
                tokens_cached=turn.cached_tokens,
                stop_reason=turn.stop,
                tool_calls=len(turn.tool_calls),
            )
        self.scope.events.span(
            span_id=span_id,
            parent_id=self.parent_span_id,
            kind="llm_call",
            name=provider.model,
            agent=self.record.principal,
            started_at=rfc3339_ns(started),
            ended_at=rfc3339_ns(now_ns()),
            status="ok" if turn is not None else "error",
            attributes=attributes,
        )

    def _account(self, tokens_in: int, tokens_out: int, tokens_cached: int = 0) -> None:
        self.budget.spend(tokens_in + tokens_out)
        self.scope.meter.tokens_in += tokens_in
        self.scope.meter.tokens_out += tokens_out
        self.scope.meter.tokens_cached += tokens_cached
        self.scope.events.tokens(tokens_in, tokens_out, tokens_cached)

    def _give_up(self, last_text: str, reason: str) -> str:
        if last_text.strip():
            self.scope.events.log("warning", f"{self.record.name}: {reason}; using last answer")
            return last_text
        raise AgentFailure(f"{self.record.name}: {reason} before producing an answer")

    async def _call_tool(self, ctx: ToolContext, call: ToolCall) -> ToolResult:
        tool = self.tools.get(call.name)
        if tool is None:
            return ToolResult(call.id, f"unknown tool {call.name!r}", is_error=True)
        self.scope.events.log("info", f"{self.record.name} -> {call.name}")
        try:
            output = await tool.invoke(ctx, call.input)
        except ToolError as exc:
            self.scope.events.log("warning", f"{self.record.name}: {call.name} failed: {exc}")
            return ToolResult(call.id, str(exc), is_error=True)
        except ApprovalTimeout as exc:
            # Nobody decided in time: the node fails rather than the model going on without.
            raise AgentFailure(str(exc), retryable=False) from None
        except (AgentFailure, SpawnRefused) as exc:
            return ToolResult(call.id, str(exc), is_error=True)
        except OSError as exc:
            log.warning("tool %s raised %s", call.name, type(exc).__name__)
            return ToolResult(call.id, f"{call.name} failed: {exc.strerror or exc}", is_error=True)
        return ToolResult(call.id, await self._screen_result(call.name, output))

    async def _screen_result(self, tool: str, output: str) -> str:
        """The prompt-injection screen on a tool result about to go to the model: a finding is
        a `policy` span; under `injection.action: block` the result is withheld."""
        finding = screen(output)
        if finding is None:
            return output
        rules = self.token.rules
        if rules.injection_model_check and self.scope.settings.runtime_injection_model_check:
            confirmed, tokens_in, tokens_out = await confirm_with_model(self.scope.provider, output)
            if tokens_in or tokens_out:
                self._account(tokens_in, tokens_out)
            if not confirmed:
                return output
        block = rules.injection_action == "block"
        reason = f"instructions aimed at the model in the result of {tool}: {finding.describe()}"
        now = rfc3339_ns(now_ns())
        self.scope.events.span(
            span_id=new_span_id(),
            parent_id=self._turn_span_id,
            kind="policy",
            name="injection",
            agent=self.record.principal,
            started_at=now,
            ended_at=now,
            status="error" if block else "ok",
            attributes={
                "decision": "deny" if block else "alert",
                "rule": "injection",
                "reason": reason,
                "action": "tool_result",
                "subject": tool,
                "severity": finding.severity,
                "patterns": list(finding.patterns),
                "hidden_chars": finding.hidden_chars,
                "result_bytes": len(output.encode("utf-8")),
                "policy_version": self.token.policy_version,
            },
        )
        self.scope.events.log(
            "warning",
            f"{self.record.name}: {reason}" + ("; withheld" if block else "; let through, flagged"),
        )
        return withheld_notice(tool, finding) if block else output

    async def _spawn(self, role: str, task: str) -> str:
        scope = self.scope
        # The token decides first: depth, fan-out and the roles this agent may delegate to.
        depth = self.record.depth + 1
        decision = decide_spawn(
            self.token.capabilities, role, depth=depth, children=self._child_seq
        )
        if not decision.allowed:
            refuse(
                self._ctx,
                action="spawn",
                subject=role,
                decision=decision,
                extra={"depth": depth, "children": self._child_seq},
            )
        try:
            budget = self.budget.allocate_child()
            self._child_seq += 1
            name = f"{self.record.name}/{role}-{self._child_seq}"
            principal = f"{self.record.principal}/{role}-{self._child_seq}"
            record = scope.registry.birth(name, role, parent=self.record, principal=principal)
        except SpawnRefused as exc:
            raise ToolError(str(exc)) from None
        # The child is born with a narrowed copy of this token: never wider than this one.
        child_token = self.token.child(principal, scope.token_key)

        scope.events.spawn(name, role)
        # The birth is an instant: the child's turns hang under it as they happen.
        spawn_id = new_span_id()
        born = rfc3339_ns(now_ns())
        scope.events.span(
            span_id=spawn_id,
            parent_id=self._turn_span_id,
            kind="spawn",
            name=principal,
            agent=self.record.principal,
            started_at=born,
            ended_at=born,
            status="ok",
            attributes={
                "child": principal,
                "role": role,
                "depth": record.depth,
                "budget_tokens": budget.limit,
                "policy_version": child_token.policy_version,
            },
        )
        scope.events.log("info", f"{name} born with a budget of {budget.limit} tokens")
        child = Agent(
            scope=scope,
            record=record,
            budget=budget,
            token=child_token,
            parent_span_id=spawn_id,
        )
        # The child runs as a task of its own, so that an operator can stop it (`cancel_agent`
        # down the stream) without stopping this agent.
        work = asyncio.create_task(
            child.run(build_subagent_prompt(parent_role=self.record.role, task=task))
        )
        control = scope.control
        if control is not None:
            control.watch_child(spawn_id, work)
        try:
            answer = await work
        except asyncio.CancelledError:
            reason = control.cancelled_reason(spawn_id) if control is not None else None
            current = asyncio.current_task()
            if reason is None or (current is not None and current.cancelling()):
                # The run itself is being cancelled: this agent goes down with its child.
                raise
            scope.events.log("warning", f"{name} stopped: {reason}")
            raise ToolError(f"sub-agent {name} was {reason}") from None
        except AgentFailure as exc:
            raise ToolError(f"sub-agent {name} failed: {exc}") from None
        finally:
            if control is not None:
                control.forget_child(spawn_id)
            scope.registry.retire(record)
        scope.events.log("info", f"{name} finished (spent {budget.spent} tokens)")
        return answer
