#!/usr/bin/env python3
"""Refreshes the generated tables of docs/AGENT-CATALOG.md from the catalog's files.

The expert roster comes from backend/templates/agent-*.json (every template outside the core
team), the skill library from backend/skills/*.md. Run from anywhere:

    python3 scripts/agent-catalog-doc.py          # rewrite the tables
    python3 scripts/agent-catalog-doc.py --check  # exit 1 when the page is out of date
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOC = ROOT / "docs" / "AGENT-CATALOG.md"
DEPARTMENTS = [
    "Finance & Risk",
    "IT, Cloud & Security",
    "Software, Data & AI",
    "Hardware & Manufacturing",
    "Supply Chain & Shipping",
    "Marketing & Sales",
    "Business, Legal & People",
]


def cell(text: str) -> str:
    return text.replace("|", "\\|").replace("\n", " ").strip()


def skills() -> list[dict]:
    out = []
    for path in sorted((ROOT / "backend" / "skills").glob("*.md")):
        lines = path.read_text(encoding="utf-8").split("\n")
        end = next(i for i in range(1, len(lines)) if lines[i].strip() == "---")
        meta: dict = {"source": []}
        for line in lines[1:end]:
            if ":" in line:
                key, value = line.split(":", 1)
                if key.strip() == "source":
                    meta["source"].append(value.strip())
                else:
                    meta[key.strip()] = value.strip()
        out.append(meta)
    return out


def roster() -> str:
    templates = [
        json.loads(p.read_text(encoding="utf-8"))
        for p in sorted((ROOT / "backend" / "templates").glob("agent-*.json"))
    ]
    experts = [t for t in templates if t.get("department") != "Core team"]
    rows = []
    for dept in DEPARTMENTS:
        members = sorted((t for t in experts if t["department"] == dept), key=lambda t: t["name"])
        if not members:
            continue
        rows.append(f"### {dept}\n")
        rows.append("| Agent | Role | Benchmarked against | Skills |")
        rows.append("| ----- | ---- | ------------------- | ------ |")
        for t in members:
            creds = "; ".join(cell(c) for c in t["credentials"])
            sk = ", ".join(f"`{s}`" for s in t["skills"])
            rows.append(f"| **{cell(t['name'])}**<br>{cell(t['title'])} | `{t['role']}` | {creds} | {sk} |")
        rows.append("")
    return "\n".join(rows).rstrip() + "\n"


def library() -> str:
    all_skills = skills()
    by_cat: dict[str, list[dict]] = {}
    for s in all_skills:
        by_cat.setdefault(s["category"], []).append(s)
    rows = [f"{len(all_skills)} skills in {len(by_cat)} categories.\n"]
    for cat in sorted(by_cat):
        rows.append(f"### {cat}\n")
        rows.append("| Skill | What it does | Sources |")
        rows.append("| ----- | ------------ | ------- |")
        for s in sorted(by_cat[cat], key=lambda s: s["id"]):
            links = ", ".join(
                f"[{cell(src.rsplit(' | ', 1)[0])}]({src.rsplit(' | ', 1)[1].strip()})" for src in s["source"]
            )
            rows.append(f"| **{cell(s['name'])}**<br>`{s['id']}` | {cell(s['description'])} | {links} |")
        rows.append("")
    return "\n".join(rows).rstrip() + "\n"


def fill(text: str, region: str, body: str) -> str:
    pattern = re.compile(rf"(<!-- #region {region} -->\n).*?(<!-- #endregion {region} -->)", re.S)
    if not pattern.search(text):
        raise SystemExit(f"region {region} not found in {DOC}")
    return pattern.sub(lambda m: m.group(1) + body + m.group(2), text, count=1)


def main() -> int:
    text = DOC.read_text(encoding="utf-8")
    new = fill(fill(text, "roster", roster()), "skills", library())
    if "--check" in sys.argv[1:]:
        if new != text:
            print(f"{DOC.relative_to(ROOT)} is out of date: run python3 scripts/agent-catalog-doc.py")
            return 1
        return 0
    DOC.write_text(new, encoding="utf-8")
    print(f"wrote {DOC.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
