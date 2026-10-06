"""HTML with the standard library parser: h1-h6, paragraphs / list items and tables."""

from __future__ import annotations

from dataclasses import dataclass, field
from html.parser import HTMLParser

from .model import Block, ParsedDocument, decode_text, heading, paragraph
from .tables import table_block

_SKIPPED = frozenset({"script", "style", "nav", "head", "noscript", "template", "svg"})
_HEADINGS = {f"h{level}": level for level in range(1, 7)}
# Tags that end the paragraph being collected.
_BLOCK_TAGS = frozenset(
    {
        "p", "li", "div", "section", "article", "main", "header", "footer", "aside", "ul", "ol",
        "dl", "dt", "dd", "blockquote", "pre", "figure", "figcaption", "form", "hr", "address",
        "details", "summary", "body", "html",
    }
)  # fmt: skip
_MAX_SPAN = 1000


@dataclass(slots=True)
class _Cell:
    header: bool
    rowspan: int
    colspan: int
    parts: list[str] = field(default_factory=list)


@dataclass(slots=True)
class _Table:
    rows: list[list[_Cell]] = field(default_factory=list)
    head_rows: set[int] = field(default_factory=set)  # indexes of rows inside <thead>
    in_head: bool = False
    in_caption: bool = False
    cell: _Cell | None = None
    nested: int = 0  # depth of tables inside a cell: their text is folded into that cell


def _span(attrs: list[tuple[str, str | None]], name: str) -> int:
    for key, value in attrs:
        if key == name and value and value.strip().isdigit():
            return max(1, min(_MAX_SPAN, int(value.strip())))
    return 1


def _grid(table: _Table) -> tuple[list[list[str]], int | None]:
    """Lay the cells out on a grid, repeating a value over its rowspan / colspan."""
    cells: dict[tuple[int, int], str] = {}
    for row_index, row in enumerate(table.rows):
        column = 0
        for cell in row:
            while (row_index, column) in cells:
                column += 1
            text = "".join(cell.parts)
            for down in range(min(cell.rowspan, len(table.rows) - row_index)):
                for across in range(cell.colspan):
                    cells.setdefault((row_index + down, column + across), text)
            column += cell.colspan
    width = max((column for _, column in cells), default=-1) + 1
    rows = [[cells.get((r, c), "") for c in range(width)] for r in range(len(table.rows))]

    header_rows = 0
    for row_index, row in enumerate(table.rows):
        is_header = row_index in table.head_rows or (bool(row) and all(c.header for c in row))
        if not is_header:
            break
        header_rows += 1
    return rows, header_rows or None


class _Extractor(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.blocks: list[Block] = []
        self._skip = 0
        self._buffer: list[str] = []
        self._heading: int | None = None
        self._table: _Table | None = None

    # --- text flow ---------------------------------------------------------------------------
    def _flush(self) -> None:
        text = "".join(self._buffer)
        self._buffer = []
        block = heading(text, self._heading) if self._heading else paragraph(text)
        if block is not None:
            self.blocks.append(block)

    def handle_data(self, data: str) -> None:
        if self._skip:
            return
        table = self._table
        if table is None or table.in_caption:
            self._buffer.append(data)
        elif table.cell is not None:
            table.cell.parts.append(data)

    # --- tags --------------------------------------------------------------------------------
    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag in _SKIPPED:
            self._skip += 1
        if self._skip:
            return
        if self._table is not None:
            self._table_start(self._table, tag, attrs)
        elif tag == "table":
            self._flush()
            self._table = _Table()
        elif tag in _HEADINGS:
            self._flush()
            self._heading = _HEADINGS[tag]
        elif tag in _BLOCK_TAGS:
            self._flush()
        elif tag == "br":
            self._buffer.append(" ")

    def handle_endtag(self, tag: str) -> None:
        if tag in _SKIPPED:
            self._skip = max(0, self._skip - 1)
            return
        if self._skip:
            return
        if self._table is not None:
            self._table_end(self._table, tag)
        elif tag in _HEADINGS:
            self._flush()
            self._heading = None
        elif tag in _BLOCK_TAGS:
            self._flush()

    def _table_start(self, table: _Table, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag == "table":
            table.nested += 1
        elif table.nested or tag not in ("tr", "td", "th", "thead", "caption"):
            if table.cell is not None:
                table.cell.parts.append(" ")  # keep words of adjacent inner elements apart
        elif tag == "thead":
            table.in_head = True
        elif tag == "caption":
            table.in_caption = True
        elif tag == "tr":
            table.cell = None
            table.rows.append([])
            if table.in_head:
                table.head_rows.add(len(table.rows) - 1)
        else:
            if not table.rows:  # <td> without <tr>
                table.rows.append([])
            table.cell = _Cell(tag == "th", _span(attrs, "rowspan"), _span(attrs, "colspan"))
            table.rows[-1].append(table.cell)

    def _table_end(self, table: _Table, tag: str) -> None:
        if tag == "table":
            if table.nested:
                table.nested -= 1
                return
            self._table = None
            rows, header_rows = _grid(table)
            block = table_block(rows, header_rows=header_rows)
            if block is not None:
                self.blocks.append(block)
        elif table.nested:
            return
        elif tag == "thead":
            table.in_head = False
        elif tag == "caption":
            table.in_caption = False
            self._flush()
        elif tag in ("td", "th", "tr"):
            table.cell = None

    def close(self) -> None:
        super().close()
        if self._table is not None:  # unclosed <table>
            self._table.nested = 0
            self._table_end(self._table, "table")
        self._flush()


def parse_html(data: bytes) -> ParsedDocument:
    extractor = _Extractor()
    extractor.feed(decode_text(data))
    extractor.close()
    return ParsedDocument(blocks=extractor.blocks)
