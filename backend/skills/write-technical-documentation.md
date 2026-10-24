---
id: write-technical-documentation
name: Technical documentation
category: writing
description: Plans and writes technical documentation with the Diataxis model (tutorial, how-to, reference, explanation), a docs-as-code workflow, tested API reference examples and versioned docs.
keywords: technical documentation, docs, diataxis, tutorial, how-to guide, reference, explanation, readme, api reference, docs as code, developer docs, versioning, changelog, runbook
source: Diataxis - A systematic approach to technical documentation | https://diataxis.fr/
source: Diataxis - The compass | https://diataxis.fr/compass/
source: Diataxis - How-to guides | https://diataxis.fr/how-to-guides/
source: Write the Docs - Docs as Code | https://www.writethedocs.org/guide/docs-as-code/
source: Google developer documentation style guide - Highlights | https://developers.google.com/style/highlights
---
## When to use
READMEs, user and developer guides, API references, runbooks, architecture explanations, migration guides and release notes.

## Diataxis types
Two questions pick the type: does the reader need to act or to understand, and are they learning or working?
| Type | Reader need | Rules |
|---|---|---|
| Tutorial | learn by doing | one path that always works; concrete steps with a visible result each; minimal explanation (link out); no choices; test on a clean setup |
| How-to guide | finish a real task, already competent | title "How to <goal>"; prerequisites; numbered steps; conditional imperatives ("If you use X, set Y"); no teaching; link to reference for options |
| Reference | look up facts while working | structure mirrors the product (one entry per command, endpoint, config key); same template everywhere; complete, neutral; generated from source where possible |
| Explanation | understand why | context, design decisions, trade-offs, alternatives; no procedures |
Never mix types on one page: split and link. Apply the two questions to paragraphs while editing; a "why" digression inside a how-to belongs in an explanation page.

## Method
1. Audience and tasks: list who reads and what they try to do; classify existing pages by type; list gaps.
2. Architecture: a landing page that routes each reader to a start point; sections by product area with the four types inside.
3. Writing style (Google developer style guide): second person, active voice, present tense, sentence-case headings, conditions before instructions, code in code font, UI labels in bold, descriptive link text. One action per step plus its expected result.
4. Examples: each reference entry has a minimal runnable example and a realistic one; show request and response; source examples from tests or run them in CI (doctest, snippet tests) so they cannot rot.
5. Docs as code: Markdown, reStructuredText or AsciiDoc in the product repo; changes through pull requests and review; CI runs build, link check, spell check and a prose linter (for example Vale); "docs updated" is part of the definition of done for user-facing changes.
6. Versioning: docs follow product versions (semantic versioning); version switcher; mark "Added in 2.3" and "Deprecated in 3.0, removed in 4.0"; changelog grouped as Added, Changed, Deprecated, Removed, Fixed, Security; a migration guide for every breaking change.

## API reference entry
Summary line; method and path or signature; parameters table (name, type, required, default, constraints, meaning); request schema; responses per status code with example bodies; errors and how to fix them; authentication and scopes; rate limits; idempotency; examples in curl and one SDK; version added or deprecated. Generate the skeleton from OpenAPI and hand-write the narrative.

## Checklist
- One Diataxis type per page, and the title says which task or topic.
- Prerequisites, versions and platforms stated.
- Every command and code block runs as shown; placeholders such as <PROJECT_ID> are marked and explained.
- No "simply", "just" or "easy"; links resolve; images have alt text.

## Output
- A doc plan: a table of pages (title, Diataxis type, reader, task, owner, status) with the gaps ranked by reader impact.
- Or the page itself in Markdown, then a note: type, audience, product version, which examples were run and how, and open questions for the owner.

## Pitfalls
- Tutorials that branch into options; how-tos that teach; reference pages full of opinion.
- Documenting screens instead of the user's goal; screenshots that age.
- Docs outside the review path, so they drift from the code.
