"""Agent births, budgets and the family tree of one run.

Agents are "born" from a spec at request time. An agent may spawn sub-agents, each receiving a
share of its parent's remaining token budget; the tree is at most `MAX_DEPTH` levels below the
root and each agent may have at most `MAX_CHILDREN` children.
"""

from __future__ import annotations

import itertools
from dataclasses import dataclass, field

MAX_DEPTH = 2
MAX_CHILDREN = 4
CHILD_SHARE = 0.5
MIN_CHILD_BUDGET = 4_000


class SpawnRefused(Exception):
    """A spawn request violates the depth, fan-out or budget rules."""


class Budget:
    """Token budget. Spending in a child budget is also charged to every ancestor."""

    def __init__(self, limit: int, parent: Budget | None = None) -> None:
        self.limit = max(0, limit)
        self.spent = 0
        self.parent = parent

    @property
    def remaining(self) -> int:
        own = self.limit - self.spent
        if self.parent is not None:
            own = min(own, self.parent.remaining)
        return max(0, own)

    @property
    def exhausted(self) -> bool:
        return self.remaining <= 0

    def spend(self, tokens: int) -> None:
        self.spent += tokens
        if self.parent is not None:
            self.parent.spend(tokens)

    def allocate_child(self) -> Budget:
        share = int(self.remaining * CHILD_SHARE)
        if share < MIN_CHILD_BUDGET:
            raise SpawnRefused(
                f"not enough budget left to spawn a sub-agent ({self.remaining} tokens remaining)"
            )
        return Budget(share, parent=self)


class Census:
    """Process-wide counters, reported by `/healthz`."""

    def __init__(self) -> None:
        self.agents_live = 0
        self.runs_active = 0


@dataclass(slots=True)
class AgentRecord:
    id: int
    name: str
    role: str
    depth: int
    parent: AgentRecord | None = None
    children: int = 0
    alive: bool = True
    lineage: list[str] = field(default_factory=list)


class AgentRegistry:
    """Tracks every agent born during one run."""

    def __init__(self, census: Census) -> None:
        self._census = census
        self._ids = itertools.count(1)
        self.records: list[AgentRecord] = []

    def birth(self, name: str, role: str, parent: AgentRecord | None = None) -> AgentRecord:
        if parent is not None:
            if parent.depth >= MAX_DEPTH:
                raise SpawnRefused(f"maximum agent depth ({MAX_DEPTH}) reached")
            if parent.children >= MAX_CHILDREN:
                raise SpawnRefused(f"an agent may spawn at most {MAX_CHILDREN} sub-agents")
            parent.children += 1
        record = AgentRecord(
            id=next(self._ids),
            name=name,
            role=role,
            depth=0 if parent is None else parent.depth + 1,
            parent=parent,
            lineage=[*parent.lineage, parent.name] if parent else [],
        )
        self.records.append(record)
        self._census.agents_live += 1
        return record

    def retire(self, record: AgentRecord) -> None:
        if record.alive:
            record.alive = False
            self._census.agents_live -= 1

    def retire_all(self) -> None:
        for record in self.records:
            self.retire(record)

    @staticmethod
    def can_spawn(record: AgentRecord) -> bool:
        return record.depth < MAX_DEPTH and record.children < MAX_CHILDREN
