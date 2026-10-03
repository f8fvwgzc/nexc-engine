"""Executes one `/v1/execute` request: workspace -> root agent -> artifacts -> terminal event."""

from __future__ import annotations

import asyncio
import logging
import re
import shutil
import tempfile
import traceback
from collections.abc import Callable
from pathlib import Path

from .agents import Agent, AgentFailure, AgentRegistry, Budget, Census, Meter, RunScope
from .api.schemas import ExecuteRequest
from .config import Settings
from .llm import DemoBrief, LLMError, LLMProvider, build_provider, redact
from .prompts import Upstream, build_task_prompt
from .streaming import EventStream, collect_artifacts
from .tools import Workspace
from .tools.docx_tool import markdown_to_docx_args, render_docx

log = logging.getLogger(__name__)

ProviderFactory = Callable[[ExecuteRequest, Settings], LLMProvider]


def default_provider_factory(request: ExecuteRequest, settings: Settings) -> LLMProvider:
    """Build the provider named in the request.

    Model precedence: `llm.model` (the backend's resolved choice) > `agent.model` > default.
    """
    llm = request.llm
    brief = DemoBrief(
        title=request.task.title,
        kind=request.task.kind,
        content=request.task.content,
        goal=request.context.goal,
        role=request.agent.role,
        upstream_titles=tuple(u.title for u in request.context.upstream if u.title),
        upstream_outputs=tuple(u.output for u in request.context.upstream if u.title),
    )
    return build_provider(
        provider=llm.provider,
        model=llm.model or request.agent.model or settings.nexc_llm_model,
        api_key=llm.api_key.get_secret_value() if llm.api_key else None,
        base_url=llm.base_url,
        brief=brief,
        max_retries=settings.runtime_llm_max_retries,
        timeout_s=float(min(request.limits.timeout_s, settings.runtime_timeout_cap_s)),
        claude_bin=settings.runtime_claude_bin,
    )


async def execute(
    request: ExecuteRequest,
    *,
    settings: Settings,
    census: Census,
    events: EventStream,
    provider_factory: ProviderFactory = default_provider_factory,
) -> None:
    """Run the request to completion. Always ends the stream with exactly one terminal event."""
    secret = request.llm.api_key.get_secret_value() if request.llm.api_key else None
    timeout = min(request.limits.timeout_s, settings.runtime_timeout_cap_s)
    registry = AgentRegistry(census)
    provider: LLMProvider | None = None
    workspace_dir = _make_workspace(settings.runtime_workspace)
    census.runs_active += 1
    try:
        async with asyncio.timeout(timeout):
            provider = provider_factory(request, settings)
            await _run(
                request=request,
                settings=settings,
                registry=registry,
                provider=provider,
                workspace_dir=workspace_dir,
                events=events,
            )
    except TimeoutError:
        events.error(f"run exceeded its time limit of {timeout}s", retryable=False)
    except LLMError as exc:
        events.error(redact(exc.message, secret), retryable=exc.retryable)
    except AgentFailure as exc:
        events.error(redact(str(exc), secret), retryable=exc.retryable)
    except asyncio.CancelledError:
        events.error("run cancelled", retryable=True)
        raise
    except Exception as exc:
        trace = redact("".join(traceback.format_exception(exc)), secret)
        log.error("unexpected failure in run %s:\n%s", request.run_id, trace)
        events.error("internal runtime error", retryable=True)
    finally:
        census.runs_active -= 1
        registry.retire_all()
        if provider is not None:
            await provider.aclose()
        if settings.runtime_keep_workspaces:
            log.info("keeping workspace %s for run %s", workspace_dir, request.run_id)
        else:
            shutil.rmtree(workspace_dir, ignore_errors=True)


async def _run(
    *,
    request: ExecuteRequest,
    settings: Settings,
    registry: AgentRegistry,
    provider: LLMProvider,
    workspace_dir: Path,
    events: EventStream,
) -> None:
    allow_code_exec = settings.runtime_allow_code_exec and request.limits.allow_code_exec
    if request.limits.allow_code_exec and not settings.runtime_allow_code_exec:
        events.log("warning", "code execution requested but disabled on this runtime")
    scope = RunScope(
        provider=provider,
        registry=registry,
        workspace=Workspace(
            workspace_dir,
            max_file_bytes=settings.runtime_max_file_bytes,
            max_total_bytes=settings.runtime_max_workspace_bytes,
        ),
        events=events,
        settings=settings,
        meter=Meter(),
        kind=request.task.kind,
        max_turns=min(request.limits.max_turns, settings.runtime_max_turns_cap),
        allow_code_exec=allow_code_exec,
    )
    spec = request.agent
    record = registry.birth(spec.name, spec.role)
    events.log(
        "info",
        f"agent {spec.name!r} ({spec.role}) born on {provider.name}/{provider.model} "
        f"with a budget of {spec.budget_tokens} tokens",
    )
    root = Agent(
        scope=scope,
        record=record,
        budget=Budget(spec.budget_tokens),
        system_prompt=spec.system_prompt,
        stream_text=True,
    )
    prompt = build_task_prompt(
        title=request.task.title,
        content=request.task.content,
        goal=request.context.goal,
        upstream=[Upstream(u.title or u.node_id, u.output) for u in request.context.upstream],
        memories=request.context.memories,
    )
    output = await root.run(prompt)
    _ensure_document(request, scope.workspace, output, events)

    artifacts, warnings = collect_artifacts(
        workspace_dir,
        max_file_bytes=settings.runtime_max_file_bytes,
        max_count=settings.runtime_max_artifacts,
    )
    for warning in warnings:
        events.log("warning", warning)
    for artifact in artifacts:
        events.artifact(artifact.path, artifact.mime, artifact.content)
    events.result(output, scope.meter.tokens_in, scope.meter.tokens_out)


DOCUMENT_KINDS = ("document", "output")


def _ensure_document(
    request: ExecuteRequest, workspace: Workspace, output: str, events: EventStream
) -> None:
    """Document and output nodes always deliver a .docx: when the agent answered in text without
    calling make_docx, its final Markdown answer is converted."""
    if request.task.kind not in DOCUMENT_KINDS or not output.strip():
        return
    if any(p.suffix.lower() == ".docx" for p in workspace.root.rglob("*") if p.is_file()):
        return
    filename = f"{_slug(request.task.title)}.docx"
    args = markdown_to_docx_args(request.task.title, output, filename)
    workspace.write_bytes(filename, render_docx(args))
    events.log("info", f"no .docx was produced; converted the final answer into {filename}")


def _slug(text: str) -> str:
    slug = re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")[:60].strip("-")
    return slug or "deliverable"


def _make_workspace(root: Path) -> Path:
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    return Path(tempfile.mkdtemp(prefix="run-", dir=root))
