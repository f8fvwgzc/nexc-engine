---
id: review-fact-checking
name: Fact-checking
category: review
description: Verifies the factual claims in a deliverable: extracts checkable claims, traces numbers, quotes and citations to primary sources, flags signs of fabricated references and labels what cannot be verified.
keywords: fact-checking, fact check, verification, citations, references, sources, claims, statistics, quotes, hallucination, fabricated references, doi, misinformation, accuracy
source: IFCN Code of Principles - The commitments | https://ifcncodeofprinciples.poynter.org/the-commitments
source: Writing Commons - SIFT: Stop, Investigate the Source, Find Better Coverage, Trace Claims | https://writingcommons.org/section/information-literacy/information-evaluation/sift-stop-investigate-source-find-better-coverage-trace-claims/
source: Crossref - Retractions | https://crossref.org/categories/retractions
source: CIA - A Tradecraft Primer: Structured Analytic Techniques | https://www.cia.gov/resources/csi/books-monographs/a-tradecraft-primer
---
## When to use
Before any document with facts, figures, quotes or citations ships; whenever a source is unnamed, a number is surprising, or references were produced by a model.

## Method
1. Extract claims: list every checkable statement (numbers, dates, names, causal claims, quotes, "first/largest/only", legal or regulatory statements, citations). Flag opinions dressed as facts. Rank by consequence if wrong and by surprise.
2. Classify support: cited, traceable (source exists in the workspace or upstream results), or unsupported.
3. Trace to the primary source (SIFT: stop, investigate the source, find better coverage, trace claims to their original context): the dataset, filing, standard, statute, paper or transcript, not a summary. Check it says exactly this, for the same population, period, unit and definition.
4. Numbers: recompute derived figures (shares, sums, growth; CAGR = (end / start)^(1 / years) - 1); check units, currency, nominal versus real, base year, denominators, percent versus percentage points, rounding; check text, tables and charts agree.
5. Quotes: verbatim against the original; right speaker, date and context; no ellipsis that changes meaning; paraphrase labelled as such.
6. Citations: author, title, venue, year, volume, pages and DOI or URL agree with each other; the work supports this specific claim; check retraction status where data is available (Crossref opened the Retraction Watch database in September 2023 and has served it through its REST API since January 2025).
7. Check every key element of a claim against more than one named source unless only one relevant source exists; prefer the best available primary source and say why when you fall back on a secondary one (IFCN Code of Principles, sources commitment).
With no web access, check against the workspace and upstream results; anything you cannot reach is Unverified, not False.

## Signs of a fabricated reference
- DOI that resolves to a different title or authors, or a journal, volume or issue that did not exist that year.
- Real authors attached to a plausible title that appears nowhere else; page ranges that do not fit the venue.
- URLs with believable paths on real domains but no matching content.
- Over-precise statistics with no method, or vague attributions ("studies show", "experts say").
Any one sign makes the reference Unverified until checked. Never repair a suspect citation by inventing a better one.

## Ratings
| Rating | Meaning |
|---|---|
| Verified | primary source matches the claim |
| Partly verified | source supports a weaker or different claim; give the correct wording |
| Unverified | could not trace; say what was tried |
| False | source contradicts; give the correction and evidence |
| Outdated | superseded figure, edition or law |

## Output
Summary first: counts by rating and the blockers (false or unverified claims the conclusions rest on). Then a table: claim, location, rating, source checked, correction or note. For each problem recommend correct, cite, soften ("about", "reported by") or remove.

## Pitfalls
Confirming a source exists but not that it supports the claim; citing secondary coverage as proof; one source for a key figure; treating unverifiable as true or as false; correcting silently without telling the author.
