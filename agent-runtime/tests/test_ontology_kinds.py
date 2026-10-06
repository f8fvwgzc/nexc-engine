"""Node kinds are keys of a per-graph ontology, so the runtime accepts any slug."""

import pytest
from pydantic import ValidationError

from nexc_runtime.api.schemas import TaskIn
from nexc_runtime.prompts import DEFAULT_KIND_INSTRUCTION, KIND_INSTRUCTIONS, build_system_prompt


def _prompt(kind: str, **attributes: object) -> str:
    return build_system_prompt(
        name="Ada",
        role="researcher",
        custom_prompt="",
        kind=kind,
        can_spawn=False,
        code_exec=False,
        **attributes,  # type: ignore[arg-type]
    )


def test_task_accepts_kinds_defined_by_a_graph() -> None:
    task = TaskIn(title="RSI divergence", kind="market_signal", produces_artifact=True)
    assert task.kind == "market_signal"
    assert TaskIn(title="Legacy").produces_artifact is None
    with pytest.raises(ValidationError):
        TaskIn(title="Bad", kind="Market Signal")


def test_prompt_follows_the_declared_attributes_of_an_unknown_kind() -> None:
    described = _prompt("market_signal", kind_description="An observable market condition.")
    assert "An observable market condition." in described
    assert DEFAULT_KIND_INSTRUCTION in described
    assert KIND_INSTRUCTIONS["document"] in _prompt("trade_plan", produces_artifact=True)
