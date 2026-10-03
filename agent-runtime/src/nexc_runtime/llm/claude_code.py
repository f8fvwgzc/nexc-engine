"""`claude_code` provider: drives the agent tool loop through the local Claude Code CLI.

Each assistant turn is one isolated `claude -p` invocation (built-in tools, MCP, settings files
and session persistence disabled; minimal environment, so the CLI uses its own login and never
sees nexc secrets or an API key). The CLI cannot call *our* tools natively, so every turn is a
structured-output request (`--json-schema`): the model answers with its visible `text` plus the
`actions` (our tools) it wants performed now. The runtime executes them exactly as for the other
providers. They are deliberately not called "tools" in the prompt: the model would otherwise try
to invoke them natively, which fails with tools disabled.
"""

from __future__ import annotations

import asyncio
import contextlib
import json
import logging
import os
import tempfile
from typing import Any

from .base import Conversation, LLMError, StopKind, TextSink, ToolCall, ToolResult, ToolSpec, Turn

log = logging.getLogger(__name__)

DEFAULT_MODEL = "sonnet"
_PASSTHROUGH_ENV = (
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TMPDIR",
    "LANG",
    "LC_ALL",
    "CLAUDE_CONFIG_DIR",
    "NODE_EXTRA_CA_CERTS",
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "NO_PROXY",
)
_MAX_RESULT_CHARS = 12_000
# Failures that a retry cannot fix; everything else (overload, max turns, transient CLI errors)
# is reported as retryable so the scheduler tries again.
_FATAL_MARKERS = ("login", "log in", "api key", "credit", "billing", "unauthorized", "forbidden")
# The CLI caps model output at a default that a large structured reply (e.g. a whole file in an
# action input) can exceed; raise it to what the agent asked for, within the models' limits.
_MIN_OUTPUT_TOKENS = 8_192
_MAX_OUTPUT_TOKENS = 64_000


def describe_failure(result: dict[str, Any]) -> tuple[str, bool]:
    """Message and retryability of a failed CLI result (`subtype` + `errors`, or `result`)."""
    parts = [str(result.get("subtype") or "error")]
    errors = result.get("errors")
    if isinstance(errors, list) and errors:
        parts.append("; ".join(str(e) for e in errors))
    elif result.get("result"):
        parts.append(str(result["result"]))
    message = ": ".join(parts)[:300]
    retryable = not any(marker in message.lower() for marker in _FATAL_MARKERS)
    return f"claude CLI: {message}", retryable


_PROTOCOL = """\

## How you act in this session
You have NO callable tools here: invoking any tool directly fails. Instead, every reply is JSON:
- `text`: what you say this turn (progress, or your final answer when you are done).
- `actions`: actions for the host to perform now, each `{"action": <name>, "input": {...}}` with \
`input` following that action's schema below. The host performs them and shows you the results \
on your next turn. Use an empty list when you are finished; then `text` is your final answer.
Keep every action input modest (a few thousand words at most): put the main deliverable in your \
final `text` - it is passed on and saved - and split large files into several smaller ones.

## Actions you can request (via `actions` only)
"""

# Used when the CLI cannot produce schema-valid JSON for a turn (typically a very large reply).
_PLAIN_FALLBACK = (
    "\n\n## This turn only\nReply in plain Markdown with your complete final answer. "
    "Do not request any actions and do not wrap the answer in JSON."
)
_STRUCTURED_FAILURE = "error_max_structured_output_retries"


def _turn_schema(tools: list[ToolSpec]) -> dict[str, Any]:
    action: dict[str, Any] = {
        "type": "object",
        "properties": {
            "action": {"type": "string", "enum": [t.name for t in tools]},
            "input": {"type": "object"},
        },
        "required": ["action", "input"],
    }
    return {
        "type": "object",
        "properties": {
            "text": {"type": "string"},
            "actions": {"type": "array", "items": action} if tools else {"type": "array"},
        },
        "required": ["text", "actions"],
    }


def _tool_catalog(tools: list[ToolSpec]) -> str:
    if not tools:
        return "(no actions available - answer directly)\n"
    return "".join(
        f"- `{t.name}`: {t.description}\n  input schema: {json.dumps(t.input_schema)}\n"
        for t in tools
    )


def _clip(text: str) -> str:
    if len(text) <= _MAX_RESULT_CHARS:
        return text
    return text[:_MAX_RESULT_CHARS] + f"\n... [{len(text) - _MAX_RESULT_CHARS} chars truncated]"


def render_transcript(conversation: Conversation) -> str:
    """The whole conversation as one prompt (the CLI is stateless per invocation)."""
    parts: list[str] = []
    for message in conversation.messages:
        role = message["role"]
        if role == "user":
            parts.append(f"### User\n\n{message['content']}")
        elif role == "assistant":
            calls = "".join(
                f"\n- {c['name']}({json.dumps(c['input'])})" for c in message["tool_calls"]
            )
            parts.append(
                f"### You\n\n{message['content']}" + (f"\n\nActions:{calls}" if calls else "")
            )
        else:  # action results
            lines = [
                f"- {r['name']} [{'error' if r['is_error'] else 'ok'}]: {_clip(r['content'])}"
                for r in message["results"]
            ]
            parts.append("### Action results\n\n" + "\n".join(lines))
    return "\n\n".join(parts)


class ClaudeCodeProvider:
    name = "claude_code"

    def __init__(self, *, model: str, binary: str = "claude", timeout_s: float = 600.0) -> None:
        self.model = model.strip() or DEFAULT_MODEL
        self._binary = binary
        self._timeout_s = timeout_s
        self._calls = 0
        self._pending: dict[str, str] = {}  # tool_use_id -> tool name, for rendering results
        self._workdir = tempfile.mkdtemp(prefix="nexc-claude-code-")

    def new_conversation(self, system: str, prompt: str) -> Conversation:
        return Conversation(system=system, messages=[{"role": "user", "content": prompt}])

    def add_tool_results(self, conversation: Conversation, results: list[ToolResult]) -> None:
        conversation.messages.append(
            {
                "role": "tool",
                "results": [
                    {
                        "name": self._pending.pop(r.tool_use_id, "tool"),
                        "content": r.content,
                        "is_error": r.is_error,
                    }
                    for r in results
                ],
            }
        )

    def add_user_text(self, conversation: Conversation, text: str) -> None:
        conversation.messages.append({"role": "user", "content": text})

    async def aclose(self) -> None:
        with contextlib.suppress(OSError):
            os.rmdir(self._workdir)

    async def next_turn(
        self,
        conversation: Conversation,
        tools: list[ToolSpec],
        *,
        max_tokens: int,
        on_text: TextSink | None = None,
    ) -> Turn:
        system = conversation.system + _PROTOCOL + _tool_catalog(tools)
        transcript = render_transcript(conversation)
        try:
            result = await self._invoke(system, transcript, _turn_schema(tools), max_tokens)
            output = result.get("structured_output")
        except LLMError as exc:
            if _STRUCTURED_FAILURE not in exc.message:
                raise
            # The JSON envelope is what failed; ask for the answer as plain text instead.
            log.info("structured turn failed; retrying as a plain final answer")
            result = await self._invoke(
                conversation.system + _PLAIN_FALLBACK, transcript, None, max_tokens
            )
            output = {"text": str(result.get("result") or ""), "actions": []}
        if not isinstance(output, dict):
            raise LLMError("claude CLI returned no structured turn", retryable=True)
        text = str(output.get("text", ""))
        known = {t.name for t in tools}
        calls: list[ToolCall] = []
        for raw in output.get("actions") or []:
            if not isinstance(raw, dict) or raw.get("action") not in known:
                continue
            self._calls += 1
            tool_input = raw.get("input")
            call = ToolCall(
                id=f"cc_{self._calls}",
                name=str(raw["action"]),
                input=tool_input if isinstance(tool_input, dict) else {},
            )
            self._pending[call.id] = call.name
            calls.append(call)

        conversation.messages.append(
            {
                "role": "assistant",
                "content": text,
                "tool_calls": [{"name": c.name, "input": c.input} for c in calls],
            }
        )
        if on_text is not None and text:
            on_text(text)

        usage = result.get("usage") or {}
        stop: StopKind = "tool_use" if calls else "end_turn"
        if result.get("stop_reason") == "max_tokens":
            stop = "max_tokens"
        return Turn(
            text=text,
            tool_calls=calls,
            stop=stop,
            input_tokens=int(usage.get("input_tokens", 0))
            + int(usage.get("cache_creation_input_tokens", 0))
            + int(usage.get("cache_read_input_tokens", 0)),
            output_tokens=int(usage.get("output_tokens", 0)),
        )

    async def _invoke(
        self, system: str, prompt: str, schema: dict[str, Any] | None, max_tokens: int
    ) -> dict[str, Any]:
        args = [
            "-p",
            "--output-format",
            "json",
            "--model",
            self.model,
            "--tools",
            "",
            "--setting-sources",
            "",
            "--strict-mcp-config",
            "--no-session-persistence",
            "--system-prompt",
            system,
        ]
        if schema is not None:
            args += ["--json-schema", json.dumps(schema)]
        env = {k: v for k in _PASSTHROUGH_ENV if (v := os.environ.get(k)) is not None}
        output_cap = min(max(max_tokens, _MIN_OUTPUT_TOKENS), _MAX_OUTPUT_TOKENS)
        env["CLAUDE_CODE_MAX_OUTPUT_TOKENS"] = str(output_cap)
        try:
            proc = await asyncio.create_subprocess_exec(
                self._binary,
                *args,
                cwd=self._workdir,
                env=env,
                stdin=asyncio.subprocess.PIPE,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
        except OSError as exc:
            raise LLMError(
                f"cannot start `{self._binary}` ({exc.strerror}); install Claude Code and log in, "
                "or set RUNTIME_CLAUDE_BIN",
                retryable=False,
            ) from exc
        try:
            stdout, stderr = await asyncio.wait_for(
                proc.communicate(prompt.encode()), timeout=self._timeout_s
            )
        except TimeoutError as exc:
            proc.kill()
            await proc.wait()
            raise LLMError("claude CLI timed out", retryable=True) from exc

        try:
            result: dict[str, Any] = json.loads(stdout)
        except json.JSONDecodeError as exc:
            detail = stderr.decode(errors="replace").strip()[:300]
            raise LLMError(
                f"claude CLI exited ({proc.returncode}) without a result: {detail}", retryable=True
            ) from exc
        if result.get("is_error") or result.get("subtype") != "success":
            message, retryable = describe_failure(result)
            log.warning("claude CLI turn failed: %s (retryable=%s)", message, retryable)
            raise LLMError(message, retryable=retryable)
        return result
