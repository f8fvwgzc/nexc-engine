"""`AgentRuntime` served with grpclib: `Execute`, one bidirectional stream per node attempt,
and `Health`. The stream's first command is the execute request (validated as the NDJSON body
is); the run then goes through the same runner, `EventStream` and approval registry, its events
sent up as messages, while a reader takes the backend's commands: `ack`, `cancel`,
`approval_decision`, `policy_update`, `tool_result`, `cancel_agent`."""

from __future__ import annotations

import asyncio
import contextlib
import hmac
import logging
from collections.abc import Awaitable, Callable
from typing import Any

from grpclib import GRPCError, Status
from grpclib.server import Server, Stream
from pydantic import ValidationError

from .. import __version__
from ..agents import Census
from ..api.schemas import ExecuteRequest
from ..approvals import ApprovalOutcome
from ..config import Settings
from ..control import RunControl, ToolOutcome
from ..keys import KeyRing
from ..policy import TokenError, token_kid, verify_token
from ..runner import ProviderFactory, default_provider_factory, execute
from ..streaming import EventStream
from . import runtime_pb2 as pb
from .convert import event_to_message, execute_to_dict
from .runtime_grpc import AgentRuntimeBase

log = logging.getLogger(__name__)

ExecuteStream = Stream[pb.BackendCommand, pb.RuntimeEvent]
HealthStream = Stream[pb.HealthRequest, pb.HealthReply]


class AgentRuntimeService(AgentRuntimeBase):
    def __init__(
        self,
        settings: Settings,
        census: Census,
        *,
        keys: KeyRing | None = None,
        provider_factory: ProviderFactory = default_provider_factory,
    ) -> None:
        self.settings = settings
        self.census = census
        self.keys = keys if keys is not None else settings.key_ring()
        self.provider_factory = provider_factory

    # --- auth -------------------------------------------------------------------------------
    def _authenticate(self, stream: Stream[Any, Any]) -> None:
        """Constant-time check of `authorization: Bearer <NEXC_RUNTIME_TOKEN>`."""
        expected = self.settings.nexc_runtime_token.get_secret_value().encode("utf-8")
        header = ""
        if stream.metadata is not None:
            value = stream.metadata.get("authorization", "")
            header = value if isinstance(value, str) else ""
        scheme, _, presented = header.partition(" ")
        ok = scheme.lower() == "bearer" and hmac.compare_digest(
            presented.strip().encode("utf-8"), expected
        )
        if not ok:
            raise GRPCError(Status.UNAUTHENTICATED, "invalid or missing bearer token")

    # --- rpcs -------------------------------------------------------------------------------
    async def Health(self, stream: HealthStream) -> None:
        await stream.recv_message()
        await stream.send_message(
            pb.HealthReply(
                status="ok",
                version=__version__,
                agents_loaded=self.census.agents_live,
                runs_active=self.census.runs_active,
            )
        )

    async def Execute(self, stream: ExecuteStream) -> None:
        self._authenticate(stream)
        first = await stream.recv_message()
        if first is None or first.WhichOneof("command") != "execute":
            raise GRPCError(
                Status.INVALID_ARGUMENT, "the first command must be the execute request"
            )
        try:
            request = ExecuteRequest.model_validate(execute_to_dict(first.execute))
        except ValidationError as exc:
            # Never echo the input: it carries the per-request API key.
            problems = "; ".join(
                f"{'.'.join(str(p) for p in err['loc']) or 'request'}: {err['msg']}"
                for err in exc.errors(include_input=False, include_url=False)
            )
            raise GRPCError(Status.INVALID_ARGUMENT, f"invalid request: {problems}") from None
        if self.census.runs_active >= self.settings.runtime_max_concurrent_runs:
            raise GRPCError(Status.RESOURCE_EXHAUSTED, "runtime is at capacity")
        events = EventStream()
        control = RunControl()
        runner = asyncio.create_task(
            execute(
                request,
                settings=self.settings,
                census=self.census,
                events=events,
                keys=self.keys,
                provider_factory=self.provider_factory,
                approvals=control.approvals,
                control=control,
            )
        )
        reader = asyncio.create_task(self._read_commands(stream, request, control, runner))
        try:
            async for event in events.events():
                await stream.send_message(event_to_message(event))
            with contextlib.suppress(asyncio.CancelledError):
                await runner
        finally:
            reader.cancel()
            if not runner.done():  # the backend went away: stop the agents
                runner.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await runner
            with contextlib.suppress(asyncio.CancelledError):
                await reader

    async def _read_commands(
        self,
        stream: ExecuteStream,
        request: ExecuteRequest,
        control: RunControl,
        runner: asyncio.Task[None],
    ) -> None:
        """Takes the backend's commands until it closes its side of the stream."""
        try:
            async for command in stream:
                kind = command.WhichOneof("command")
                if kind == "cancel":
                    control.cancel_reason = command.cancel.reason or None
                    runner.cancel()
                elif kind == "approval_decision":
                    decision = command.approval_decision
                    control.approvals.resolve(
                        decision.approval_id,
                        ApprovalOutcome(decision.decision, decision.reason, decision.decided_by),
                    )
                elif kind == "tool_result":
                    result = command.tool_result
                    control.tools.resolve(
                        result.request_id, ToolOutcome(result.output, result.is_error)
                    )
                elif kind == "ack":
                    control.tools.ack(command.ack.request_id)
                elif kind == "policy_update":
                    await self._update_policy(request, control, command.policy_update.token)
                elif kind == "cancel_agent":
                    stop = command.cancel_agent
                    if not control.cancel_agent(stop.spawn_span_id, stop.reason or None):
                        log.info(
                            "cancel_agent for run %s names no live sub-agent (%s)",
                            request.run_id,
                            stop.spawn_span_id,
                        )
        except asyncio.CancelledError:
            raise
        except Exception as exc:  # a broken stream ends the run, never the server
            log.warning(
                "the command stream of run %s broke: %s", request.run_id, type(exc).__name__
            )
        # The backend half-closed its side: it has nothing more to say, and the run goes on to
        # its terminal event. A backend that goes away resets the stream instead, which
        # cancels the handler and, with it, the run.

    async def _update_policy(
        self, request: ExecuteRequest, control: RunControl, token: str
    ) -> None:
        """A fresh token for the same run and node replaces the rules; a bad one is ignored."""
        await self.keys.ensure(token_kid(token))
        try:
            fresh = verify_token(
                token,
                self.keys,
                run_id=request.run_id,
                node_id=request.node_id,
            )
        except TokenError as exc:
            log.warning("policy update for run %s refused: %s", request.run_id, exc)
            return
        control.updated_token = fresh


async def serve(
    settings: Settings,
    census: Census,
    *,
    host: str | None = None,
    port: int | None = None,
    provider_factory: ProviderFactory = default_provider_factory,
    keys: KeyRing | None = None,
) -> Server:
    """Starts the gRPC server; the caller closes it. `keys` is the HTTP app's ring, so both
    wires learn a rotated key once."""
    server = Server(
        [AgentRuntimeService(settings, census, keys=keys, provider_factory=provider_factory)]
    )
    await server.start(
        host if host is not None else settings.runtime_host,
        port if port is not None else settings.runtime_grpc_port,
    )
    return server


ServeFn = Callable[[], Awaitable[Server]]
