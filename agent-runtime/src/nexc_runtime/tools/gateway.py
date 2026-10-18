"""Gateway tools: tools the backend performs on the agent's behalf (`fetch_url`,
`mcp:<server>/<tool>`). The runtime never talks to a server or a host: the call goes up the
gRPC stream as a `tool_request`, the backend decides and performs it, and the result comes
back as a `tool_result` command. Without the stream (the NDJSON wire) these tools are not
offered, and a call fails with a clear error."""

from __future__ import annotations

import asyncio
import time
from typing import Any

from pydantic import BaseModel, ConfigDict

from ..trace import new_span_id
from .base import APPROVAL_HEARTBEAT_S, Tool, ToolContext, ToolError

# A gateway call that the backend neither acknowledged nor answered in this long is given up.
GATEWAY_WAIT_S = 120.0
# Longest the backend may take to acknowledge a request before the call is given up early.
ACK_WAIT_S = 15.0


class GatewayArgs(BaseModel):
    """The arguments as the server's schema describes them: passed through, checked there."""

    model_config = ConfigDict(extra="allow")


async def call_gateway(ctx: ToolContext, args: GatewayArgs) -> str:
    tool = ctx.current_tool or "gateway"
    if ctx.control is None:
        raise ToolError(
            f"{tool} is performed by the backend over the gRPC stream, which this run does not have"
        )
    registry = ctx.control.tools
    request_id = new_span_id()
    arguments: dict[str, Any] = dict(args.model_extra or {})
    if ctx.pending_approval_id is not None:
        arguments["__approval_id"] = ctx.pending_approval_id
    future = registry.register(request_id)
    ctx.events.tool_request(
        request_id=request_id,
        tool=tool,
        arguments=arguments,
        agent=ctx.principal,
        turn_span_id=ctx.turn_span_id,
    )
    started = time.monotonic()
    try:
        while True:
            elapsed = time.monotonic() - started
            if elapsed >= GATEWAY_WAIT_S:
                raise ToolError(f"the gateway did not answer {tool} within {int(GATEWAY_WAIT_S)}s")
            if elapsed >= ACK_WAIT_S and not registry.acked(request_id):
                raise ToolError(f"the backend did not take the {tool} request")
            done, _ = await asyncio.wait({future}, timeout=APPROVAL_HEARTBEAT_S)
            if done:
                break
            ctx.events.log("info", f"{ctx.agent_name}: still waiting for the gateway on {tool}")
    finally:
        registry.forget(request_id)
    outcome = future.result()
    if outcome.is_error:
        raise ToolError(outcome.output)
    return outcome.output


def gateway_tool(name: str, description: str, input_schema: dict[str, Any]) -> Tool[GatewayArgs]:
    """A tool the model sees under the server's own schema; calls go to the backend."""
    return Tool(
        name=name,
        description=description or f"{name} (performed by the platform's gateway)",
        args=GatewayArgs,
        handler=call_gateway,
        input_schema=input_schema,
        # The backend records the authoritative `tool_call` and `egress` spans of the call.
        traced=False,
    )
