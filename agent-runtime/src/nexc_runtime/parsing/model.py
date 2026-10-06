"""Format-independent result of parsing a document: ordered heading / text / table blocks."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Literal

BlockKind = Literal["heading", "text", "table"]
MAX_HEADING_LEVEL = 6


class ParseError(Exception):
    """Base class; the message is short, human-readable and safe to return to the caller."""


class UnsupportedFormatError(ParseError):
    """The file type is not one the runtime can parse (HTTP 415)."""


class UnparsableDocumentError(ParseError):
    """The file type is supported but this file could not be read (HTTP 422)."""


@dataclass(frozen=True, slots=True)
class Block:
    kind: BlockKind
    text: str = ""
    level: int | None = None
    page: int | None = None
    rows: list[list[str]] | None = None
    header_rows: int | None = None


@dataclass(slots=True)
class ParsedDocument:
    pages: int | None = None
    blocks: list[Block] = field(default_factory=list)


def clean(text: str | None) -> str:
    """Collapse every run of whitespace to one space; drop NULs (PostgreSQL rejects them)."""
    if not text:
        return ""
    return " ".join(text.replace("\x00", "").split())


def heading(text: str | None, level: int, page: int | None = None) -> Block | None:
    cleaned = clean(text)
    if not cleaned:
        return None
    return Block("heading", cleaned, level=max(1, min(MAX_HEADING_LEVEL, level)), page=page)


def paragraph(text: str | None, page: int | None = None) -> Block | None:
    cleaned = clean(text)
    return Block("text", cleaned, page=page) if cleaned else None


def decode_text(data: bytes) -> str:
    """UTF-8 (BOM tolerated), UTF-16 when a BOM says so, else latin-1, which never fails."""
    if data.startswith((b"\xff\xfe", b"\xfe\xff")):
        try:
            return data.decode("utf-16")
        except UnicodeDecodeError:
            pass
    try:
        return data.decode("utf-8-sig")
    except UnicodeDecodeError:
        return data.decode("latin-1")
