"""Tool results made smaller before they reach the model.

A tool loop sends every result back to the model on every later turn, so a noisy result is
paid for many times over. Before a result enters the conversation it loses what carries no
information: terminal colour codes, trailing spaces, runs of blank lines, the same line
repeated, and the middle of an output too long to read whole (its head and tail are kept,
since that is where commands print what they did and how they ended). File reads are left
exactly as they are: an agent may write back what it read.
"""

from __future__ import annotations

import re

#: Escape sequences terminals use for colour and cursor movement.
_ANSI = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]")

#: Longest result, in characters, that goes to the model whole.
RESULT_MAX_CHARS = 24_000

#: A line repeated this many times in a row is folded into one and a count.
REPEAT_MIN = 3

#: Tools whose results must reach the model byte for byte.
EXACT_TOOLS = frozenset({"read_file"})


def compact(text: str, limit: int = RESULT_MAX_CHARS) -> str:
    """`text` without what carries no information, at most about `limit` characters."""
    lines = [line.rstrip() for line in _ANSI.sub("", text).split("\n")]
    out: list[str] = []
    blank = 0
    i = 0
    while i < len(lines):
        line = lines[i]
        if not line:
            blank += 1
            if blank == 1:
                out.append(line)
            i += 1
            continue
        blank = 0
        j = i + 1
        while j < len(lines) and lines[j] == line:
            j += 1
        run = j - i
        if run >= REPEAT_MIN:
            out.extend([line, f"[… the line above, {run - 1} more times]"])
        else:
            out.extend([line] * run)
        i = j
    folded = "\n".join(out).strip("\n")
    if len(folded) <= limit:
        return folded
    return _head_and_tail(folded, limit)


def _head_and_tail(text: str, limit: int) -> str:
    """The first two thirds and the last third of `limit`, cut on lines where one is near."""
    marker_room = 80
    head_len = (limit - marker_room) * 2 // 3
    tail_len = limit - marker_room - head_len
    head = text[:head_len]
    cut = head.rfind("\n")
    if cut > head_len // 2:
        head = head[:cut]
    tail = text[-tail_len:]
    cut = tail.find("\n")
    if 0 <= cut < tail_len // 2:
        tail = tail[cut + 1 :]
    left_out = len(text) - len(head) - len(tail)
    return f"{head}\n[… {left_out} characters left out of the middle]\n{tail}"
