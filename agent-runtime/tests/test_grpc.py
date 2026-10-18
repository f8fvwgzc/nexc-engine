"""The gRPC wire (CONTRACT section 8): a full run with the demo provider, a cancel mid-run, the
cancel of one sub-agent while the run goes on, an approval round trip, a gateway tool round
trip, a policy update, a dropped stream, and the refusals (no token, a bad first command, an
invalid request)."""

from __future__ import annotations

import asyncio
import socket
from collections.abc import AsyncIterator, Callable
from typing import Any

import pytest
from google.protobuf import json_format
from grpclib import GRPCError, Status
from grpclib.client import Channel
from grpclib.server import Server

from nexc_runtime.agents import Census
from nexc_runtime.api.schemas import ExecuteRequest
from nexc_runtime.config import Settings
from nexc_runtime.llm import Conversation, ToolSpec, Turn
from nexc_runtime.llm.base import TextSink
from nexc_runtime.rpc import AgentRuntimeService
from nexc_runtime.rpc import runtime_pb2 as pb
from nexc_runtime.rpc.convert import execute_to_dict
from nexc_runtime.rpc.runtime_grpc import AgentRuntimeStub
from nexc_runtime.runner import default_provider_factory

from .conftest import AUTH, TOKEN, FakeProvider, Step, call, execute_body, mint_token

pytestmark = pytest.mark.asyncio


def to_request(body: dict[str, Any]) -> pb.ExecuteRequest:
    """The JSON body as the message the backend sends."""

    def clean(value: Any) -> Any:
        if isinstance(value, dict):
            return {k: clean(v) for k, v in value.items() if v is not None}
        if isinstance(value, list):
            return [clean(v) for v in value]
        return value

    message = pb.ExecuteRequest()
    json_format.ParseDict(clean(body), message, ignore_unknown_fields=True)
    return message


class SlowProvider(FakeProvider):
    """A model that never answers its first turn until told to."""

    def __init__(self, steps: list[Step]) -> None:
        super().__init__(steps)
        self.started = asyncio.Event()
        self.release = asyncio.Event()

    async def next_turn(
        self,
        conversation: Conversation,
        tools: list[ToolSpec],
        *,
        max_tokens: int,
        on_text: TextSink | None = None,
    ) -> Turn:
        self.started.set()
        await self.release.wait()
        return await super().next_turn(conversation, tools, max_tokens=max_tokens, on_text=on_text)


class BlockingProvider(FakeProvider):
    """A model whose `block_at`-th turn never answers until told to (or is cancelled)."""

    def __init__(self, steps: list[Step], *, block_at: int) -> None:
        super().__init__(steps)
        self.block_at = block_at
        self.calls = 0
        self.blocked = asyncio.Event()
        self.release = asyncio.Event()

    async def next_turn(
        self,
        conversation: Conversation,
        tools: list[ToolSpec],
        *,
        max_tokens: int,
        on_text: TextSink | None = None,
    ) -> Turn:
        self.calls += 1
        if self.calls == self.block_at:
            self.blocked.set()
            await self.release.wait()
        return await super().next_turn(conversation, tools, max_tokens=max_tokens, on_text=on_text)


class Running:
    def __init__(self, server: Server, port: int, census: Census) -> None:
        self.server = server
        self.port = port
        self.census = census

    def channel(self) -> Channel:
        return Channel("127.0.0.1", self.port)


@pytest.fixture
async def serve(
    make_settings: Callable[..., Settings],
) -> AsyncIterator[Callable[..., Any]]:
    """Starts an in-process gRPC server; `provider` scripts the model (None: the real factory)."""
    servers: list[Server] = []

    async def start(provider: FakeProvider | None = None, **overrides: Any) -> Running:
        settings = make_settings(**overrides)
        census = Census()
        factory = (
            default_provider_factory if provider is None else (lambda _req, _settings: provider)
        )
        service = AgentRuntimeService(settings, census, provider_factory=factory)
        server = Server([service])
        sock = socket.socket()
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
        await server.start(sock=sock)
        servers.append(server)
        return Running(server, port, census)

    yield start
    for server in servers:
        server.close()
        await server.wait_closed()


async def collect(stream: Any) -> list[pb.RuntimeEvent]:
    """Every event until the stream ends."""
    events: list[pb.RuntimeEvent] = []
    while True:
        event = await stream.recv_message()
        if event is None:
            return events
        events.append(event)


def kinds(events: list[pb.RuntimeEvent]) -> list[str]:
    return [e.WhichOneof("event") or "" for e in events]


async def test_a_full_run_over_grpc_with_the_demo_provider(serve: Callable[..., Any]) -> None:
    running = await serve()
    body = execute_body(llm={"provider": "demo", "api_key": None})
    async with running.channel() as channel:
        stub = AgentRuntimeStub(channel)
        async with stub.Execute.open(metadata=[("authorization", AUTH["Authorization"])]) as stream:
            await stream.send_message(pb.BackendCommand(execute=to_request(body)), end=True)
            events = await collect(stream)
    seen = kinds(events)
    assert seen[0] == "log", seen
    assert seen[-1] == "result", seen
    assert seen.count("result") + seen.count("error") == 1
    spans = [e.span for e in events if e.WhichOneof("event") == "span"]
    assert any(s.kind == "llm_call" for s in spans)
    assert any(s.kind == "tool_call" for s in spans)
    turn = next(s for s in spans if s.kind == "llm_call")
    assert turn.attributes["provider"] == "demo"
    assert "delta" in seen and "tokens" in seen
    artifacts = [e.artifact for e in events if e.WhichOneof("event") == "artifact"]
    assert artifacts and artifacts[0].path.endswith(".docx")
    assert artifacts[0].content.startswith(b"PK")
    result = events[-1].result
    assert result.output
    assert result.tokens_in > 0
    assert running.census.runs_active == 0


async def test_a_cancel_mid_run_ends_the_stream_with_one_error(serve: Callable[..., Any]) -> None:
    provider = SlowProvider([Step(text="never")])
    running = await serve(provider)
    async with running.channel() as channel:
        stub = AgentRuntimeStub(channel)
        async with stub.Execute.open(metadata=[("authorization", AUTH["Authorization"])]) as stream:
            await stream.send_message(pb.BackendCommand(execute=to_request(execute_body())))
            await asyncio.wait_for(provider.started.wait(), 5)
            await stream.send_message(
                pb.BackendCommand(cancel=pb.Cancel(reason="the run was cancelled")), end=True
            )
            events = await collect(stream)
    seen = kinds(events)
    assert seen[-1] == "error", seen
    error = events[-1].error
    assert error.retryable is True
    assert "cancelled" in error.message and "the run was cancelled" in error.message
    assert seen.count("error") == 1
    assert running.census.runs_active == 0


async def test_a_cancel_agent_stops_one_sub_agent_and_the_run_goes_on(
    serve: Callable[..., Any],
) -> None:
    # The root spawns a researcher whose first turn hangs; the operator stops the researcher;
    # the root sees the tool error and finishes.
    provider = BlockingProvider(
        [
            Step(calls=[call("spawn_subagent", role="researcher", task="Dig.")]),
            # The researcher's turn blocks before it takes a step; the root's second turn
            # takes this one.
            Step(calls=[call("finish", answer="went on without the researcher")]),
        ],
        block_at=2,
    )
    running = await serve(provider)
    # A plain task: `finish` needs no file in the workspace.
    body = execute_body(task={"kind": "task"})
    async with running.channel() as channel:
        stub = AgentRuntimeStub(channel)
        async with stub.Execute.open(metadata=[("authorization", AUTH["Authorization"])]) as stream:
            await stream.send_message(pb.BackendCommand(execute=to_request(body)))
            spawn = None
            while spawn is None:
                event = await asyncio.wait_for(stream.recv_message(), 5)
                assert event is not None
                if event.WhichOneof("event") == "span" and event.span.kind == "spawn":
                    spawn = event.span
            await asyncio.wait_for(provider.blocked.wait(), 5)
            assert running.census.agents_live == 2
            # A cancel naming no live sub-agent changes nothing.
            await stream.send_message(
                pb.BackendCommand(cancel_agent=pb.CancelAgent(spawn_span_id="nobody"))
            )
            await stream.send_message(
                pb.BackendCommand(
                    cancel_agent=pb.CancelAgent(spawn_span_id=spawn.id, reason="going in circles")
                ),
                end=True,
            )
            events = await collect(stream)
    seen = kinds(events)
    assert seen[-1] == "result", seen
    assert events[-1].result.output == "went on without the researcher"
    assert seen.count("error") == 0
    # The parent saw the tool error and went on; the child's turn was cut short.
    result = provider.tool_results[0]
    assert result.is_error
    assert "cancelled by an operator: going in circles" in result.content
    assert provider.tool_results[1].content == "final answer recorded"
    assert not provider.release.is_set(), "the child's turn was never released: it was cancelled"
    spans = [e.span for e in events if e.WhichOneof("event") == "span"]
    spawn_call = next(s for s in spans if s.kind == "tool_call" and s.name == "spawn_subagent")
    assert spawn_call.attributes["is_error"] is True
    assert spawn_call.parent_id == spawn.parent_id, "under the root's first turn, as the spawn"
    assert not any(s.kind == "llm_call" and s.agent == "writer/researcher-1" for s in spans)
    assert any(
        e.WhichOneof("event") == "log" and "stopped: cancelled by an operator" in e.log.message
        for e in events
    )
    assert running.census.agents_live == 0
    assert running.census.runs_active == 0


async def test_an_approval_round_trip(serve: Callable[..., Any]) -> None:
    provider = FakeProvider(
        [
            Step(calls=[call("write_file", path="held.md", content="x")]),
            Step(calls=[call("finish", answer="done")]),
        ]
    )
    running = await serve(provider)
    body = execute_body(rules={"require_approval": {"tools": ["write_file"], "timeout_s": 30}})
    async with running.channel() as channel:
        stub = AgentRuntimeStub(channel)
        async with stub.Execute.open(metadata=[("authorization", AUTH["Authorization"])]) as stream:
            await stream.send_message(pb.BackendCommand(execute=to_request(body)))
            request = None
            while request is None:
                event = await asyncio.wait_for(stream.recv_message(), 5)
                assert event is not None
                if event.WhichOneof("event") == "approval_request":
                    request = event.approval_request
            assert request.tool == "write_file"
            assert request.rule == "approvals.tools"
            assert len(request.args_sha256) == 64
            assert request.span_id and request.turn_span_id
            await stream.send_message(pb.BackendCommand(ack=pb.Ack(request_id=request.approval_id)))
            await stream.send_message(
                pb.BackendCommand(
                    approval_decision=pb.ApprovalDecision(
                        approval_id=request.approval_id,
                        decision="approved",
                        reason="go ahead",
                        decided_by="Alice",
                    )
                ),
                end=True,
            )
            events = await collect(stream)
    spans = [e.span for e in events if e.WhichOneof("event") == "span"]
    held = next(s for s in spans if s.kind == "tool_call" and s.name == "write_file")
    assert held.attributes["approval_id"] == request.approval_id
    assert events[-1].WhichOneof("event") == "result"
    assert not provider.tool_results[0].is_error
    assert any(
        e.WhichOneof("event") == "log" and "approved by Alice" in e.log.message for e in events
    )


async def test_a_gateway_tool_goes_up_as_a_request_and_comes_back_as_a_result(
    serve: Callable[..., Any],
) -> None:
    provider = FakeProvider(
        [
            Step(calls=[call("fetch_url", url="https://api.example.com/v1/status")]),
            Step(calls=[call("finish", answer="fetched")]),
        ]
    )
    running = await serve(provider)
    body = execute_body(
        gateway_tools=[
            {
                "name": "fetch_url",
                "description": "Fetch a URL through the gateway.",
                "input_schema": {
                    "type": "object",
                    "properties": {"url": {"type": "string"}},
                    "required": ["url"],
                },
            }
        ]
    )
    async with running.channel() as channel:
        stub = AgentRuntimeStub(channel)
        async with stub.Execute.open(metadata=[("authorization", AUTH["Authorization"])]) as stream:
            await stream.send_message(pb.BackendCommand(execute=to_request(body)))
            request = None
            while request is None:
                event = await asyncio.wait_for(stream.recv_message(), 5)
                assert event is not None
                if event.WhichOneof("event") == "tool_request":
                    request = event.tool_request
            assert request.tool == "fetch_url"
            assert json_format.MessageToDict(request.arguments) == {
                "url": "https://api.example.com/v1/status"
            }
            assert request.turn_span_id
            await stream.send_message(pb.BackendCommand(ack=pb.Ack(request_id=request.request_id)))
            await stream.send_message(
                pb.BackendCommand(
                    tool_result=pb.ToolResult(
                        request_id=request.request_id,
                        output='HTTP 200 application/json\n\n{"ok": true}',
                        is_error=False,
                    )
                ),
                end=True,
            )
            events = await collect(stream)
    assert events[-1].WhichOneof("event") == "result"
    assert provider.tool_results[0].content.endswith('{"ok": true}')
    assert not provider.tool_results[0].is_error
    # The model saw the gateway tool under its own schema, and the runtime recorded no
    # `tool_call` span for it: the backend's spans are the record of what it performed.
    assert "fetch_url" in provider.seen_tools[0]
    spans = [e.span for e in events if e.WhichOneof("event") == "span"]
    assert not any(s.kind == "tool_call" and s.name == "fetch_url" for s in spans)


async def test_a_policy_update_tightens_the_next_decision(serve: Callable[..., Any]) -> None:
    # The update arrives while the first turn is under way; the second turn decides with it.
    provider = SlowProvider(
        [
            Step(calls=[call("list_files", path=".")]),
            Step(calls=[call("write_file", path="late.md", content="x")]),
            Step(calls=[call("finish", answer="done")]),
        ]
    )
    running = await serve(provider)
    async with running.channel() as channel:
        stub = AgentRuntimeStub(channel)
        async with stub.Execute.open(metadata=[("authorization", AUTH["Authorization"])]) as stream:
            await stream.send_message(pb.BackendCommand(execute=to_request(execute_body())))
            await asyncio.wait_for(provider.started.wait(), 5)
            tightened = mint_token(policy_version=7, rules={"tools_denied": ["write_file"]})
            await stream.send_message(
                pb.BackendCommand(policy_update=pb.PolicyUpdate(token=tightened))
            )
            # A token for another run is ignored; the tightened one stays.
            await stream.send_message(
                pb.BackendCommand(
                    policy_update=pb.PolicyUpdate(token=mint_token(run_id="other", rules={}))
                ),
                end=True,
            )
            await asyncio.sleep(0.05)
            provider.release.set()
            events = await collect(stream)
    spans = [e.span for e in events if e.WhichOneof("event") == "span"]
    refused = next(s for s in spans if s.kind == "policy" and s.name == "write_file")
    assert refused.attributes["rule"] == "tools.deny"
    assert refused.attributes["policy_version"] == 7
    assert any(
        e.WhichOneof("event") == "log" and "policy updated to version 7" in e.log.message
        for e in events
    )
    assert events[-1].WhichOneof("event") == "result"


async def test_a_dropped_stream_stops_the_run(serve: Callable[..., Any]) -> None:
    provider = SlowProvider([Step(text="never")])
    running = await serve(provider)
    async with running.channel() as channel:
        stub = AgentRuntimeStub(channel)
        async with stub.Execute.open(metadata=[("authorization", AUTH["Authorization"])]) as stream:
            await stream.send_message(pb.BackendCommand(execute=to_request(execute_body())))
            await asyncio.wait_for(provider.started.wait(), 5)
            assert running.census.runs_active == 1
            await stream.cancel()
    for _ in range(100):
        if running.census.runs_active == 0:
            break
        await asyncio.sleep(0.05)
    assert running.census.runs_active == 0
    assert provider.closed


async def test_refusals_never_start_a_run(serve: Callable[..., Any]) -> None:
    provider = FakeProvider([Step(text="hi")])
    running = await serve(provider)
    async with running.channel() as channel:
        stub = AgentRuntimeStub(channel)
        # No token, a wrong token.
        for metadata in ([], [("authorization", "Bearer " + "x" * 40)]):
            async with stub.Execute.open(metadata=metadata) as stream:
                await stream.send_message(
                    pb.BackendCommand(execute=to_request(execute_body())), end=True
                )
                with pytest.raises(GRPCError) as refused:
                    await collect(stream)
                assert refused.value.status == Status.UNAUTHENTICATED
        # The first command is not the request.
        async with stub.Execute.open(metadata=[("authorization", AUTH["Authorization"])]) as stream:
            await stream.send_message(pb.BackendCommand(cancel=pb.Cancel()), end=True)
            with pytest.raises(GRPCError) as refused:
                await collect(stream)
            assert refused.value.status == Status.INVALID_ARGUMENT
        # An invalid request: refused without echoing it.
        async with stub.Execute.open(metadata=[("authorization", AUTH["Authorization"])]) as stream:
            bad = execute_body(task={"title": "", "kind": "Not A Kind"})
            await stream.send_message(pb.BackendCommand(execute=to_request(bad)), end=True)
            with pytest.raises(GRPCError) as refused:
                await collect(stream)
            assert refused.value.status == Status.INVALID_ARGUMENT
            assert "sk-ant-secret-value" not in (refused.value.message or "")
        # A forged agent token ends the stream with the terminal error, as on NDJSON.
        async with stub.Execute.open(metadata=[("authorization", AUTH["Authorization"])]) as stream:
            forged = execute_body(token=mint_token(key=b"wrong-key-wrong-key-wrong-key-12"))
            await stream.send_message(pb.BackendCommand(execute=to_request(forged)), end=True)
            events = await collect(stream)
            assert kinds(events) == ["error"]
            assert "agent token refused" in events[0].error.message
        # Health answers without a token.
        reply = await stub.Health(pb.HealthRequest())
        assert reply.status == "ok" and reply.runs_active == 0
    assert provider.steps, "the model was never called"
    assert TOKEN  # the shared secret the stream is opened with


async def test_the_request_message_is_the_json_body() -> None:
    body = execute_body(gateway_tools=[{"name": "mcp:x/y", "description": "d"}])
    message = to_request(body)
    parsed = ExecuteRequest.model_validate(execute_to_dict(message))
    assert parsed.run_id == "run-1" and parsed.node_id == "node-1"
    assert parsed.agent.name == "writer"
    assert parsed.task.kind == "document"
    assert parsed.llm.provider == "anthropic"
    assert parsed.llm.api_key is not None
    assert parsed.llm.api_key.get_secret_value() == "sk-ant-secret-value"
    assert parsed.limits.max_turns == 8
    assert parsed.gateway_tools[0].name == "mcp:x/y"
    assert parsed.gateway_tools[0].input_schema == {"type": "object"}
    assert parsed.token == body["token"]
