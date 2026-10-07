"""NDJSON event stream for `POST /v1/execute` (CONTRACT section 8).

Every line is one JSON object. The stream always ends with exactly one terminal event:
`result` or `error`. Anything emitted after the terminal event is dropped.
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


class EventStream:
    """A single-consumer queue of NDJSON lines with a guaranteed single terminal event."""

    def __init__(self) -> None:
        self._queue: asyncio.Queue[bytes | None] = asyncio.Queue()
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

    def artifact(self, path: str, mime: str, content: bytes) -> None:
        self._emit(
            {
                "type": "artifact",
                "path": path,
                "mime": mime,
                "content_b64": base64.b64encode(content).decode("ascii"),
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

    # --- consumer ---------------------------------------------------------------------------
    async def lines(self) -> AsyncIterator[bytes]:
        while True:
            line = await self._queue.get()
            if line is None:
                return
            yield line

    # --- internals --------------------------------------------------------------------------
    def _emit(self, event: dict[str, Any]) -> None:
        if not self._terminated:
            self._queue.put_nowait(_encode(event))

    def _terminal(self, event: dict[str, Any]) -> None:
        if self._terminated:
            return
        self._queue.put_nowait(_encode(event))
        self._terminated = True
        self._queue.put_nowait(None)


def _encode(event: dict[str, Any]) -> bytes:
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
