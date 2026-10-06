"""CSV / TSV: the whole file is one table whose first row is the header."""

from __future__ import annotations

import csv
import io
from collections import Counter

from .model import ParsedDocument, decode_text
from .tables import table_block

_SAMPLE_LINES = 20
_DELIMITERS = ",;\t|"


def _delimiter(text: str) -> str:
    """The candidate that splits the first lines into the most consistent number of columns.

    (`csv.Sniffer` gives up on small or ragged files.)
    """
    sample = [line for line in text.splitlines()[: _SAMPLE_LINES * 2] if line.strip()]
    sample = sample[:_SAMPLE_LINES]
    best, best_score = ",", (0, 0)
    for candidate in _DELIMITERS:
        widths = Counter(len(row) for row in csv.reader(sample, delimiter=candidate))
        width, lines = max(widths.items(), key=lambda item: (item[1], item[0]), default=(1, 0))
        score = (lines, width) if width > 1 else (0, 0)
        if score > best_score:
            best, best_score = candidate, score
    return best


def parse_delimited(data: bytes, *, delimiter: str | None = None) -> ParsedDocument:
    text = decode_text(data)
    reader = csv.reader(io.StringIO(text, newline=""), delimiter=delimiter or _delimiter(text))
    block = table_block((row for row in reader if any(cell.strip() for cell in row)), header_rows=1)
    return ParsedDocument(blocks=[block] if block else [])


def parse_csv(data: bytes) -> ParsedDocument:
    return parse_delimited(data)


def parse_tsv(data: bytes) -> ParsedDocument:
    return parse_delimited(data, delimiter="\t")
