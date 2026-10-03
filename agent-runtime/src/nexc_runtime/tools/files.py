"""Workspace file tools: write_file, read_file, list_files."""

from __future__ import annotations

from pydantic import Field

from .base import Tool, ToolArgs, ToolContext, ToolError

READ_LIMIT_CHARS = 200_000
LIST_LIMIT = 500


class WriteFileArgs(ToolArgs):
    path: str = Field(description="Relative path inside the workspace, e.g. 'src/app.py'.")
    content: str = Field(description="Complete UTF-8 file contents (replaces any existing file).")


class ReadFileArgs(ToolArgs):
    path: str = Field(description="Relative path inside the workspace.")


class ListFilesArgs(ToolArgs):
    path: str = Field(description="Relative directory to list recursively; use '.' for the root.")


async def write_file(ctx: ToolContext, args: WriteFileArgs) -> str:
    data = args.content.encode("utf-8")
    target = ctx.workspace.write_bytes(args.path, data)
    return f"wrote {ctx.workspace.relative(target)} ({len(data)} bytes)"


async def read_file(ctx: ToolContext, args: ReadFileArgs) -> str:
    target = ctx.workspace.path(args.path)
    if not target.is_file():
        raise ToolError(f"no such file: {args.path}")
    text = target.read_bytes().decode("utf-8", errors="replace")
    if len(text) > READ_LIMIT_CHARS:
        return text[:READ_LIMIT_CHARS] + f"\n\n[truncated: {len(text)} characters in total]"
    return text


async def list_files(ctx: ToolContext, args: ListFilesArgs) -> str:
    base = ctx.workspace.root if args.path.strip() in (".", "./") else ctx.workspace.path(args.path)
    if not base.is_dir():
        raise ToolError(f"no such directory: {args.path}")
    entries = []
    for item in sorted(base.rglob("*")):
        if item.is_symlink() or not item.is_file():
            continue
        entries.append(f"{ctx.workspace.relative(item)}\t{item.stat().st_size} bytes")
        if len(entries) >= LIST_LIMIT:
            entries.append(f"[listing truncated at {LIST_LIMIT} files]")
            break
    return "\n".join(entries) if entries else "(empty)"


WRITE_FILE = Tool(
    name="write_file",
    description=(
        "Create or overwrite a text file in your private workspace. Every file in the workspace "
        "is delivered to the user as an artifact when you finish."
    ),
    args=WriteFileArgs,
    handler=write_file,
)
READ_FILE = Tool(
    name="read_file",
    description="Read a UTF-8 text file from your workspace.",
    args=ReadFileArgs,
    handler=read_file,
)
LIST_FILES = Tool(
    name="list_files",
    description="List the files in your workspace (recursively) with their sizes.",
    args=ListFilesArgs,
    handler=list_files,
)
