"""Tools available to agents. All file access is confined to the run's workspace."""

from __future__ import annotations

from collections.abc import Sequence
from typing import Any

from .agent_tools import FINISH, SPAWN_SUBAGENT
from .base import Tool, ToolContext, ToolError, strict_schema
from .docx_tool import MAKE_DOCX, render_docx
from .files import LIST_FILES, READ_FILE, WRITE_FILE
from .gateway import gateway_tool
from .python_exec import RUN_PYTHON
from .workspace import PathError, Workspace, safe_path

# Names the built-in tools keep for themselves; a gateway tool may not shadow one.
BUILTIN_NAMES = frozenset(
    t.name
    for t in (WRITE_FILE, READ_FILE, LIST_FILES, MAKE_DOCX, RUN_PYTHON, SPAWN_SUBAGENT, FINISH)
)


def build_toolset(
    *,
    can_spawn: bool,
    allow_code_exec: bool,
    gateway_tools: Sequence[tuple[str, str, dict[str, Any]]] = (),
) -> dict[str, Tool[Any]]:
    """The tools one agent may use, keyed by name. `gateway_tools` are `(name, description,
    input schema)` of the tools the backend performs (`fetch_url`, `mcp:<server>/<tool>`)."""
    tools: list[Tool[Any]] = [WRITE_FILE, READ_FILE, LIST_FILES, MAKE_DOCX]
    if allow_code_exec:
        tools.append(RUN_PYTHON)
    if can_spawn:
        tools.append(SPAWN_SUBAGENT)
    for name, description, schema in gateway_tools:
        if name in BUILTIN_NAMES or not name:
            continue
        tools.append(gateway_tool(name, description, schema))
    tools.append(FINISH)
    return {tool.name: tool for tool in tools}


__all__ = [
    "PathError",
    "Tool",
    "ToolContext",
    "ToolError",
    "Workspace",
    "build_toolset",
    "render_docx",
    "safe_path",
    "strict_schema",
]
