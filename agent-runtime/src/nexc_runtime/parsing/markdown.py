"""Markdown: ATX / setext headings, paragraphs, list items and pipe tables. Line based."""

from __future__ import annotations

import re

from .model import Block, ParsedDocument, decode_text, heading, paragraph
from .tables import table_block

_ATX = re.compile(r"^ {0,3}(#{1,6})(?:\s+(.*?))?\s*#*\s*$")
_SETEXT = re.compile(r"^ {0,3}(=+|-+)\s*$")
_RULE = re.compile(r"^ {0,3}([-*_])(?:\s*\1){2,}\s*$")
_FENCE = re.compile(r"^ {0,3}(```|~~~)")
_LIST_ITEM = re.compile(r"^\s*(?:[-*+]|\d{1,9}[.)])\s+(.*)$")
_DELIMITER_ROW = re.compile(r"^\s*\|?\s*:?-+:?\s*(?:\|\s*:?-+:?\s*)*\|?\s*$")
_CELL_SPLIT = re.compile(r"(?<!\\)\|")


def _cells(line: str) -> list[str]:
    stripped = line.strip()
    stripped = stripped.removeprefix("|")
    if stripped.endswith("|") and not stripped.endswith("\\|"):
        stripped = stripped[:-1]
    return [cell.replace("\\|", "|") for cell in _CELL_SPLIT.split(stripped)]


def _is_table_start(lines: list[str], index: int) -> bool:
    if index + 1 >= len(lines) or "|" not in lines[index]:
        return False
    delimiter = lines[index + 1]
    return "-" in delimiter and "|" in delimiter and bool(_DELIMITER_ROW.match(delimiter))


class _Builder:
    def __init__(self) -> None:
        self.blocks: list[Block] = []
        self.pending: list[str] = []

    def add(self, block: Block | None) -> None:
        if block is not None:
            self.blocks.append(block)

    def flush(self) -> None:
        if self.pending:
            self.add(paragraph(" ".join(self.pending)))
            self.pending = []


def _skip_front_matter(lines: list[str]) -> int:
    if lines and lines[0].strip() == "---":
        for index in range(1, len(lines)):
            if lines[index].strip() in ("---", "..."):
                return index + 1
    return 0


def parse_markdown(data: bytes) -> ParsedDocument:
    lines = decode_text(data).replace("\r\n", "\n").replace("\r", "\n").split("\n")
    out = _Builder()
    index = _skip_front_matter(lines)
    while index < len(lines):
        line = lines[index]
        index += 1
        if not line.strip():
            out.flush()
        elif fence := _FENCE.match(line):
            out.flush()
            code: list[str] = []
            while index < len(lines) and not lines[index].lstrip().startswith(fence.group(1)):
                code.append(lines[index])
                index += 1
            index += 1  # closing fence
            out.add(paragraph(" ".join(code)))
        elif atx := _ATX.match(line):
            out.flush()
            out.add(heading(atx.group(2), len(atx.group(1))))
        elif out.pending and (setext := _SETEXT.match(line)):
            title, out.pending = " ".join(out.pending), []
            out.add(heading(title, 1 if setext.group(1).startswith("=") else 2))
        elif _RULE.match(line):
            out.flush()
        elif _is_table_start(lines, index - 1):
            out.flush()
            rows = [_cells(line)]
            index += 1  # delimiter row
            while index < len(lines) and lines[index].strip() and "|" in lines[index]:
                rows.append(_cells(lines[index]))
                index += 1
            out.add(table_block(rows, header_rows=1))
        elif item := _LIST_ITEM.match(line):
            out.flush()
            out.pending = [item.group(1)]
        else:
            out.pending.append(line.strip())
    out.flush()
    return ParsedDocument(blocks=out.blocks)
