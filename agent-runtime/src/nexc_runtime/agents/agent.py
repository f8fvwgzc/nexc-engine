"""The Agent: one persona running the model <-> tools loop inside the run's workspace."""

from __future__ import annotations

import asyncio
import logging
import time
from collections.abc import Awaitable
from dataclasses import dataclass
from typing import Any

from ..config import Settings
from ..llm import LLMProvider, ToolCall, ToolResult
from ..prompts import build_subagent_prompt, build_system_prompt
from ..streaming import EventStream
from ..tools import Tool, ToolContext, ToolError, Workspace, build_toolset
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


class AgentFailure(Exception):
    def __init__(self, message: str, *, retryable: bool = False) -> None:
        super().__init__(message)
        self.retryable = retryable


@dataclass(slots=True)
class Meter:
    """Token totals for the whole run (root agent + all sub-agents)."""

    tokens_in: int = 0
    tokens_out: int = 0


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
    kind_description: str = ""
    produces_artifact: bool = False


class Agent:
    def __init__(
        self,
        *,
        scope: RunScope,
        record: AgentRecord,
        budget: Budget,
        system_prompt: str = "",
        stream_text: bool = False,
    ) -> None:
        self.scope = scope
        self.record = record
        self.budget = budget
        self.stream_text = stream_text
        can_spawn = AgentRegistry.can_spawn(record)
        self.tools: dict[str, Tool[Any]] = build_toolset(
            can_spawn=can_spawn, allow_code_exec=scope.allow_code_exec
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
        )
        last_text = ""

        for _ in range(scope.max_turns):
            if self.budget.exhausted:
                return self._give_up(last_text, "token budget exhausted")
            max_tokens = max(MIN_OUTPUT_TOKENS, min(MAX_OUTPUT_TOKENS, self.budget.remaining))
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
            self._account(turn.input_tokens, turn.output_tokens)
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
    def _account(self, tokens_in: int, tokens_out: int) -> None:
        self.budget.spend(tokens_in + tokens_out)
        self.scope.meter.tokens_in += tokens_in
        self.scope.meter.tokens_out += tokens_out
        self.scope.events.tokens(tokens_in, tokens_out)

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
        except (AgentFailure, SpawnRefused) as exc:
            return ToolResult(call.id, str(exc), is_error=True)
        except OSError as exc:
            log.warning("tool %s raised %s", call.name, type(exc).__name__)
            return ToolResult(call.id, f"{call.name} failed: {exc.strerror or exc}", is_error=True)
        return ToolResult(call.id, output)

    async def _spawn(self, role: str, task: str) -> str:
        scope = self.scope
        try:
            budget = self.budget.allocate_child()
            self._child_seq += 1
            name = f"{self.record.name}/{role}-{self._child_seq}"
            record = scope.registry.birth(name, role, parent=self.record)
        except SpawnRefused as exc:
            raise ToolError(str(exc)) from None

        scope.events.spawn(name, role)
        scope.events.log("info", f"{name} born with a budget of {budget.limit} tokens")
        child = Agent(scope=scope, record=record, budget=budget)
        try:
            answer = await child.run(build_subagent_prompt(parent_role=self.record.role, task=task))
        except AgentFailure as exc:
            raise ToolError(f"sub-agent {name} failed: {exc}") from None
        finally:
            scope.registry.retire(record)
        scope.events.log("info", f"{name} finished (spent {budget.spent} tokens)")
        return answer
