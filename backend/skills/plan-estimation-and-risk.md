---
id: plan-estimation-and-risk
name: Estimation and risk
category: planning
description: Produces range estimates with three-point (PERT) maths and reference-class adjustments, keeps a RAID log, chooses risk responses, and sizes buffers; use when a plan needs dates, effort, cost or a risk view.
keywords: estimate, estimation, pert, three-point, reference class forecasting, optimism bias, risk, risk register, raid log, assumptions, issues, dependencies, contingency, buffer, probability, impact
source: GAO-16-89G Schedule Assessment Guide, best practice 8 schedule risk analysis | https://www.gao.gov/assets/d1689G.pdf
source: HM Treasury - Green Book supplementary guidance: optimism bias | https://www.gov.uk/government/publications/green-book-supplementary-guidance-optimism-bias
source: PMI - Planning for the planning fallacy (reference class forecasting) | https://www.pmi.org/learning/library/planning-fallacy-causes-solutions-project-expectations-6374
---
## When to use
Any plan that commits to effort, duration, cost or tokens, or must say what could go wrong.

## Three-point estimates (PERT)
For each step give O (optimistic), M (most likely), P (pessimistic).
- Mean E = (O + 4M + P) / 6; standard deviation s = (P - O) / 6. Triangular alternative: (O + M + P) / 3.
- Path total: sum the E values; sum the variances s^2 and take the square root. This assumes independent steps; correlated risks widen the range.
- P(finish by T) is about Phi((T - E_path) / s_path). Quote P50 and P80, not one number: under this normal approximation P50 = E_path, P80 = E_path + 0.84 s_path, P90 = E_path + 1.28 s_path.
- Example: O 2, M 4, P 12 days gives E = 5.0 days, s = 1.67 days.

## Reference-class forecasting
1. Pick a class of comparable past work, large enough for a distribution yet similar in kind.
2. Get the distribution of outcomes (actual vs planned ratio).
3. Place this job in it and adjust the inside-view estimate by the uplift that matches the confidence you need (e.g. the P80 overrun ratio). Example: past similar reports ran 1.3x plan at the median and 1.8x at P80, so a 10-day inside view becomes P50 13 days, P80 18 days.
The UK Green Book supplementary guidance on optimism bias (published 21 April 2013) asks appraisers to adjust estimates explicitly, raising costs and durations and lowering benefits, using data from past or similar projects. Reduce an uplift only when mitigations or better estimates are documented.

## RAID log
| Type | Meaning | Required fields |
|---|---|---|
| Risk | uncertain future event | cause > event > effect statement, probability 1-5, impact 1-5, score, response, owner, trigger, review date |
| Assumption | treated as true, unverified | how and by when it will be validated, impact if false |
| Issue | happening now | severity, action, owner, due date |
| Dependency | needed from outside the step | provider, need-by date, status |

Score = probability x impact; 15-25 high, 8-14 medium, 1-7 low (calibrate locally). Expected monetary value EMV = p x impact in money.

## Risk responses
- Threats: avoid (change the plan), mitigate (cut p or impact), transfer (contract, insurance), accept (active with a contingency, or passive), escalate (outside the team's authority).
- Opportunities: exploit, enhance, share, accept, escalate.
ISO 31000:2018 clause 6.5 frames treatment as choosing options, planning, checking effectiveness and judging residual risk; a revision is in drafting (verify locally).

## Buffers
- Contingency reserve for known risks: sum of EMVs, or P80 - P50 of the path.
- Management reserve for unknowns, held by the Planner, not inside steps.
- Put one buffer at the end of the critical path and feeding buffers where side branches join, rather than padding every step.
- For agent runs, reserve tokens and retries for the review loop.

## Output
Estimate table (step, O, M, P, E, s, basis), P50/P80 totals, RAID log, top 5 risks with responses, buffer sizes and the reason for each.

## Pitfalls
- Single-point estimates; anchoring on the first number heard.
- Hidden padding in each step; ignoring correlated risks.
- Recording issues as risks; a register no one reviews.
- Optimism about novel work with no reference class.
