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
from .approvals import ApprovalRegistry
from .config import Settings
from .control import RunControl
from .evidence import Sources, split_head
from .keys import KeyRing
from .llm import DemoBrief, LLMError, LLMProvider, build_provider, redact
from .llm.demo import first_sentence
from .policy import (
    AgentToken,
    TokenError,
    UnknownKeyError,
    decide_model_with,
    decide_tool_with,
    token_kid,
    verify_token,
)
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
    documents: list[tuple[str, str, str]] = []
    for passage in request.context.documents:
        head, _, body = passage.strip().partition("\n")
        cid, source = split_head(head)
        if cid:
            documents.append((cid, source.strip(), first_sentence(body)))
    memories: list[tuple[str, str]] = []
    for memory in request.context.memories:
        cid, text = split_head(memory)
        if cid:
            memories.append((cid, first_sentence(text)))
    upstream = [u for u in request.context.upstream if u.title]
    brief = DemoBrief(
        title=request.task.title,
        kind=request.task.kind,
        content=request.task.content,
        goal=request.context.goal,
        role=request.agent.role,
        upstream_titles=tuple(u.title for u in upstream),
        upstream_outputs=tuple(u.output for u in upstream),
        upstream_ids=tuple(u.node_id for u in upstream),
        documents=tuple(documents),
        memories=tuple(memories),
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
    keys: KeyRing,
    provider_factory: ProviderFactory = default_provider_factory,
    approvals: ApprovalRegistry | None = None,
    control: RunControl | None = None,
) -> None:
    """Run the request to completion. Always ends the stream with exactly one terminal event.

    `control` is the backend's hold on the run over the gRPC stream (gateway results, token
    updates, the cancel); the NDJSON wire runs without one."""
    # Nothing starts without the backend's token: no workspace, no provider, no agent. Its key
    # is the backend's public one, fetched first when the token names one not seen yet.
    await keys.ensure(token_kid(request.token))
    try:
        token = verify_token(
            request.token,
            keys,
            run_id=request.run_id,
            node_id=request.node_id,
        )
    except TokenError as exc:
        # A key not known yet may be one the backend just published while its key set could not
        # be fetched: the backend may try again. Anything else is final.
        events.error(f"agent token refused: {exc}", retryable=isinstance(exc, UnknownKeyError))
        return
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
                token=token,
                approvals=approvals,
                control=control,
            )
    except TimeoutError:
        events.error(f"run exceeded its time limit of {timeout}s", retryable=False)
    except LLMError as exc:
        events.error(redact(exc.message, secret), retryable=exc.retryable)
    except AgentFailure as exc:
        events.error(redact(str(exc), secret), retryable=exc.retryable)
    except asyncio.CancelledError:
        reason = control.cancel_reason if control is not None else None
        events.error(f"run cancelled: {reason}" if reason else "run cancelled", retryable=True)
        raise
    except Exception as exc:
        trace = redact("".join(traceback.format_exception(exc)), secret)
        log.error("unexpected failure in run %s:\n%s", _loggable(request.run_id), trace)
        events.error("internal runtime error", retryable=True)
    finally:
        census.runs_active -= 1
        registry.retire_all()
        if provider is not None:
            await provider.aclose()
        if settings.runtime_keep_workspaces:
            log.info("keeping workspace %s for run %s", workspace_dir, _loggable(request.run_id))
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
    token: AgentToken,
    approvals: ApprovalRegistry | None,
    control: RunControl | None = None,
) -> None:
    # The backend refuses a model outside the allow-list before dispatch; this is the same
    # rule applied to the model the provider really resolved.
    verdict = decide_model_with(token.capabilities, token.rules, provider.model)
    if not verdict.allowed:
        raise AgentFailure(f"policy: {verdict.reason}", retryable=False)
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
        kind_description=request.task.kind_description,
        produces_artifact=_produces_artifact(request),
        max_turns=min(request.limits.max_turns, settings.runtime_max_turns_cap),
        allow_code_exec=allow_code_exec,
        approvals=approvals,
        control=control,
        # Gateway tools are performed by the backend over the stream; none without one.
        gateway_tools=tuple((t.name, t.description, t.input_schema) for t in request.gateway_tools)
        if control is not None
        else (),
        sources=Sources.from_context(
            upstream=[(u.node_id, u.title) for u in request.context.upstream],
            memories=list(request.context.memories),
            documents=list(request.context.documents),
        ),
    )
    spec = request.agent
    # The policy's ceiling per run caps the budget whatever the agent has left.
    budget_tokens = spec.budget_tokens
    if token.rules.tokens_per_run is not None:
        budget_tokens = min(budget_tokens, token.rules.tokens_per_run)
    record = registry.birth(spec.name, spec.role, principal=token.principal)
    events.log(
        "info",
        f"agent {spec.name!r} ({spec.role}) born on {provider.name}/{provider.model} "
        f"with a budget of {budget_tokens} tokens, as {token.principal} "
        f"on behalf of {_human(token)} under policy version {token.policy_version}",
    )
    root = Agent(
        scope=scope,
        record=record,
        budget=Budget(budget_tokens),
        token=token,
        system_prompt=spec.system_prompt,
        stream_text=True,
    )
    prompt = build_task_prompt(
        title=request.task.title,
        content=request.task.content,
        goal=request.context.goal,
        upstream=[
            Upstream(title=u.title or u.node_id, output=u.output, node_id=u.node_id)
            for u in request.context.upstream
        ],
        memories=request.context.memories,
        documents=request.context.documents,
    )
    # An answer that cites ids names them (`finish` does this; an agent that gave up without
    # finishing is covered here).
    output = scope.sources.with_sources(await root.run(prompt))
    _ensure_document(request, scope.workspace, output, events, token)

    artifacts, warnings = collect_artifacts(
        workspace_dir,
        max_file_bytes=settings.runtime_max_file_bytes,
        max_count=settings.runtime_max_artifacts,
    )
    for warning in warnings:
        events.log("warning", warning)
    # A node that claims files but delivers none is the failure that hides best: say the count.
    if artifacts:
        names = ", ".join(a.path for a in artifacts[:8]) + (" …" if len(artifacts) > 8 else "")
        events.log("info", f"delivering {len(artifacts)} file(s): {names}")
    elif _produces_artifact(request) or "write_file" in output or "`" in output[:400]:
        events.log(
            "warning",
            "the agent wrote no files to the workspace; only its final answer is delivered",
        )
    for artifact in artifacts:
        events.artifact(artifact.path, artifact.mime, artifact.content)
    events.result(output, scope.meter.tokens_in, scope.meter.tokens_out, scope.meter.tokens_cached)


DOCUMENT_KINDS = ("document", "output")


def _loggable(value: str) -> str:
    """A request-supplied value made safe for log lines (no forged line breaks)."""
    return value.replace("\r", "\\r").replace("\n", "\\n")


def _ensure_document(
    request: ExecuteRequest,
    workspace: Workspace,
    output: str,
    events: EventStream,
    token: AgentToken,
) -> None:
    """Nodes whose type produces an artifact always deliver a .docx: when the agent answered in
    text without calling make_docx, its final Markdown answer is converted. Not for an agent
    whose policy forbids `make_docx` or holds it for an approval: the runtime makes nothing
    the agent may not."""
    if not _produces_artifact(request) or not output.strip():
        return
    if any(p.suffix.lower() == ".docx" for p in workspace.root.rglob("*") if p.is_file()):
        return
    decision = decide_tool_with(token.capabilities, token.rules, token.labels, "make_docx")
    if not decision.allowed:
        events.log(
            "warning",
            f"no .docx was produced and the agent's policy does not allow make_docx "
            f"({decision.rule}); only its final answer is delivered",
        )
        return
    filename = f"{_slug(request.task.title)}.docx"
    # The answer may claim a file that was never written; say what this file is instead.
    note = (
        "Note: the agent did not write the document it describes, so this file was made from "
        "its final answer. Run the task again for the full document."
    )
    lines = output.strip().splitlines()
    noted = (
        "\n".join([lines[0], "", note, *lines[1:]])
        if lines and lines[0].startswith("# ")
        else f"{note}\n\n{output}"
    )
    args = markdown_to_docx_args(request.task.title, noted, filename)
    workspace.write_bytes(filename, render_docx(args))
    events.log("warning", f"no .docx was produced; converted the final answer into {filename}")


def _produces_artifact(request: ExecuteRequest) -> bool:
    """Whether the node's type delivers a file, as the graph's ontology declares it."""
    declared = request.task.produces_artifact
    return request.task.kind in DOCUMENT_KINDS if declared is None else declared


def _slug(text: str) -> str:
    slug = re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")[:60].strip("-")
    return slug or "deliverable"


def _make_workspace(root: Path) -> Path:
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    return Path(tempfile.mkdtemp(prefix="run-", dir=root))


def _human(token: AgentToken) -> str:
    """`user <id> (<role>)`, as the backend's audit details name the human."""
    who = token.on_behalf_of
    if who is None:
        return "nobody"
    return f"user {who.user_id} ({who.workspace_role or 'no workspace'})"
