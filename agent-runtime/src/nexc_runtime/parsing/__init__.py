"""Document parsing: bytes of an uploaded file -> ordered heading / text / table blocks.

One module per format; `parse_document` picks the parser from the file name, falling back to
the file's magic bytes.
"""

from __future__ import annotations

from .dispatch import SUPPORTED_EXTENSIONS, detect_format, parse_document
from .model import (
    Block,
    ParsedDocument,
    ParseError,
    UnparsableDocumentError,
    UnsupportedFormatError,
)

__all__ = [
    "SUPPORTED_EXTENSIONS",
    "Block",
    "ParseError",
    "ParsedDocument",
    "UnparsableDocumentError",
    "UnsupportedFormatError",
    "detect_format",
    "parse_document",
]
