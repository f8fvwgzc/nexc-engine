---
id: research-search-strategy
name: Search strategy
category: research
description: Frames an answerable research question, builds concept-block keyword and boolean queries, prioritises primary over secondary sources, logs every search and decides when to stop at saturation.
keywords: research, search, search strategy, query, keywords, boolean, synonyms, pico, literature review, desk research, primary sources, secondary sources, saturation, search log
source: Cochrane Handbook v6.5.1, Chapter 4 Searching for and selecting studies (March 2025) | https://www.cochrane.org/authors/handbooks-and-manuals/handbook/current/chapter-04
source: Anthropic - How we built our multi-agent research system (2025) | https://www.anthropic.com/engineering/multi-agent-research-system
---
## When to use
Before reading anything at length: market and competitor scans, literature or policy reviews, fact checks, "find everything we have on X". In nexc the corpus is the workspace (list_files, read_file) and upstream results; the method is the same as for a database.

## Method
1. Frame the question so it can be answered.
   - Effects: PICO (population, intervention, comparison, outcome); add C for context (PICOC).
   - Experiences: PEO (population, exposure, outcome).
   - Business facts: entity + metric + geography + period + unit ("EU heat-pump unit sales, 2023-2025, units/yr").
   Write inclusion and exclusion criteria first: dates, geography, source types, language, definitions.
2. List 3-5 known items you expect to find. A strategy that misses them is too narrow.
3. Build concept blocks (usually 2-3 concepts). For each, collect synonyms, acronyms, US/UK spellings, plurals, trade and product names, codes (NAICS, HS, ticker). Combine synonyms with OR inside a block and blocks with AND; avoid NOT, which silently drops relevant items. Use truncation (analy*) and exact phrases where supported.
   Example: ("supplier delay*" OR "late deliver*" OR "lead time*") AND (Vietnam OR VN OR Hanoi OR "Ho Chi Minh") AND (2024 OR 2025).
4. Go broad first, then narrow. Aim for high recall with tolerable precision; screening a few extra items is cheaper than missing the key one.
5. Map the workspace before deep reading: list folders, read indexes, READMEs and tables of contents, then open the files that match the blocks.
6. Prefer primary sources: original data, filings, standards and regulations as published, contracts, minutes, logs, survey microdata. Use secondary sources (reviews, analyst and press reports) to locate primaries and for context; tertiary sources (encyclopaedias, textbooks) only for orientation.
7. Chase citations: backward (what a key document cites) and forward (later documents that refer to it). Many new hits from chasing means the main search was too narrow; revise it.
8. Log every search: query or path, date, hits, relevant hits, decision. For a formal review, report the strategy to PRISMA-S, which Cochrane chapter 4 points to.

## Stopping rules
Stop when any of these holds and say which:
- Saturation: two consecutive rounds of new terms or sources add no new relevant item or theme.
- All known items are found and citation chasing returns only items already seen.
- Budget limit reached; then list what was not searched.
Be slower to stop when evidence is scarce or the decision is high-stakes.

## Output
| # | Query or location | Date | Hits | Relevant | Notes |
Then the source list with type (primary, secondary, tertiary), inclusion decisions, and gaps: what was searched for and not found.

## Pitfalls
- Queries phrased to confirm the expected answer.
- One spelling or one term per concept.
- Stopping at the first plausible source; trusting a snippet or abstract without reading the source.
- Ignoring internal and grey material (reports, decks, minutes).
- No log, so the search cannot be repeated or reviewed.
