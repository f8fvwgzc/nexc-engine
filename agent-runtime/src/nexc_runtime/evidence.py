"""Citation ids on what an agent is given (CONTRACT section 8, `docs/AGENTSECOPS.md` section 2).

The backend heads every document passage with `[doc:<chunk id>] <source>`, every memory with
`[mem:<id>]` and names every upstream output's node id; the model is told to cite these ids
after each claim that rests on them and to end a claim of its own with `[assumption]`. The
backend verifies the output; this module only reads the ids back and finds the citations in
text so that deliverables can carry a "Sources" section.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

CITATION_KINDS = ("doc", "mem", "node", "fact")
# A claim the model declares its own: counted as declared, not as evidence.
ASSUMPTION_MARKERS = ("[assumption]", "[own reasoning]", "[inference]")
_ID = r"[A-Za-z0-9._:/-]{1,120}"
_TOKEN = re.compile(rf"^(doc|mem|node|fact):({_ID})$")
_HEAD = re.compile(rf"^\[((?:doc|mem|node|fact):{_ID})\]\s*")
_BRACKET = re.compile(r"\[([^\[\]\n]{1,2000})\]")
_FENCE = re.compile(r"```.*?```", re.DOTALL)
_INLINE_CODE = re.compile(r"`[^`\n]*`")

CITATION_INSTRUCTION = (
    "Every document passage, memory and upstream result you are given carries an id in "
    "brackets, such as [doc:…], [mem:…] or [node:…]. After each claim that rests on one of "
    "them, cite its id in brackets exactly as given; several ids may share one bracket, "
    "separated by commas. Say explicitly when a statement is your own reasoning or an "
    "assumption by ending it with [assumption]. Never invent an id, and never cite what you "
    "were not given."
)


def split_head(line: str) -> tuple[str, str]:
    """`"[doc:0199…] customs.pdf, p. 17"` -> `("doc:0199…", "customs.pdf, p. 17")`; a line
    without a citation head comes back with an empty id."""
    match = _HEAD.match(line)
    if match is None:
        return "", line
    return match.group(1), line[match.end() :]


def find_citations(text: str) -> list[str]:
    """Every citation id in `text` (`doc:…`, `mem:…`, …), in order, once each; ids inside
    fenced or inline code are not citations, and a bracket holding anything but ids is not
    one either (`[customs.pdf, p. 17]`)."""
    prose = _INLINE_CODE.sub(" ", _FENCE.sub(" ", text))
    found: list[str] = []
    for bracket in _BRACKET.finditer(prose):
        tokens = [t for t in re.split(r"[,;\s]+", bracket.group(1).strip()) if t]
        ids = [m.group(0) for m in map(_TOKEN.match, tokens) if m is not None]
        if not tokens or len(ids) != len(tokens):
            continue
        for item in ids:
            if item not in found:
                found.append(item)
    return found


@dataclass(frozen=True, slots=True)
class Source:
    """One thing the agent was given, by its citation id."""

    id: str
    title: str


@dataclass(slots=True)
class Sources:
    """What the agent was given, by id, with a title each: what a "Sources" section says
    about a cited id."""

    items: dict[str, Source] = field(default_factory=dict)

    @classmethod
    def from_context(
        cls,
        *,
        upstream: list[tuple[str, str]],
        memories: list[str],
        documents: list[str],
    ) -> Sources:
        """Reads the ids off the context as the backend sends it: `upstream` as `(node id,
        title)` pairs, memories and documents as the headed strings."""
        sources = cls()
        for node_id, title in upstream:
            if node_id:
                sources.add(f"node:{node_id}", title or node_id)
        for memory in memories:
            cid, text = split_head(memory)
            if cid:
                sources.add(cid, _excerpt(text))
        for passage in documents:
            head, _, _ = passage.strip().partition("\n")
            cid, source = split_head(head)
            if cid:
                sources.add(cid, source.strip() or cid)
        return sources

    def add(self, cid: str, title: str) -> None:
        self.items.setdefault(cid, Source(cid, title))

    def cited_in(self, text: str) -> list[Source]:
        """The sources `text` cites, in order of first citation; an id the agent was not
        given is listed by its id alone, so a reader sees it was never a source."""
        return [
            self.items.get(cid, Source(cid, "not among what the agent was given"))
            for cid in find_citations(text)
        ]

    def markdown_section(self, text: str) -> str:
        """A `## Sources` section for the ids `text` cites; empty when it cites none."""
        cited = self.cited_in(text)
        if not cited:
            return ""
        lines = ["## Sources", ""]
        lines.extend(f"- [{s.id}] {s.title}" for s in cited)
        return "\n".join(lines) + "\n"

    def with_sources(self, text: str) -> str:
        """`text` with its `## Sources` section appended, when it cites ids and has no such
        section yet; the text as it is otherwise."""
        section = self.markdown_section(text)
        if not section or _has_sources_heading(text):
            return text
        return text.rstrip() + "\n\n" + section


_SOURCES_HEADING = re.compile(
    r"^#{1,6}\s+(sources|references|citations|bibliography)\s*:?\s*$", re.I | re.M
)


def _has_sources_heading(text: str) -> bool:
    return _SOURCES_HEADING.search(text) is not None


def _excerpt(text: str, limit: int = 120) -> str:
    text = " ".join(text.split())
    return text if len(text) <= limit else text[: limit - 1].rstrip() + "…"
