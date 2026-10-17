"""Span ids, timestamps and digests for the `span` lines of the NDJSON stream (CONTRACT section 8).

The runtime only reports what its agents did. The backend decides which run, node and workspace
agent a span belongs to and computes the hash chain; nothing here is trusted by it.
"""

from __future__ import annotations

import hashlib
import json
import os
import threading
import time
import uuid
from datetime import UTC, datetime
from typing import Any


class _Clock:
    """Millisecond clock of the ids, with a counter for ids made within one millisecond."""

    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.last_ms = 0
        self.counter = 0

    def tick(self) -> tuple[int, int]:
        with self.lock:
            ms = time.time_ns() // 1_000_000
            if ms <= self.last_ms:
                ms = self.last_ms
                self.counter += 1
                if self.counter > 0xFFF:  # more than 4096 in one millisecond: borrow the next
                    ms += 1
                    self.counter = 0
            else:
                self.counter = 0
            self.last_ms = ms
            return ms, self.counter


_CLOCK = _Clock()


def new_span_id() -> str:
    """A UUID v7 (RFC 9562): 48-bit Unix milliseconds, then randomness.

    Ids made within one millisecond keep their order through a 12-bit counter.
    """
    ms, counter = _CLOCK.tick()
    rand = int.from_bytes(os.urandom(8), "big") & ((1 << 62) - 1)
    value = (ms & ((1 << 48) - 1)) << 80
    value |= 0x7 << 76  # version
    value |= counter << 64
    value |= 0b10 << 62  # variant
    value |= rand
    return str(uuid.UUID(int=value))


def now_ns() -> int:
    return time.time_ns()


def rfc3339_ns(ns: int) -> str:
    """`2026-10-17T12:00:00.123456789Z` (UTC, nanoseconds)."""
    seconds, fraction = divmod(ns, 1_000_000_000)
    stamp = datetime.fromtimestamp(seconds, UTC).strftime("%Y-%m-%dT%H:%M:%S")
    return f"{stamp}.{fraction:09d}Z"


def args_digest(arguments: Any) -> tuple[str, int]:
    """Hex SHA-256 and byte length of the arguments as canonical JSON (sorted keys, no spaces).

    The arguments themselves never leave the runtime in a span: only this digest does.
    """
    try:
        encoded = json.dumps(
            arguments, sort_keys=True, separators=(",", ":"), ensure_ascii=False
        ).encode("utf-8")
    except (TypeError, ValueError):
        encoded = repr(arguments).encode("utf-8", "replace")
    return hashlib.sha256(encoded).hexdigest(), len(encoded)
