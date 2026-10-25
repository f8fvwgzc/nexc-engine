---
id: eng-code-review
name: Code review
category: engineering
description: Reviews code changes for correctness, design, tests, security and readability following Google engineering practices, with severity-labelled comments and a clear approve or request-changes verdict.
keywords: code review, pull request, pr review, merge request, changelist, reviewer, review comments, nit, approve, request changes, readability, security review, diff
source: Google Engineering Practices - The standard of code review | https://google.github.io/eng-practices/review/reviewer/standard.html
source: Google Engineering Practices - What to look for in a code review | https://google.github.io/eng-practices/review/reviewer/looking-for.html
source: Google Engineering Practices - Small CLs | https://google.github.io/eng-practices/review/developer/small-cls.html
source: Conventional Comments | https://conventionalcomments.org/
---
## When to use
Reviewing a pull request, changelist or diff, or preparing your own change so it is easy to review.

## The standard
- Approve once the change clearly improves overall code health, even if it is not perfect. Never accept a change that makes the system's health worse.
- Facts and data beat opinion; the style guide decides style; where several designs are sound, the author's choice wins; otherwise match the existing code.
- Respond within one business day at most. Disagreement: discuss synchronously, record the outcome on the change, escalate to a tech lead if still stuck; do not let it stall.

## What to check, in order
1. Intent and size: read the description; is the change wanted and focused on one thing? About 100 lines is comfortable, about 1,000 is usually too big: ask for a split (stacked changes, by layer, refactor first).
2. Design: right place, fits the architecture, right abstraction level, no speculative generality.
3. Correctness: logic, edge cases (empty, null, zero, maximum, Unicode, time zones), error handling, concurrency (races, deadlocks, shared state), resource leaks, backward compatibility, reversible migrations, user-visible behaviour.
4. Tests: in the same change, would fail if the code broke, cover edge and error paths, not brittle or over-mocked.
5. Security: input validation and output encoding; injection (SQL, shell, template); authentication and authorisation on every path; no secrets in code or logs; safe deserialisation; new dependencies (licence, known CVEs); handling of personal data. Bring in a specialist when out of depth.
6. Readability: names, function size, comments that explain why, no dead code.
7. Style: automated by formatter and linter where possible; never mix reformatting with logic.
8. Docs: README, API reference, changelog and runbooks updated when behaviour changes.
Read every line you are asked to review, plus enough surrounding code for context. If you reviewed only part, say which part.

## Comment severity
| Label | Meaning | Blocks merge |
|---|---|---|
| issue (blocking) | bug, security hole, data loss, broken contract, logic without tests | yes |
| suggestion | a better approach, with the reason | author decides unless agreed |
| question | need an answer to judge | until answered |
| nitpick (nit) | minor polish or preference | no |
| praise | a good pattern worth repeating | no |
Write each comment as label (decoration): subject, then the reasoning, e.g. "issue (blocking): token is logged in plain text at auth.py:42". Comment on the code, not the person; say why and cite the principle or doc; offer a concrete alternative; keep must-fix separate from could-improve.

## Output
Verdict first: Approve, Approve with nits, Request changes, or Needs discussion. Then blocking items with file:line, the problem, why it matters and a fix; then non-blocking items; then what you did not review and follow-ups to file as tickets.

## Pitfalls
Rubber-stamping large diffs; arguing over style a linter should enforce; demanding rewrites for taste; reviewing only the diff and missing the call sites; burying a blocker among twenty nits.
