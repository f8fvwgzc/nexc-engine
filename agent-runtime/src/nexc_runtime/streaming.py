"""The event stream of one run (CONTRACT section 8), on either wire.

Every event is one object. The stream always ends with exactly one terminal event: `result` or
`error`. Anything emitted after the terminal event is dropped. `lines()` serves the NDJSON wire
of `POST /v1/execute` (one JSON object per line, artifacts base64-encoded); `events()` serves the
gRPC stream, which turns each object into its message.
"""

from __future__ import annotations

import asyncio
import base64
import json
import mimetypes
import re
from collections.abc import AsyncIterator
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any, Literal

LogLevel = Literal["debug", "info", "warning", "error"]
# Kinds the runtime reports: its agents' turns, tool calls, spawns and the policy decisions
# that refused one of them. Data reads, egress and approvals are recorded by the backend.
SpanKind = Literal["llm_call", "tool_call", "spawn", "policy"]
SpanStatus = Literal["ok", "error"]
# What a payload holds: what a model turn was given and answered, what a tool call got and gave.
PayloadKind = Literal["prompt", "completion", "tool_input", "tool_output"]
# Most characters of one payload sent up; longer content is cut and says so.
PAYLOAD_MAX_CHARS = 200_000

DOCX_MIME = "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
_EXTRA_MIME = {
    ".docx": DOCX_MIME,
    ".xlsx": "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ".pptx": "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ".md": "text/markdown",
    ".py": "text/x-python",
    ".rs": "text/x-rust",
    ".ts": "text/typescript",
    ".tsx": "text/typescript",
    ".toml": "application/toml",
    ".yaml": "application/yaml",
    ".yml": "application/yaml",
    ".json": "application/json",
    ".csv": "text/csv",
    ".txt": "text/plain",
}
_SAFE_SEGMENT = re.compile(r"[^A-Za-z0-9._ -]+")
# Events only the gRPC wire carries: they are answered with a command, which NDJSON has no
# way to send. `lines()` leaves them out.
STREAM_ONLY_EVENTS = frozenset({"approval_request", "tool_request"})


class EventStream:
    """A single-consumer queue of events with a guaranteed single terminal event."""

    def __init__(self) -> None:
        self._queue: asyncio.Queue[dict[str, Any] | None] = asyncio.Queue()
        self._terminated = False

    @property
    def terminated(self) -> bool:
        return self._terminated

    # --- producers --------------------------------------------------------------------------
    def log(self, level: LogLevel, message: str) -> None:
        self._emit({"type": "log", "level": level, "message": message})

    def delta(self, text: str) -> None:
        if text:
            self._emit({"type": "delta", "text": text})

    def tokens(self, input_tokens: int, output_tokens: int, cached_tokens: int = 0) -> None:
        """Token usage of one LLM call (incremental; `result` carries the run totals)."""
        self._emit(
            {
                "type": "tokens",
                "input": input_tokens,
                "output": output_tokens,
                "cached": cached_tokens,
            }
        )

    def spawn(self, name: str, role: str) -> None:
        self._emit({"type": "spawn", "agent": {"name": name, "role": role}})

    def span(
        self,
        *,
        span_id: str,
        parent_id: str | None,
        kind: SpanKind,
        name: str,
        agent: str,
        started_at: str,
        ended_at: str | None,
        status: SpanStatus,
        attributes: dict[str, Any],
    ) -> None:
        """One action of an agent. `attributes` hold digests, sizes and counts, never arguments."""
        self._emit(
            {
                "type": "span",
                "id": span_id,
                "parent_id": parent_id,
                "kind": kind,
                "name": name,
                "agent": agent,
                "started_at": started_at,
                "ended_at": ended_at,
                "status": status,
                "attributes": attributes,
            }
        )

    def artifact(self, path: str, mime: str, content: bytes) -> None:
        self._emit({"type": "artifact", "path": path, "mime": mime, "content": content})

    def payload(self, *, span_id: str, kind: PayloadKind, content: str) -> None:
        """What a span said, sent only while the token's `recording` claim asks for the kind;
        the backend scrubs, redacts and seals it (CONTRACT section 4, `Recording`)."""
        self._emit({"type": "payload", "span_id": span_id, "kind": kind, "content": clip(content)})

    def approval_request(
        self,
        *,
        approval_id: str,
        tool: str,
        rule: str,
        reason: str,
        args_sha256: str,
        span_id: str,
        agent: str,
        turn_span_id: str | None,
    ) -> None:
        """A held tool call asks for its decision (the gRPC wire; the policy span is the record)."""
        self._emit(
            {
                "type": "approval_request",
                "approval_id": approval_id,
                "tool": tool,
                "rule": rule,
                "reason": reason,
                "args_sha256": args_sha256,
                "span_id": span_id,
                "agent": agent,
                "turn_span_id": turn_span_id,
            }
        )

    def tool_request(
        self,
        *,
        request_id: str,
        tool: str,
        arguments: dict[str, Any],
        agent: str,
        turn_span_id: str | None,
    ) -> None:
        """A gateway tool call for the backend to perform (the gRPC wire only)."""
        self._emit(
            {
                "type": "tool_request",
                "request_id": request_id,
                "tool": tool,
                "arguments": arguments,
                "agent": agent,
                "turn_span_id": turn_span_id,
            }
        )

    def result(self, output: str, tokens_in: int, tokens_out: int, tokens_cached: int = 0) -> None:
        self._terminal(
            {
                "type": "result",
                "output": output,
                "tokens_in": tokens_in,
                "tokens_out": tokens_out,
                "tokens_cached": tokens_cached,
            }
        )

    def error(self, message: str, *, retryable: bool) -> None:
        self._terminal({"type": "error", "message": message, "retryable": retryable})

    # --- consumers --------------------------------------------------------------------------
    async def events(self) -> AsyncIterator[dict[str, Any]]:
        """Every event as emitted, the terminal one last."""
        while True:
            event = await self._queue.get()
            if event is None:
                return
            yield event

    async def lines(self) -> AsyncIterator[bytes]:
        """The NDJSON wire: events the backend reads line by line."""
        async for event in self.events():
            if event["type"] in STREAM_ONLY_EVENTS:
                continue
            yield _encode(event)

    # --- internals --------------------------------------------------------------------------
    def _emit(self, event: dict[str, Any]) -> None:
        if not self._terminated:
            self._queue.put_nowait(event)

    def _terminal(self, event: dict[str, Any]) -> None:
        if self._terminated:
            return
        self._queue.put_nowait(event)
        self._terminated = True
        self._queue.put_nowait(None)


def clip(content: str) -> str:
    """`content` cut at `PAYLOAD_MAX_CHARS`, with a marker."""
    if len(content) <= PAYLOAD_MAX_CHARS:
        return content
    return content[:PAYLOAD_MAX_CHARS] + f"\n\n[… cut: {len(content)} characters in all]"


def _encode(event: dict[str, Any]) -> bytes:
    if event["type"] == "artifact":
        event = {
            "type": "artifact",
            "path": event["path"],
            "mime": event["mime"],
            "content_b64": base64.b64encode(event["content"]).decode("ascii"),
        }
    return json.dumps(event, ensure_ascii=False, separators=(",", ":")).encode("utf-8") + b"\n"


# --- artifacts ------------------------------------------------------------------------------


@dataclass(frozen=True, slots=True)
class Artifact:
    path: str
    mime: str
    content: bytes


def guess_mime(path: str) -> str:
    suffix = PurePosixPath(path).suffix.lower()
    if suffix in _EXTRA_MIME:
        return _EXTRA_MIME[suffix]
    guessed, _ = mimetypes.guess_type(path, strict=False)
    return guessed or "application/octet-stream"


def sanitize_relative_path(path: str) -> str | None:
    """Normalise a workspace-relative path for the wire; `None` if it cannot be made safe."""
    parts = []
    for raw in PurePosixPath(path.replace("\\", "/")).parts:
        if raw in ("", ".", "/"):
            continue
        if raw == "..":
            return None
        cleaned = _SAFE_SEGMENT.sub("_", raw).strip(" .")
        if not cleaned:
            return None
        parts.append(cleaned[:128])
    if not parts:
        return None
    return "/".join(parts)


def collect_artifacts(
    workspace: Path, *, max_file_bytes: int, max_count: int
) -> tuple[list[Artifact], list[str]]:
    """Return every regular file in `workspace` (sorted) plus human-readable skip warnings.

    Symlinks are never followed, oversized files are skipped, and paths are sanitised.
    """
    artifacts: list[Artifact] = []
    warnings: list[str] = []
    root = workspace.resolve()
    for file in sorted(root.rglob("*")):
        if file.is_symlink() or not file.is_file():
            continue
        relative = file.relative_to(root).as_posix()
        safe = sanitize_relative_path(relative)
        if safe is None:
            warnings.append(f"skipped artifact with unsafe path: {relative!r}")
            continue
        size = file.stat().st_size
        if size > max_file_bytes:
            warnings.append(f"skipped {safe}: {size} bytes exceeds the {max_file_bytes} byte limit")
            continue
        if len(artifacts) >= max_count:
            warnings.append(f"skipped {safe}: more than {max_count} artifacts")
            continue
        artifacts.append(Artifact(path=safe, mime=guess_mime(safe), content=file.read_bytes()))
    return artifacts, warnings
