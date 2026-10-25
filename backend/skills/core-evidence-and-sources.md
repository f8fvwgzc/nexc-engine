---
id: core-evidence-and-sources
name: Evidence and sources
category: core
description: Ties every material claim to a citable source, labels each statement as fact, assumption or judgement, attaches likelihood and confidence, and states what is unknown.
keywords: evidence, sources, citation, cite, fact, assumption, opinion, judgement, confidence, likelihood, uncertainty, unknowns, provenance, verify
source: ODNI - ICD 203 Analytic Standards (2015, as amended) | https://archive.dni.gov/files/documents/ICD/ICD-203.pdf
source: GRADE Working Group - GRADE Handbook, quality of evidence | https://gdt.gradepro.org/app/handbook/handbook.html
---
## When to use
Any deliverable that states facts, figures or conclusions another agent or a person will rely on. In nexc your evidence is the workspace files and upstream step results you can read; you cannot browse, so never cite a page you have not read in this run.

## Method
1. Keep a claim ledger. Each material claim gets a tag such as [S3] that resolves to a file path plus section, page, table or line, or to an upstream step id. A claim is material if a reader's decision would change were it false.
2. Label every statement with one type:
   - Fact: directly supported by a cited source you read; quote or paraphrase it faithfully.
   - Assumption: a supposition you use to frame the argument or bridge a gap. State it, say why it is reasonable, and say what changes if it is wrong. Flag the linchpin assumptions that carry the conclusion.
   - Judgement: your conclusion drawn from facts and assumptions. Show the reasoning chain.
   - Background: from your general training, not from a workspace source. Mark it "background, unverified" and keep it out of key figures.
3. Separate two kinds of uncertainty (ICD 203 sec. D.6.e(2)): the likelihood that an event happens, and your confidence in the basis of the judgement. A sentence carries a likelihood term or a confidence level, never both.
4. Grade confidence from the evidence base:
   - High: two or more independent, reliable, current sources agree and the evidence bears directly on the question.
   - Moderate: one reliable source, or several that are indirect, partly inconsistent or somewhat dated.
   - Low: a single weak, old, indirect or conflicted source, or mostly inference.
5. List unknowns: what you looked for and did not find, the evidence that would close each gap, and indicators that would change the judgement.

## Likelihood terms (ICD 203 bands)
| Term | Probability |
|---|---:|
| almost no chance / remote | 1-5 % |
| very unlikely | 5-20 % |
| unlikely | 20-45 % |
| roughly even chance | 45-55 % |
| likely | 55-80 % |
| very likely | 80-95 % |
| almost certain | 95-99 % |

ICD 203 also lists a parallel row (remote, highly improbable, improbable, roughly even odds, probable, highly probable, nearly certain); do not mix rows in one product. If you give a number, the word must match its band: 70 % is "likely", not "very likely".

Worked phrasing:
- "Supplier X will likely (55-80 %) miss the Q3 delivery [S2, S4]."
- "We have moderate confidence in this: two current internal reports agree, but neither covers the new plant."

## Checklist
- Every number has a source tag, a unit and an as-of date.
- Sources are independent (two reports quoting one press release count once).
- Quotes are exact and short; paraphrases keep the original meaning and hedges.
- Assumptions and judgements are visibly labelled, not blended into facts.
- Gaps and conflicting evidence are reported, not dropped.

## Output
Findings first, then an evidence table:
| # | Claim | Type | Source (file, section) | Source date | Confidence |
Close with "Unknowns and gaps" and "What would change this view".

## Pitfalls
- Fabricated or guessed citations, page numbers or URLs. If you cannot locate it, say so.
- Citation laundering: citing a summary as if it were the original study or dataset.
- Treating absence of evidence in the workspace as evidence of absence.
- False precision (12.347 %) on soft inputs; stale data presented as current.
- Hedging every sentence until no claim can be checked.
