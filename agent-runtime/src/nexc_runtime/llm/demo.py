"""Deterministic, offline `demo` provider.

It lets anyone try nexc-engine end to end with no API key: it drives the real tool loop
(so documents become genuine .docx files and code nodes become real files), streams deltas
and reports plausible token counts. Every output is clearly labelled as demo content.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Any

from .base import Conversation, StopKind, TextSink, ToolCall, ToolResult, ToolSpec, Turn

DEMO_BANNER = "> **Demo mode** - generated offline by the nexc demo provider; no LLM was called."
_CHUNK = 24


@dataclass(frozen=True, slots=True)
class DemoBrief:
    """What the demo needs to know about the node it is 'working' on."""

    title: str
    kind: str
    content: str = ""
    goal: str = ""
    role: str = "agent"
    upstream_titles: tuple[str, ...] = field(default_factory=tuple)
    upstream_outputs: tuple[str, ...] = field(default_factory=tuple)


class DemoProvider:
    name = "demo"

    def __init__(self, brief: DemoBrief, *, model: str = "nexc-demo") -> None:
        self.model = model
        self._brief = brief

    def new_conversation(self, system: str, prompt: str) -> Conversation:
        return Conversation(system=system, messages=[{"role": "user", "content": prompt}])

    def add_tool_results(self, conversation: Conversation, results: list[ToolResult]) -> None:
        conversation.messages.append(
            {
                "role": "user",
                "content": [
                    {"tool_use_id": r.tool_use_id, "content": r.content, "is_error": r.is_error}
                    for r in results
                ],
            }
        )

    def add_user_text(self, conversation: Conversation, text: str) -> None:
        conversation.messages.append({"role": "user", "content": text})

    async def aclose(self) -> None:
        return None

    async def next_turn(
        self,
        conversation: Conversation,
        tools: list[ToolSpec],
        *,
        max_tokens: int,
        on_text: TextSink | None = None,
    ) -> Turn:
        available = {t.name for t in tools}
        assistant_turns = sum(1 for m in conversation.messages if m["role"] == "assistant")
        call = self._deliverable_call() if assistant_turns == 0 else None

        stop: StopKind
        if call is not None and call.name in available:
            text = f"Working as {self._brief.role}: drafting the deliverable for "
            text += f"“{self._brief.title}”.\n\n"
            calls = [call]
            stop = "tool_use"
        else:
            text = self._final_answer()
            calls = []
            stop = "end_turn"

        _stream(text, on_text)
        conversation.messages.append(
            {"role": "assistant", "content": text, "tool_calls": [c.name for c in calls]}
        )
        prompt_chars = len(conversation.system) + sum(len(str(m)) for m in conversation.messages)
        produced = len(text) + sum(len(str(c.input)) for c in calls)
        return Turn(
            text=text,
            tool_calls=calls,
            stop=stop,
            input_tokens=max(1, prompt_chars // 4),
            output_tokens=max(1, produced // 4),
        )

    # --- deliverables -----------------------------------------------------------------------
    def _deliverable_call(self) -> ToolCall | None:
        b = self._brief
        slug = slugify(b.title)
        if b.kind == "document":
            return ToolCall(id="demo_docx", name="make_docx", input=self._docx_input(slug))
        if b.kind == "output":
            return ToolCall(id="demo_final", name="make_docx", input=self._compiled_docx(slug))
        if b.kind == "code":
            return ToolCall(
                id="demo_code",
                name="write_file",
                input={"path": f"{slug.replace('-', '_')}.py", "content": self._code_file()},
            )
        if b.kind == "research":
            return ToolCall(
                id="demo_notes",
                name="write_file",
                input={"path": f"{slug}-notes.md", "content": self._research_notes()},
            )
        return None

    def _docx_input(self, slug: str) -> dict[str, Any]:
        b = self._brief
        sections: list[dict[str, Any]] = [
            {
                "heading": "Executive summary",
                "paragraphs": [
                    f"This document addresses “{b.title}”"
                    + (f" in service of the goal: {b.goal}." if b.goal else "."),
                    "It was produced in demo mode to show the shape of a real deliverable; "
                    "connect an LLM provider to get substantive content.",
                ],
                "bullets": [],
                "table_rows": [],
            },
            {
                "heading": "Scope and inputs",
                "paragraphs": [_first_sentence(b.content) or "No additional brief was given."],
                "bullets": list(b.upstream_titles) or ["No upstream nodes."],
                "table_rows": [],
            },
            {
                "heading": "Plan",
                "paragraphs": ["The work is broken down into the following steps."],
                "bullets": [],
                "table_rows": [
                    ["Step", "Owner", "Outcome"],
                    ["1. Clarify requirements", b.role, "Agreed scope"],
                    ["2. Draft deliverable", b.role, "Reviewed draft"],
                    ["3. Review and finalise", "reviewer", "Signed-off document"],
                ],
            },
            {
                "heading": "Next steps",
                "paragraphs": [],
                "bullets": [
                    "Switch NEXC_LLM_PROVIDER to anthropic (or add your key in Settings).",
                    "Re-run this node to replace the demo content.",
                ],
                "table_rows": [],
            },
        ]
        return {"filename": f"{slug}.docx", "title": f"{b.title} (demo)", "sections": sections}

    def _compiled_docx(self, slug: str) -> dict[str, Any]:
        """The final deliverable: one section per upstream node, built from its real output."""
        b = self._brief
        sections: list[dict[str, Any]] = [
            {
                "heading": "About this document",
                "paragraphs": [
                    (f"Goal: {b.goal}" if b.goal else f"Deliverable: {b.title}"),
                    "Compiled in demo mode from the outputs of the upstream graph nodes; "
                    "connect an LLM provider for substantive content.",
                ],
                "bullets": [],
                "table_rows": [],
            }
        ]
        for title, output in zip(b.upstream_titles, b.upstream_outputs, strict=False):
            sections.append(
                {
                    "heading": title,
                    "paragraphs": _summary_lines(output) or ["(no output)"],
                    "bullets": [],
                    "table_rows": [],
                }
            )
        return {"filename": f"{slug}.docx", "title": f"{b.title} (demo)", "sections": sections}

    def _code_file(self) -> str:
        b = self._brief
        title = _escape(b.title)
        steps = ["Clarify requirements", "Implement the core logic", "Test and document"]
        lines = [
            f'"""{title} - demo program generated offline by nexc (no LLM was called).',
            "",
            "Run it with `python <file>`; connect an LLM provider for a full implementation.",
            '"""',
            "",
            "from __future__ import annotations",
            "",
            f'TITLE = "{title}"',
            f"STEPS = {steps!r}",
            "",
            "",
            "def plan() -> list[str]:",
            '    """Return the numbered work plan for this task."""',
            '    return [f"{i}. {step}" for i, step in enumerate(STEPS, start=1)]',
            "",
            "",
            "def main() -> None:",
            '    print(f"{TITLE} (demo)")',
            "    for line in plan():",
            "        print(line)",
            "",
            "",
            'if __name__ == "__main__":',
            "    main()",
        ]
        return "\n".join(lines) + "\n"

    def _research_notes(self) -> str:
        b = self._brief
        lines = [
            f"# {b.title} - research notes (demo)",
            "",
            DEMO_BANNER,
            "",
            "## Question",
            _first_sentence(b.content) or b.title,
            "",
            "## Sources to consult",
            "- Primary documentation and specifications",
            "- Recent peer-reviewed or industry reports",
            "- Comparable open-source projects",
            "",
            "## Findings",
            "- (demo) Findings appear here once a real LLM provider is configured.",
        ]
        return "\n".join(lines) + "\n"

    def _final_answer(self) -> str:
        b = self._brief
        parts = [
            DEMO_BANNER,
            "",
            f"## {b.title}",
            "",
            f"**Role:** {b.role} · **Kind:** {b.kind}",
        ]
        if b.goal:
            parts += ["", f"**Goal:** {b.goal}"]
        if b.upstream_titles:
            parts += ["", "**Built on:** " + ", ".join(b.upstream_titles)]
        parts += [
            "",
            "### Outcome",
            f"- Analysed the brief: {_first_sentence(b.content) or 'no extra brief given'}",
            "- Produced the deliverable"
            + (
                " attached as an artifact."
                if b.kind in ("document", "code", "research", "output")
                else "."
            ),
            "- Ready for downstream nodes.",
        ]
        return "\n".join(parts) + "\n"


def _summary_lines(text: str, limit: int = 4) -> list[str]:
    """First few prose lines of a node output, without banners or markdown markup."""
    lines: list[str] = []
    for raw in text.splitlines():
        line = raw.strip().lstrip("#>-* ").replace("**", "").strip()
        if line and "demo mode" not in line.lower():
            lines.append(line)
        if len(lines) == limit:
            break
    return lines


def _stream(text: str, on_text: TextSink | None) -> None:
    if on_text is None:
        return
    for start in range(0, len(text), _CHUNK):
        on_text(text[start : start + _CHUNK])


def slugify(text: str) -> str:
    slug = re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")
    return slug[:60] or "deliverable"


def _first_sentence(text: str) -> str:
    text = " ".join(text.split())
    match = re.match(r"(.{1,240}?[.!?])(\s|$)", text)
    return match.group(1) if match else text[:240]


def _escape(text: str) -> str:
    return text.replace("\\", "\\\\").replace('"', '\\"')
