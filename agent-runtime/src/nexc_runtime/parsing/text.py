"""Plain text and anything text-like (logs, JSON, YAML, source code): one block per paragraph."""

from __future__ import annotations

import re

from .model import ParsedDocument, decode_text, paragraph

_BLANK_LINES = re.compile(r"\n[ \t]*(?:\n[ \t]*)+")


def parse_text(data: bytes) -> ParsedDocument:
    text = decode_text(data).replace("\r\n", "\n").replace("\r", "\n")
    blocks = [block for part in _BLANK_LINES.split(text) if (block := paragraph(part))]
    return ParsedDocument(blocks=blocks)
