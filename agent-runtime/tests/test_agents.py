"""Agent births, sub-agent budget split, spawn limits and sandboxed code execution."""

from __future__ import annotations

from collections.abc import Callable
from typing import Any

import pytest
from fastapi.testclient import TestClient

from nexc_runtime.agents import MAX_CHILDREN, AgentRegistry, Budget, Census, SpawnRefused
from nexc_runtime.agents.registry import MIN_CHILD_BUDGET

from .conftest import AUTH, FakeProvider, Step, call, execute_body, parse_ndjson

ClientFactory = Callable[..., tuple[TestClient, FakeProvider]]


def test_child_budget_is_a_share_of_the_parent_remaining() -> None:
    parent = Budget(100_000)
    parent.spend(20_000)
    child = parent.allocate_child()
    assert child.limit == 40_000  # half of the 80k remaining
    child.spend(10_000)
    assert parent.spent == 30_000  # child spending is charged to the parent
    assert child.remaining == 30_000
    grandchild = child.allocate_child()
    assert grandchild.limit == 15_000


def test_spawn_refused_when_budget_too_small() -> None:
    parent = Budget(MIN_CHILD_BUDGET)
    with pytest.raises(SpawnRefused):
        parent.allocate_child()


def test_registry_enforces_depth_and_fan_out() -> None:
    census = Census()
    registry = AgentRegistry(census)
    root = registry.birth("root", "planner")
    child = registry.birth("c", "researcher", parent=root)
    grandchild = registry.birth("g", "writer", parent=child)
    assert grandchild.depth == 2 and grandchild.lineage == ["root", "c"]
    assert not AgentRegistry.can_spawn(grandchild)
    with pytest.raises(SpawnRefused):
        registry.birth("too-deep", "x", parent=grandchild)
    for i in range(MAX_CHILDREN - 1):
        registry.birth(f"s{i}", "x", parent=root)
    with pytest.raises(SpawnRefused):
        registry.birth("one-too-many", "x", parent=root)
    assert census.agents_live == 2 + MAX_CHILDREN
    registry.retire_all()
    assert census.agents_live == 0


def test_spawn_subagent_runs_child_with_budget_share(client_for: ClientFactory) -> None:
    client, provider = client_for(
        [
            Step(
                calls=[call("spawn_subagent", role="researcher", task="Find three facts.")],
                tokens_in=1_000,
                tokens_out=0,
            ),
            # the child's turns
            Step(text="Child working."),
            # back in the parent
            Step(calls=[call("finish", answer="Parent done.")]),
        ]
    )
    body = execute_body(agent={"budget_tokens": 101_000})
    events = parse_ndjson(client.post("/v1/execute", json=body, headers=AUTH).content)

    spawns = [e for e in events if e["type"] == "spawn"]
    assert spawns == [
        {"type": "spawn", "agent": {"name": "writer/researcher-1", "role": "researcher"}}
    ]
    born = [e["message"] for e in events if e["type"] == "log" and "born with" in e["message"]]
    assert any("50000 tokens" in message for message in born)  # half of 100k remaining
    assert provider.tool_results[0].content == "Child working."
    assert events[-1]["output"] == "Parent done."
    # the child (depth 1) may spawn too; nobody beyond depth 2 gets the tool
    assert "spawn_subagent" in provider.seen_tools[1]
    assert client.get("/healthz").json()["agents_loaded"] == 0


def test_run_python_unavailable_unless_enabled_on_runtime(client_for: ClientFactory) -> None:
    client, provider = client_for(
        [Step(calls=[call("run_python", code="print(1)")]), Step(text="done")]
    )
    body: dict[str, Any] = execute_body(limits={"allow_code_exec": True})
    events = parse_ndjson(client.post("/v1/execute", json=body, headers=AUTH).content)
    assert "run_python" not in provider.seen_tools[0]
    assert provider.tool_results[0].is_error
    assert any(e["type"] == "log" and "disabled" in e["message"] for e in events)


def test_run_python_executes_in_workspace_when_enabled(client_for: ClientFactory) -> None:
    code = (
        "import os\n"
        "open('out.txt', 'w').write('42')\n"
        "print(sorted(k for k in os.environ if 'KEY' in k or 'TOKEN' in k))\n"
    )
    client, provider = client_for(
        [Step(calls=[call("run_python", code=code)]), Step(text="done")],
        runtime_allow_code_exec=True,
    )
    body = execute_body(limits={"allow_code_exec": True}, task={"kind": "code"})
    events = parse_ndjson(client.post("/v1/execute", json=body, headers=AUTH).content)
    result = provider.tool_results[0]
    assert not result.is_error
    assert "exit code: 0" in result.content
    assert "[]" in result.content  # no secrets in the child environment
    assert [e["path"] for e in events if e["type"] == "artifact"] == ["out.txt"]


def test_run_python_timeout_kills_process(client_for: ClientFactory) -> None:
    client, provider = client_for(
        [Step(calls=[call("run_python", code="while True: pass")]), Step(text="done")],
        runtime_allow_code_exec=True,
        runtime_python_timeout_s=1,
    )
    body = execute_body(limits={"allow_code_exec": True}, task={"kind": "code"})
    client.post("/v1/execute", json=body, headers=AUTH)
    assert "timeout" in provider.tool_results[0].content or "exit code: -" in (
        provider.tool_results[0].content
    )
