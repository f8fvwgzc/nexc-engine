"""The offline demo provider drives the real tool loop end to end."""

from __future__ import annotations

import base64
import io
from collections.abc import Callable
from typing import Any

import docx
import pytest
from fastapi.testclient import TestClient

from nexc_runtime.api.app import create_app
from nexc_runtime.config import Settings
from nexc_runtime.llm.demo import DEMO_BANNER

from .conftest import AUTH, execute_body, parse_ndjson


def _run(make_settings: Callable[..., Settings], **overrides: Any) -> list[dict[str, Any]]:
    with TestClient(create_app(make_settings())) as client:
        body = execute_body(llm={"provider": "demo", "api_key": None}, **overrides)
        response = client.post("/v1/execute", json=body, headers=AUTH)
    assert response.status_code == 200
    return parse_ndjson(response.content)


def test_demo_document_produces_real_docx(make_settings: Callable[..., Settings]) -> None:
    events = _run(make_settings)
    assert events[-1]["type"] == "result"
    assert events[-1]["output"].startswith(DEMO_BANNER)
    assert events[-1]["tokens_in"] > 0 and events[-1]["tokens_out"] > 0
    assert any(e["type"] == "delta" for e in events)
    (artifact,) = [e for e in events if e["type"] == "artifact"]
    assert artifact["path"] == "quarterly-report.docx"
    document = docx.Document(io.BytesIO(base64.b64decode(artifact["content_b64"])))
    assert document.paragraphs[0].text == "Quarterly report (demo)"
    assert len(document.tables) == 1


def test_demo_output_compiles_upstream_into_docx(make_settings: Callable[..., Settings]) -> None:
    upstream = [
        {
            "node_id": "a",
            "title": "Findings",
            "output": f"{DEMO_BANNER}\n\n## Findings\nRevenue grew 12%.",
        },
        {"node_id": "b", "title": "Risks", "output": "- Supply chain delays"},
    ]
    events = _run(
        make_settings,
        task={"title": "Final report", "kind": "output"},
        context={"upstream": upstream},
    )
    (artifact,) = [e for e in events if e["type"] == "artifact"]
    assert artifact["path"] == "final-report.docx"
    document = docx.Document(io.BytesIO(base64.b64decode(artifact["content_b64"])))
    text = [p.text for p in document.paragraphs]
    assert "Findings" in text and "Revenue grew 12%." in text
    assert "Risks" in text and "Supply chain delays" in text
    assert not any("demo mode - generated offline" in t.lower() for t in text[1:])


@pytest.mark.parametrize(
    ("kind", "expected"),
    [("code", ["quarterly_report.py"]), ("research", ["quarterly-report-notes.md"]), ("topic", [])],
)
def test_demo_other_kinds(
    make_settings: Callable[..., Settings], kind: str, expected: list[str]
) -> None:
    events = _run(make_settings, task={"kind": kind})
    assert events[-1]["type"] == "result"
    assert [e["path"] for e in events if e["type"] == "artifact"] == expected


def _comparable(events: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Events with .docx bytes replaced by their text: the files embed creation timestamps."""
    out = []
    for event in events:
        if event["type"] == "artifact" and event["path"].endswith(".docx"):
            document = docx.Document(io.BytesIO(base64.b64decode(event["content_b64"])))
            text = [p.text for p in document.paragraphs]
            out.append({**event, "content_b64": text, "size": None})
        else:
            out.append(event)
    return out


def test_demo_is_deterministic(make_settings: Callable[..., Settings]) -> None:
    first = _run(make_settings)
    second = _run(make_settings)
    assert _comparable(first) == _comparable(second)
