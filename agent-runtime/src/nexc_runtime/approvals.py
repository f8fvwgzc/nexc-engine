"""Approvals: tool calls a policy holds until a human decides (CONTRACT section 8).

The runtime asks by reporting a `policy` span whose decision is `require_approval` and then
waits in the tool call. The backend records the request, tells the approvers and, once one of
them decided, hands the decision to this runtime with `POST /v1/approvals/{id}`; the registry
here wakes the waiting call. A call nobody decides fails its node when the wait runs out.
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass


class ApprovalTimeout(Exception):
    """Nobody decided before the wait ran out: the node fails."""


@dataclass(frozen=True, slots=True)
class ApprovalOutcome:
    """What the backend delivered."""

    decision: str  # "approved" | "denied"
    reason: str = ""
    decided_by: str = ""

    @property
    def approved(self) -> bool:
        return self.decision == "approved"


class ApprovalRegistry:
    """The calls of this process that wait for a decision, by approval id."""

    def __init__(self) -> None:
        self._pending: dict[str, asyncio.Future[ApprovalOutcome]] = {}

    def register(self, approval_id: str) -> asyncio.Future[ApprovalOutcome]:
        """Starts waiting for `approval_id`; the future resolves with the decision."""
        future: asyncio.Future[ApprovalOutcome] = asyncio.get_running_loop().create_future()
        self._pending[approval_id] = future
        return future

    def forget(self, approval_id: str) -> None:
        self._pending.pop(approval_id, None)

    def resolve(self, approval_id: str, outcome: ApprovalOutcome) -> bool:
        """Wakes the call waiting for `approval_id`; False when none waits here."""
        future = self._pending.pop(approval_id, None)
        if future is None or future.done():
            return False
        future.set_result(outcome)
        return True

    @property
    def pending(self) -> int:
        return len(self._pending)
