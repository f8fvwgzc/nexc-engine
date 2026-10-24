---
id: review-red-team-critique
name: Red-team critique
category: review
description: Stress-tests plans, proposals and arguments with steelmanning, assumption checks, pre-mortems, failure-mode and counter-example analysis, and reports the weaknesses that matter with tests and mitigations.
keywords: red team, red teaming, critique, pre-mortem, premortem, devil's advocate, steelman, assumptions, failure modes, stress test, challenge, adversarial review, what if analysis, cognitive bias
source: UK MOD DCDC - Red Teaming Handbook, 3rd edition (2021) | https://assets.publishing.service.gov.uk/government/uploads/system/uploads/attachment_data/file/1027158/20210625-Red_Teaming_Handbook.pdf
source: CIA - A Tradecraft Primer: Structured Analytic Techniques (2009) | https://www.cia.gov/resources/csi/books-monographs/a-tradecraft-primer
source: Pre-mortem | https://en.wikipedia.org/wiki/Pre-mortem
---
## When to use
Before committing to a plan, strategy, design, forecast or argument with high stakes or high uncertainty; when a team agreed too fast; when asked "what could go wrong?" or "poke holes in this".

## Stance
Critique the work, never the people. Steelman first: restate the goal and the strongest case for the plan, and get that right before attacking; beating a weak version proves nothing. Then be adversarial and specific. End constructively: every weakness gets a test or a mitigation.

## Techniques (choose 2-4 by time and stakes)
- Key assumptions check (CIA Tradecraft Primer, 2009; UK MOD Red Teaming Handbook, 3rd ed., 2021): list stated assumptions and hidden ones, signalled by words like "typically", "experience shows", "will have to". For each ask how valid it is and what happens if it is wrong; rank by likelihood of being wrong times impact.
- Pre-mortem (Gary Klein, 2007; steps as in the UK MOD handbook): assume the plan failed badly by a set date; define what failure looks like; name two to six immediate causes; for each, chain back cause by cause until you reach a decision the owner controls. That root is what to mitigate.
- Argument mapping: split each conclusion into assumptions, assertions (facts) and deductions; check that facts are sourced and deductions follow.
- Failure modes (FMEA style): for each step or component list failure mode, effect and detection; score severity, occurrence and detection 1-10; RPN = S x O x D ranks them (a ranking aid, not a measure; the AIAG & VDA FMEA Handbook of 2019 ranks by Action Priority instead, so follow it for automotive work).
- Outside view and counter-examples: find cases where similar plans failed; compare forecasts with the reference class and typical overruns.
- What-if and high-impact/low-probability: describe an unlikely event as already happened, trace plausible paths to it, and set indicators that would warn of it.
- Adversary and stakeholder view: how would a competitor, attacker, regulator or affected user exploit or resist this? Avoid mirror-imaging (assuming they think like you).
- Analysis of Competing Hypotheses (ACH): list every plausible explanation; build a matrix of evidence against hypotheses marked consistent, inconsistent or not applicable; drop evidence that fits all of them (no diagnostic value); favour the hypothesis with the least inconsistent evidence, not the most supporting; test how far the result hangs on a few items and what expected evidence is missing.

## Biases to look for
Confirmation, anchoring, optimism and the planning fallacy, sunk cost, groupthink, availability, survivorship, overconfidence in precise forecasts.

## Output
1. The strongest version of the plan in two or three lines.
2. Critical weaknesses ranked by impact and likelihood, each with reasoning or evidence, consequence, a test to check it, and a mitigation.
3. Assumptions table: assumption, confidence (high, medium, low), impact if wrong, how to validate.
4. Early-warning indicators and kill criteria.
5. What survived scrutiny, and an overall call: proceed, proceed with changes, or rethink.

## Pitfalls
Generic risks with no mechanism ("the market may change"); contrarianism without evidence; burying the two issues that matter under twenty small ones; a tone that makes owners defensive; mixing facts with speculation without labels.
