"""What the backend sends down the gRPC stream while a run is under way (CONTRACT section 8):
decisions on held tool calls, results of gateway tool calls, a fresh token, a cancel.

The registries here are the runtime's side of each exchange: a call registers what it waits
for, the stream's reader resolves it when the command arrives. The NDJSON wire has none of
this but the approvals, which it receives through `POST /v1/approvals/{id}`.
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from typing import Any

from .approvals import ApprovalRegistry
from .policy import AgentToken


@dataclass(frozen=True, slots=True)
class ToolOutcome:
    """What the backend's gateway answered."""

    output: str
    is_error: bool = False


class ToolRequestRegistry:
    """The gateway calls of this run that wait for their result, by request id."""

    def __init__(self) -> None:
        self._pending: dict[str, asyncio.Future[ToolOutcome]] = {}
        self._acked: set[str] = set()

    def register(self, request_id: str) -> asyncio.Future[ToolOutcome]:
        future: asyncio.Future[ToolOutcome] = asyncio.get_running_loop().create_future()
        self._pending[request_id] = future
        return future

    def forget(self, request_id: str) -> None:
        self._pending.pop(request_id, None)
        self._acked.discard(request_id)

    def ack(self, request_id: str) -> None:
        """The backend took the request and is working on it."""
        if request_id in self._pending:
            self._acked.add(request_id)

    def acked(self, request_id: str) -> bool:
        return request_id in self._acked

    def resolve(self, request_id: str, outcome: ToolOutcome) -> bool:
        """Wakes the call waiting for `request_id`; False when none waits."""
        future = self._pending.pop(request_id, None)
        if future is None or future.done():
            return False
        future.set_result(outcome)
        return True

    @property
    def pending(self) -> int:
        return len(self._pending)


@dataclass(slots=True)
class RunControl:
    """The backend's hold on one run over the gRPC stream."""

    approvals: ApprovalRegistry = field(default_factory=ApprovalRegistry)
    tools: ToolRequestRegistry = field(default_factory=ToolRequestRegistry)
    # A token the backend sent after the run started (`policy_update`): the root agent takes
    # it up at its next decision; sub-agents already born keep what they were given.
    updated_token: AgentToken | None = None
    # Why the backend cancelled, when it did.
    cancel_reason: str | None = None

    def take_token(self, current: AgentToken) -> AgentToken:
        """The token to decide with now: an update not yet taken, else `current`."""
        if self.updated_token is not None and self.updated_token is not current:
            return self.updated_token
        return current


def tool_arguments(raw: dict[str, Any]) -> dict[str, Any]:
    """The arguments of a gateway call as sent up: the model's, as given."""
    return dict(raw)
