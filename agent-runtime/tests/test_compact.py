"""Tool results are compacted before they reach the model."""

from __future__ import annotations

from nexc_runtime.compact import RESULT_MAX_CHARS, compact


def test_noise_goes_and_meaning_stays() -> None:
    raw = "\x1b[32mok\x1b[0m   \n\n\n\nstep 1\nstep 1\nstep 1\nstep 1\nstep 2\nstep 2\n  indented\n"
    assert compact(raw) == (
        "ok\n\nstep 1\n[… the line above, 3 more times]\nstep 2\nstep 2\n  indented"
    )


def test_short_text_is_unchanged() -> None:
    assert compact("a\n  b\nc") == "a\n  b\nc"


def test_a_long_result_keeps_its_head_and_tail() -> None:
    lines = [f"line {n}" for n in range(20_000)]
    out = compact("\n".join(lines))
    assert len(out) <= RESULT_MAX_CHARS
    assert out.startswith("line 0\nline 1\n")
    assert out.endswith("line 19999")
    assert "characters left out of the middle]" in out
    # Cut on whole lines on both sides of the marker.
    before, after = out.split("\n[…", 1)
    assert before.split("\n")[-1].startswith("line ")
    assert after.split("\n", 1)[1].startswith("line ")
