---
id: audit-controls-testing
name: Controls testing
category: audit
description: Tests internal controls against COSO 2013 and SOX 404: top-down scoping, walkthroughs, design vs operating effectiveness, sample sizes, evidence standards and deficiency evaluation.
keywords: controls testing, internal controls, coso 2013, sox 404, icfr, walkthrough, test of design, operating effectiveness, sample size, audit sampling, control deficiency, material weakness, rcm, ipe
source: COSO - Internal Control | https://www.coso.org/internal-control
source: Anthropic knowledge-work-plugins - audit-support skill (SOX 404 testing) | https://raw.githubusercontent.com/anthropics/knowledge-work-plugins/main/finance/skills/audit-support/SKILL.md
source: Weaver - COSO framework 17 principles of effective internal control | https://weaver.com/resources/coso-frameworks-17-principles-effective-internal-control/
---
## When to use
SOX 404 or ICFR programmes, operational control reviews, building a risk and control matrix (RCM), writing test scripts, or judging how severe a control failure is.

## Framework
- COSO Internal Control - Integrated Framework (2013): 5 components, 17 principles. Control environment: 1 integrity and ethics, 2 independent board oversight, 3 structure and authority, 4 competence, 5 accountability. Risk assessment: 6 clear objectives, 7 identify and analyse risk, 8 fraud risk, 9 significant change. Control activities: 10 select controls, 11 technology general controls, 12 policies and procedures. Information and communication: 13 quality information, 14 internal, 15 external. Monitoring: 16 ongoing and separate evaluations, 17 evaluate and communicate deficiencies. All must be present, functioning and operating together.
- SOX 404(a) management assessment; 404(b) auditor attestation for accelerated and large accelerated filers; PCAOB AS 2201 top-down, risk-based approach.

## Scoping
Materiality, then significant accounts and disclosures (size and qualitative factors), relevant assertions (existence or occurrence, completeness, valuation, rights and obligations, presentation), processes and locations, what can go wrong, then key controls (entity-level, process, ITGC, automated, reports used as evidence).

## Design vs operating effectiveness
- Walkthrough yearly or after change: trace one item end to end with inquiry, observation, inspection and reperformance.
- Design attributes: objective, risk and assertion, frequency, preventive or detective, manual or automated, precision (threshold that triggers follow-up), performer authority, evidence, reports relied on.
- Operating tests: inquiry alone never suffices; combine with inspection, observation or reperformance (strongest).
- Automated control: one instance per period if ITGCs over the system are effective. IT-dependent manual control: test the review and the completeness and accuracy of its report (IPE).

## Sample sizes (common practice; firm methodologies vary)
| Frequency | Low risk | High risk |
|---|---|---|
| Annual | 1 | 1 |
| Quarterly | 2 | 3 |
| Monthly | 2 | 4-5 |
| Weekly | 5 | 15 |
| Daily (~250) | 20 | 40 |
| Many per day (250+) | 25 | 60 |
Statistical attribute sampling with zero expected deviations and 5% tolerable rate: about 59 items at 95% confidence, 45 at 90% (verify against firm tables). Raise sizes for sole key controls, prior deficiencies, new controls or external auditor reliance. One exception: extend the sample or conclude, and find the cause; never discard it as an anomaly without evidence.
Interim testing needs a roll-forward to year end; remediated controls need enough operating time before retesting.

## Evidence
Sufficient, appropriate, dated and attributable to a named performer or approver; screenshots with system date; populations proven complete; inquiry corroborated.

## Deficiency evaluation
Deficiency, significant deficiency (merits governance attention), material weakness (reasonable possibility a material misstatement is not prevented or detected on time). Weigh likelihood, magnitude, compensating controls and aggregation. Indicators of a material weakness (AS 2201.69): senior management fraud, restatement, auditor-found material misstatement, ineffective audit committee oversight.

## Output
Workpaper per control: ID, description, risk and assertion, test objective, population and completeness check, sample method and size, attributes, results, exceptions, conclusion, preparer and reviewer sign-off.

## Pitfalls
Reports used without IPE testing; samples from incomplete populations; reviews without precision; inquiry-only tests; late remediation.
