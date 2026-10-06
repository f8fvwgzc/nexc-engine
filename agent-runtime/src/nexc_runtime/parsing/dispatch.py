"""Choose a parser by file extension, falling back to magic bytes, and run it."""

from __future__ import annotations

import io
import logging
import zipfile
from collections.abc import Callable
from pathlib import PurePosixPath

from .delimited import parse_csv, parse_tsv
from .docx import parse_docx
from .html import parse_html
from .markdown import parse_markdown
from .model import ParsedDocument, ParseError, UnparsableDocumentError, UnsupportedFormatError
from .pdf import parse_pdf
from .pptx import parse_pptx
from .text import parse_text
from .xlsx import parse_xlsx

log = logging.getLogger(__name__)

Parser = Callable[[bytes], ParsedDocument]

_PARSERS: dict[str, Parser] = {
    "pdf": parse_pdf,
    "docx": parse_docx,
    "xlsx": parse_xlsx,
    "pptx": parse_pptx,
    "html": parse_html,
    "csv": parse_csv,
    "tsv": parse_tsv,
    "markdown": parse_markdown,
    "text": parse_text,
}

SUPPORTED_EXTENSIONS: dict[str, str] = {
    ".pdf": "pdf",
    ".docx": "docx",
    ".xlsx": "xlsx",
    ".xlsm": "xlsx",
    ".pptx": "pptx",
    ".html": "html",
    ".htm": "html",
    ".xhtml": "html",
    ".csv": "csv",
    ".tsv": "tsv",
    ".md": "markdown",
    ".markdown": "markdown",
    ".txt": "text",
    ".text": "text",
    ".log": "text",
}

# Marker member of each zip-based Office format.
_OFFICE_MEMBERS = {
    "word/document.xml": "docx",
    "xl/workbook.xml": "xlsx",
    "ppt/presentation.xml": "pptx",
}
_PDF_MAGIC = b"%PDF"
_PDF_MAGIC_WINDOW = 1024  # the header may be preceded by a little junk
_ZIP_MAGIC = b"PK\x03\x04"
_TEXT_SAMPLE = 8192
_MAX_CONTROL_RATIO = 0.05


def _sniff_zip(data: bytes) -> str | None:
    try:
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            names = set(archive.namelist())
    except zipfile.BadZipFile:
        return None
    return next((kind for member, kind in _OFFICE_MEMBERS.items() if member in names), None)


def _looks_like_text(data: bytes) -> bool:
    sample = data[:_TEXT_SAMPLE]
    if sample.startswith((b"\xff\xfe", b"\xfe\xff")):  # UTF-16 with a BOM
        return True
    if b"\x00" in sample:
        return False
    control = sum(byte < 32 and byte not in (9, 10, 12, 13) for byte in sample)
    return control <= len(sample) * _MAX_CONTROL_RATIO


def detect_format(filename: str, data: bytes) -> str:
    """Format key for a file: by extension, then by magic bytes, then "any text is text"."""
    extension = PurePosixPath(filename.replace("\\", "/")).suffix.lower()
    if extension in SUPPORTED_EXTENSIONS:
        return SUPPORTED_EXTENSIONS[extension]
    if _PDF_MAGIC in data[:_PDF_MAGIC_WINDOW]:
        return "pdf"
    if data.startswith(_ZIP_MAGIC):
        if kind := _sniff_zip(data):
            return kind
    elif _looks_like_text(data):
        return "text"  # .json, .yaml, source code, no extension...
    label = f'"{extension}" files' if extension else "this file type"
    raise UnsupportedFormatError(f"{label} cannot be parsed")


def parse_document(filename: str, data: bytes) -> ParsedDocument:
    """Parse `data`; raises `UnsupportedFormatError` (415) or `UnparsableDocumentError` (422)."""
    if not data:
        raise UnparsableDocumentError("the file is empty")
    kind = detect_format(filename, data)
    try:
        return _PARSERS[kind](data)
    except ParseError:
        raise
    except Exception as error:
        # Library errors differ per format and may quote file content: log the type only.
        log.warning("could not parse a %s document: %s", kind, type(error).__name__)
        raise UnparsableDocumentError(f"the file could not be parsed as {kind}") from error
