"""Tools available to agents. All file access is confined to the run's workspace."""

from __future__ import annotations

from typing import Any

from .agent_tools import FINISH, SPAWN_SUBAGENT
from .base import Tool, ToolContext, ToolError, strict_schema
from .docx_tool import MAKE_DOCX, render_docx
from .files import LIST_FILES, READ_FILE, WRITE_FILE
from .python_exec import RUN_PYTHON
from .workspace import PathError, Workspace, safe_path


def build_toolset(*, can_spawn: bool, allow_code_exec: bool) -> dict[str, Tool[Any]]:
    """The tools one agent may use, keyed by name."""
    tools: list[Tool[Any]] = [WRITE_FILE, READ_FILE, LIST_FILES, MAKE_DOCX]
    if allow_code_exec:
        tools.append(RUN_PYTHON)
    if can_spawn:
        tools.append(SPAWN_SUBAGENT)
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
