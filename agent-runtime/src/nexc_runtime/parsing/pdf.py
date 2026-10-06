"""PDF via pdfplumber: ruled tables, and text lines grouped into headings and paragraphs.

Deterministic heuristics only, no OCR: a page without a text layer contributes no blocks.
"""

from __future__ import annotations

import io
from collections import Counter
from dataclasses import dataclass
from typing import Any

import pdfplumber
from pdfminer.pdfdocument import PDFEncryptionError, PDFPasswordIncorrect
from pdfplumber.page import Page
from pdfplumber.table import Table
from pdfplumber.utils.exceptions import PdfminerException

from .model import Block, ParsedDocument, UnparsableDocumentError, clean, heading, paragraph
from .tables import table_block

_BBox = tuple[float, float, float, float]

_LARGER = 1.15  # a line this much larger than the body text is a heading
_LEVEL_1 = 1.7
_LEVEL_2 = 1.35
_RUN_IN_LEVEL = 4  # bold or all-caps line at body size
_MAX_HEADING_CHARS = 120
_MAX_RUN_IN_CHARS = 80
_PARAGRAPH_GAP = 0.75  # of the font size: a larger vertical gap starts a new paragraph
_SPAN_SLACK = 1.0  # points
_BOLD_MARKERS = ("bold", "black", "heavy", "semibold", "demi")


@dataclass(slots=True)
class _Line:
    text: str
    top: float
    bottom: float
    size: float
    bold: bool


@dataclass(slots=True)
class _PageContent:
    number: int
    lines: list[_Line]
    tables: list[tuple[float, Block]]  # (top, table block)


def _is_bold(fontname: object) -> bool:
    name = str(fontname).lower()
    return any(marker in name for marker in _BOLD_MARKERS)


def _table_rows(table: Table) -> list[list[str | None]]:
    """Cell texts with merged cells repeated over the grid positions they span."""
    rows = table.extract()
    cells: list[_BBox] = list(table.cells)
    lefts = sorted({cell[0] for cell in cells})
    tops = sorted({cell[1] for cell in cells})
    for x0, top, x1, bottom in cells:
        row_index, column = tops.index(top), lefts.index(x0)
        value = rows[row_index][column]
        if not value:
            continue
        for down in range(row_index, len(tops)):
            if down > row_index and tops[down] >= bottom - _SPAN_SLACK:
                break
            for across in range(column, len(lefts)):
                if across > column and lefts[across] >= x1 - _SPAN_SLACK:
                    break
                if rows[down][across] is None:  # no cell of its own: covered by this one
                    rows[down][across] = value
    return rows


def _inside(word: dict[str, Any], boxes: list[_BBox]) -> bool:
    x = (word["x0"] + word["x1"]) / 2
    y = (word["top"] + word["bottom"]) / 2
    return any(x0 <= x <= x1 and top <= y <= bottom for x0, top, x1, bottom in boxes)


def _make_line(words: list[dict[str, Any]]) -> _Line:
    words.sort(key=lambda word: word["x0"])
    sizes: Counter[float] = Counter()
    for word in words:
        sizes[round(float(word.get("size") or 0.0) * 2) / 2] += len(word["text"])
    return _Line(
        text=clean(" ".join(word["text"] for word in words)),
        top=min(word["top"] for word in words),
        bottom=max(word["bottom"] for word in words),
        size=sizes.most_common(1)[0][0],
        bold=all(_is_bold(word.get("fontname")) for word in words),
    )


def _lines(words: list[dict[str, Any]]) -> list[_Line]:
    """Group words into visual lines: a word joins the line its vertical middle falls into."""
    lines: list[_Line] = []
    current: list[dict[str, Any]] = []
    bottom = 0.0
    for word in sorted(words, key=lambda word: (word["top"], word["x0"])):
        if current and (word["top"] + word["bottom"]) / 2 > bottom:
            lines.append(_make_line(current))
            current = []
        if not current:
            bottom = word["bottom"]
        current.append(word)
    if current:
        lines.append(_make_line(current))
    return [line for line in lines if line.text]


def _read_page(page: Page, number: int) -> _PageContent:
    tables: list[tuple[float, Block]] = []
    boxes: list[_BBox] = []
    try:
        found = page.find_tables()
    except Exception:  # odd vector graphics must not lose the page's text
        found = []
    for table in found:
        if len(table.cells) < 2:  # a frame around text, not a table
            continue
        block = table_block(_table_rows(table), page=number)
        if block is not None:
            tables.append((table.bbox[1], block))
            boxes.append(table.bbox)
    words = page.extract_words(extra_attrs=["fontname", "size"])
    lines = _lines([word for word in words if not _inside(word, boxes)])
    return _PageContent(number, lines, tables)


def _body_style(pages: list[_PageContent]) -> tuple[float, bool]:
    """The dominant font size of the document and whether its running text is bold."""
    sizes: Counter[float] = Counter()
    bold = 0
    total = 0
    for page in pages:
        for line in page.lines:
            sizes[line.size] += len(line.text)
            total += len(line.text)
            bold += len(line.text) if line.bold else 0
    size = sizes.most_common(1)[0][0] if sizes else 0.0
    return size, total > 0 and bold * 2 > total


def _heading_level(line: _Line, body_size: float, body_bold: bool) -> int | None:
    text = line.text
    letters = sum(char.isalpha() for char in text)
    if not letters or len(text) > _MAX_HEADING_CHARS:
        return None
    if body_size > 0 and line.size >= body_size * _LARGER:
        ratio = line.size / body_size
        return 1 if ratio >= _LEVEL_1 else 2 if ratio >= _LEVEL_2 else 3
    if len(text) > _MAX_RUN_IN_CHARS or text.endswith((".", ",", ";")):
        return None
    if line.bold and not body_bold:
        return _RUN_IN_LEVEL
    solid = len(text.replace(" ", ""))
    if letters >= 4 and text.isupper() and letters * 10 >= solid * 6:
        return _RUN_IN_LEVEL
    return None


def _join(parts: list[str]) -> str:
    """Join the lines of a paragraph, undoing end-of-line hyphenation."""
    text = ""
    for part in parts:
        if len(text) > 1 and text.endswith("-") and text[-2].isalpha() and part[:1].islower():
            text = text[:-1] + part
        else:
            text = f"{text} {part}" if text else part
    return text


def _page_blocks(page: _PageContent, body_size: float, body_bold: bool) -> list[Block]:
    blocks: list[Block | None] = []
    pending: list[str] = []
    pending_level: int | None = None
    previous: _Line | None = None

    def flush() -> None:
        nonlocal pending, pending_level
        if pending:
            text = _join(pending)
            blocks.append(
                heading(text, pending_level, page.number)
                if pending_level
                else paragraph(text, page.number)
            )
        pending, pending_level = [], None

    flow: list[tuple[float, _Line | Block]] = [(line.top, line) for line in page.lines]
    flow += page.tables
    for _, item in sorted(flow, key=lambda entry: entry[0]):
        if isinstance(item, Block):
            flush()
            blocks.append(item)
            previous = None
            continue
        level = _heading_level(item, body_size, body_bold)
        gap = item.top - previous.bottom if previous is not None else 0.0
        same_block = (
            previous is not None
            and level == pending_level
            and abs(item.size - previous.size) < 0.6
            and gap <= _PARAGRAPH_GAP * max(previous.size, 1.0)
        )
        if not same_block:
            flush()
            pending_level = level
        pending.append(item.text)
        previous = item
    flush()
    return [block for block in blocks if block is not None]


def _open_error(error: PdfminerException) -> UnparsableDocumentError:
    cause = error.args[0] if error.args else None
    if isinstance(cause, PDFPasswordIncorrect | PDFEncryptionError):
        return UnparsableDocumentError("the PDF is password-protected")
    return UnparsableDocumentError("the PDF is corrupt or not a PDF")


def parse_pdf(data: bytes) -> ParsedDocument:
    try:
        with pdfplumber.open(io.BytesIO(data)) as pdf:
            pages: list[_PageContent] = []
            for number, page in enumerate(pdf.pages, start=1):
                pages.append(_read_page(page, number))
                page.close()  # drop the page's cached objects: keeps 100-page files small
    except PdfminerException as error:
        raise _open_error(error) from error
    body_size, body_bold = _body_style(pages)
    blocks = [block for page in pages for block in _page_blocks(page, body_size, body_bold)]
    return ParsedDocument(pages=len(pages), blocks=blocks)
