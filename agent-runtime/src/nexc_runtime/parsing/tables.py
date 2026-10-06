"""Table helpers shared by every format: rectangular matrices and header-row estimation."""

from __future__ import annotations

import re
from collections.abc import Iterable, Sequence

from .model import Block, clean

MAX_HEADER_ROWS = 4
# 12 / 1,234.5 / -3% / (1 200) / $10 / 10 EUR-less amounts. Plain years are labels, not data.
_NUMERIC = re.compile(r"^[(+\-−]?\s?[$€£¥₮]?\s?\d[\d\s.,]*\s?%?\)?$")  # noqa: RUF001
_YEAR = re.compile(r"^(19|20)\d{2}$")


def normalize_rows(rows: Iterable[Sequence[str | None]]) -> list[list[str]]:
    """Clean every cell and pad short rows with "" so the matrix is rectangular."""
    cleaned = [[clean(cell) for cell in row] for row in rows]
    width = max((len(row) for row in cleaned), default=0)
    return [row + [""] * (width - len(row)) for row in cleaned]


def drop_empty(rows: list[list[str]]) -> list[list[str]]:
    """Remove rows and columns that have no content at all."""
    kept = [row for row in rows if any(row)]
    if not kept:
        return []
    columns = [index for index in range(len(kept[0])) if any(row[index] for row in kept)]
    return [[row[index] for index in columns] for row in kept]


def is_numeric(cell: str) -> bool:
    return bool(_NUMERIC.match(cell)) and not _YEAR.match(cell)


def _header_like(row: Sequence[str]) -> bool:
    return any(row) and not any(is_numeric(cell) for cell in row)


def _continues_header(above: Sequence[str], row: Sequence[str]) -> bool:
    """Does `row` look like a second header level under `above`?

    Signals: a group label spanning columns in the row above (repeated across the merged span, or
    followed by blanks that this row fills in), or a label merged down into this row.
    """
    seen_label = False
    for index, (top, cell) in enumerate(zip(above, row, strict=True)):
        if top and index > 0 and top == above[index - 1]:
            return True  # merged group label, repeated
        if not top and cell and seen_label:
            return True  # merged group label, not repeated: the sub-header fills the gap
        seen_label = seen_label or bool(top)
    return list(above) != list(row) and any(
        top and top == cell for top, cell in zip(above, row, strict=True)
    )  # label merged vertically across both header rows


def estimate_header_rows(rows: Sequence[Sequence[str]]) -> int:
    """Best guess of how many leading rows are headers: at least 1 for a non-empty table."""
    if not rows:
        return 0
    count = 1
    limit = min(MAX_HEADER_ROWS, len(rows) - 1)  # always leave one data row
    while count < limit and _header_like(rows[count]):
        if not _continues_header(rows[count - 1], rows[count]):
            break
        count += 1
    return count


def table_block(
    rows: Iterable[Sequence[str | None]],
    *,
    page: int | None = None,
    header_rows: int | None = None,
    compact: bool = False,
) -> Block | None:
    """Build a table block, or None when the table has no content.

    `header_rows` is used when the format states it (HTML `<th>`, CSV); otherwise it is estimated.
    `compact` also removes fully empty rows and columns.
    """
    matrix = normalize_rows(rows)
    if compact:
        matrix = drop_empty(matrix)
    if not any(any(row) for row in matrix):
        return None
    if header_rows is None or header_rows < 1:
        header_rows = estimate_header_rows(matrix)
    return Block("table", page=page, rows=matrix, header_rows=min(header_rows, len(matrix)))
