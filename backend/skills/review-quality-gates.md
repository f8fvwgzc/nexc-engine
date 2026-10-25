---
id: review-quality-gates
name: Quality gates
category: review
description: Runs a quality gate on a deliverable: checks it against the goal, the acceptance criteria and the definition of done, rates findings blocker, major, minor or nit, and issues a clear verdict.
keywords: quality gate, review, acceptance criteria, definition of done, severity, blocker, verdict, sign-off, qa review, deliverable review, go/no-go, inspection, audit
source: IEEE 1028-2008 - Standard for Software Reviews and Audits | https://standards.ieee.org/ieee/1028/4402/
source: ISO 19011:2026 - Guidelines for auditing management systems | https://committee.iso.org/standard/88984.html
source: Scrum.org - The 2020 Scrum Guide: Definition of Done | https://www.scrum.org/resources/blog/2020-scrum-guide-definition-done-created-scrum-team
source: Conventional Comments | https://conventionalcomments.org/
---
## When to use
Before a deliverable goes back to the Planner, ships, or reaches a stakeholder: documents, code, analyses, plans, designs.

## Inputs first
The goal (why the work exists and who uses it), the brief and acceptance criteria, the definition of done (DoD) for this kind of deliverable, applicable standards or style guide, earlier review findings. If criteria are missing, derive testable ones from the goal and state them as assumptions.

## Method
1. Goal: does the deliverable achieve the outcome its user needs? Meeting the brief while missing the goal fails the gate.
2. Criteria: mark each acceptance criterion Met, Partly met, Not met or Not verifiable, with evidence (section, line, figure, test).
3. DoD: complete (no TODOs or placeholders), correct (figures and facts checked), internally consistent, sources cited, security and privacy considered, formatted to standard, tests passing where code is involved.
4. Independent checks: recompute key numbers; sample citations; read or run the tests; compare the summary with the body and the tables with the text.
5. Rate each finding with the rubric; merge duplicates; group by root cause.
6. Issue the verdict.

## Review vocabulary
IEEE 1028-2008 (inactive-reserved since 2019, still the common reference) defines five review types: management review (progress against plan), technical review (fitness for purpose), inspection (formal defect hunt with roles and entry and exit criteria), walk-through (author-led) and audit (independent check against criteria). ISO 19011 sets the audit principles a gate should share: integrity, fair presentation, due professional care, confidentiality, independence, and evidence-based and risk-based approaches (as listed in the 2018 edition; the 4th edition of May 2026 replaced it, so verify wording locally).

## Severity rubric
| Severity | Definition | Gate effect |
|---|---|---|
| Blocker | goal not met; wrong result or decision likely; error in a key figure; safety, security, legal or privacy exposure; fabricated source; required section missing | fail until fixed |
| Major | acceptance criterion not or partly met; significant error or gap a user would notice; key claim unsupported | fail unless the owner accepts it with a dated fix |
| Minor | local error or unclear passage; conclusions unchanged | pass; fix if cheap |
| Nit | wording preference or typo that does not change meaning | pass; optional |
Rate by impact on the goal, not by effort to fix. Torn between two levels: pick the higher and say why.

## Verdict format
- First line: PASS, PASS WITH CONDITIONS or FAIL, plus one sentence why.
- Findings table: ID, severity, location, finding, evidence, required fix.
- Criteria table: criterion, status, evidence.
- What you did not check and why; residual risks; scope of the re-review.
Exit rule: PASS means no blockers and no open majors; PASS WITH CONDITIONS means no blockers and every major has an owner and a date; anything else is FAIL.

## Pitfalls
Grading effort instead of outcome; nits that drown the blockers; findings with no location or fix; rewriting the work instead of reviewing it; approving what you could not verify without saying so.
