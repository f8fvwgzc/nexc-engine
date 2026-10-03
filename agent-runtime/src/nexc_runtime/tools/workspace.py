"""Per-run workspace and the `safe_path` resolver every file tool goes through."""

from __future__ import annotations

import os
import re
from pathlib import Path, PurePosixPath

MAX_PATH_LENGTH = 512
_DRIVE = re.compile(r"^[A-Za-z]:")


class PathError(ValueError):
    """The requested path is not allowed (absolute, traversal, symlink escape...)."""


def safe_path(root: Path, relative: str) -> Path:
    """Resolve `relative` inside `root`, rejecting anything that could escape it.

    Rejected: empty paths, NUL bytes, backslashes, absolute paths and drive letters,
    any `..` component, and paths whose resolution (following symlinks) leaves `root`.
    """
    if not isinstance(relative, str) or not relative.strip():
        raise PathError("path must be a non-empty relative path")
    if len(relative) > MAX_PATH_LENGTH:
        raise PathError(f"path longer than {MAX_PATH_LENGTH} characters")
    if "\x00" in relative or "\\" in relative:
        raise PathError("path contains forbidden characters")
    pure = PurePosixPath(relative)
    if pure.is_absolute() or _DRIVE.match(relative):
        raise PathError("absolute paths are not allowed; use a path relative to the workspace")
    if any(part == ".." for part in pure.parts):
        raise PathError("'..' is not allowed in paths")

    # Canonicalise both sides (following symlinks) and require the result to stay under root.
    base = os.path.realpath(root)
    resolved = os.path.realpath(os.path.join(base, relative))
    if resolved != base and not resolved.startswith(base + os.sep):
        raise PathError("path escapes the workspace")
    return Path(resolved)


class Workspace:
    """A scratch directory owned by one run, with size accounting."""

    def __init__(self, root: Path, *, max_file_bytes: int, max_total_bytes: int) -> None:
        self.root = root.resolve()
        self.max_file_bytes = max_file_bytes
        self.max_total_bytes = max_total_bytes

    def path(self, relative: str) -> Path:
        return safe_path(self.root, relative)

    def relative(self, absolute: Path) -> str:
        return absolute.relative_to(self.root).as_posix()

    def usage_bytes(self) -> int:
        return sum(
            p.stat().st_size for p in self.root.rglob("*") if p.is_file() and not p.is_symlink()
        )

    def write_bytes(self, relative: str, data: bytes) -> Path:
        """Create or replace a file, enforcing per-file and total quotas; never follows symlinks."""
        target = self.path(relative)
        if target == self.root:
            raise PathError("path must name a file")
        if len(data) > self.max_file_bytes:
            raise PathError(f"file larger than {self.max_file_bytes} bytes")
        existing = target.stat().st_size if target.is_file() else 0
        if self.usage_bytes() - existing + len(data) > self.max_total_bytes:
            raise PathError(f"workspace quota of {self.max_total_bytes} bytes exceeded")
        target.parent.mkdir(parents=True, exist_ok=True)
        flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC | getattr(os, "O_NOFOLLOW", 0)
        fd = os.open(target, flags, 0o600)
        with os.fdopen(fd, "wb") as handle:
            handle.write(data)
        return target
