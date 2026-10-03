"""run_python: sandboxed Python execution, only registered when RUNTIME_ALLOW_CODE_EXEC=true.

Defence in depth (the container is the primary boundary):
* `python -I` (isolated mode: ignores PYTHON* env vars and the user site directory),
* cwd is the run's workspace, the environment is scrubbed (no API keys or tokens),
* rlimits on CPU time, address space, file size, process count and open files,
* a wall-clock timeout that kills the whole process group, and truncated output.
"""

from __future__ import annotations

import asyncio
import contextlib
import os
import resource
import signal
import sys
from collections.abc import Callable

from pydantic import Field

from .base import Tool, ToolArgs, ToolContext

OUTPUT_LIMIT = 16_000


class RunPythonArgs(ToolArgs):
    code: str = Field(
        description="Python 3 source to execute. The working directory is the workspace."
    )


def _limits(cpu_s: int, memory_bytes: int, fsize_bytes: int) -> Callable[[], None]:
    def apply() -> None:  # runs in the child between fork and exec
        def setlimit(kind: int, value: int) -> None:
            with contextlib.suppress(ValueError, OSError):  # not every limit exists on every OS
                resource.setrlimit(kind, (value, value))

        setlimit(resource.RLIMIT_CPU, cpu_s)
        setlimit(resource.RLIMIT_AS, memory_bytes)
        setlimit(resource.RLIMIT_FSIZE, fsize_bytes)
        setlimit(resource.RLIMIT_NPROC, 32)
        setlimit(resource.RLIMIT_NOFILE, 64)
        setlimit(resource.RLIMIT_CORE, 0)

    return apply


def _truncate(data: bytes) -> str:
    text = data.decode("utf-8", errors="replace")
    if len(text) > OUTPUT_LIMIT:
        return text[:OUTPUT_LIMIT] + f"\n[truncated: {len(text)} characters]"
    return text


async def run_python(ctx: ToolContext, args: RunPythonArgs) -> str:
    settings = ctx.settings
    workspace = str(ctx.workspace.root)
    env = {
        "PATH": "/usr/local/bin:/usr/bin:/bin",
        "HOME": workspace,
        "TMPDIR": workspace,
        "LANG": "C.UTF-8",
        "PYTHONDONTWRITEBYTECODE": "1",
        "PYTHONUNBUFFERED": "1",
    }
    timeout = settings.runtime_python_timeout_s
    process = await asyncio.create_subprocess_exec(
        sys.executable,
        "-I",
        "-",
        cwd=workspace,
        env=env,
        stdin=asyncio.subprocess.PIPE,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
        start_new_session=True,
        preexec_fn=_limits(
            cpu_s=timeout,
            memory_bytes=settings.runtime_python_memory_mb * 1024 * 1024,
            fsize_bytes=settings.runtime_max_file_bytes,
        ),
    )
    try:
        stdout, stderr = await asyncio.wait_for(
            process.communicate(args.code.encode("utf-8")), timeout=timeout
        )
    except TimeoutError:
        with contextlib.suppress(ProcessLookupError):
            os.killpg(process.pid, signal.SIGKILL)
        await process.wait()
        return f"exit: timeout after {timeout}s (process killed)"
    finally:
        if process.returncode is None:
            with contextlib.suppress(ProcessLookupError):
                os.killpg(process.pid, signal.SIGKILL)

    parts = [f"exit code: {process.returncode}"]
    if stdout:
        parts.append(f"stdout:\n{_truncate(stdout)}")
    if stderr:
        parts.append(f"stderr:\n{_truncate(stderr)}")
    return "\n".join(parts)


RUN_PYTHON = Tool(
    name="run_python",
    description=(
        "Execute a short Python 3 script in a sandbox whose working directory is your workspace "
        "(no network guarantees, strict CPU/memory/time limits). Returns exit code, stdout and "
        "stderr. Files the script writes into the workspace become artifacts."
    ),
    args=RunPythonArgs,
    handler=run_python,
)
