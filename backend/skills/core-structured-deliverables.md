---
id: core-structured-deliverables
name: Structured deliverables
category: core
description: Shapes any written output with the bottom line up front, clear headings, tables with units, an append-only decision log and consistent Markdown, and decides when the result belongs in a file.
keywords: bluf, bottom line up front, report, summary, executive summary, headings, tables, markdown, decision log, adr, deliverable, document, format, docx
source: Wikipedia - BLUF (communication), traced to Army Regulation 25-50 on correspondence | https://en.wikipedia.org/wiki/BLUF_(communication)
source: GitHub Flavored Markdown Spec 0.29-gfm | https://github.github.com/gfm/
source: Architectural Decision Records - decision log practice | https://adr.github.io/
---
## When to use
Every deliverable: notes, reports, memos, plans, reviews, handoffs to the next step.

## Method
1. BLUF. The first 3-5 lines answer the question: the conclusion or recommendation, the one or two figures that matter, the confidence, and any decision or action needed and by whom. A reader who stops there must not be misled. Pattern:
   ```text
   **Bottom line:** Switch to supplier B; it cuts landed cost 9 % (USD 410k/yr) [S2].
   **Confidence:** moderate (quotes are firm, volumes are forecasts).
   **Decision needed:** procurement lead approves by 2026-11-15.
   ```
   When the reader needs persuading, order the opening as situation, complication, question, answer (SCQA), then give the answer's supporting points.
2. Then a predictable body. Default order: Context (2-4 lines) > Findings or Analysis > Options with trade-offs > Recommendation > Risks and open questions > Next steps (owner, date) > Appendix (method, data, sources).
3. Headings carry meaning ("Costs rise 18 % in 2026", not "Analysis"). Use one title, then H2 sections and H3 at most; sentence case; no empty or decorative headings.
4. Paragraphs: one idea each, topic sentence first, active voice, short sentences. Bullets for parallel items, numbered lists for sequences, prose for argument.
5. Tables when three or more items share two or more attributes:
   - units and currency in the header ("Cost (USD k, 2026)"), not in every cell;
   - right-align numbers with `---:`; same decimals per column; a totals row when it helps;
   - escape a literal pipe as `\|`; keep to about 7 columns; split wide tables;
   - a Source column, or a source line under the table.
6. Decision log (ADR style), append-only. Each entry: ID, date (YYYY-MM-DD), status (proposed, accepted, rejected, superseded by D-n), context, decision, options considered, consequences, owner. Never rewrite an accepted entry; add a new one that supersedes it.
7. Markdown conventions (GFM): fenced code blocks with a language tag, `- [ ]` task lists for open actions, ISO 8601 dates, relative paths to workspace files, no raw HTML, no emoji, consistent number style (thousands separators, units with every quantity).

## When to deliver a file
- The step names a path or format: write exactly there (write_file for .md/.csv/.json, make_docx for a formal document).
- Output is long (over about one screen or 400 words), tabular data above about 20 rows, or another step must reuse it.
- Always return a short summary in the finish message with the file path(s) and what each contains; never only "see attached".

## Checklist
- Bottom line is in the first lines and matches the body.
- Every figure has a unit, a date basis and a source.
- Headings alone tell the story when skimmed.
- Assumptions, risks and next steps have their own sections.
- File written where the step asked; path stated in the reply.

## Pitfalls
- Burying the answer after pages of background.
- Walls of bullets with no hierarchy, or tables used for prose.
- Mixed date, number or unit formats in one document.
- Editing past decisions in place so the history is lost.
- Over-formatting short answers: a two-line reply needs no headings.
