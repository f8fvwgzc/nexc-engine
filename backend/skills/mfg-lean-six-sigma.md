---
id: mfg-lean-six-sigma
name: Lean Six Sigma
category: manufacturing
description: Runs improvement projects with DMAIC and lean tools: charters, value stream maps, waste removal, kaizen and 5S, plus hypothesis tests, regression and DOE to prove root causes and gains.
keywords: lean six sigma, dmaic, value stream mapping, vsm, kaizen, 5s, muda, eight wastes, takt time, sipoc, hypothesis test, regression, design of experiments, doe, dpmo, oee
source: Lean Enterprise Institute - Value stream mapping | https://www.lean.org/lexicon-terms/value-stream-mapping/
source: Lean Enterprise Institute - Kaizen | https://www.lean.org/lexicon-terms/kaizen/
source: NIST/SEMATECH e-Handbook - What is design of experiments | https://www.itl.nist.gov/div898/handbook/pri/section1/pri11.htm
---
## When to use
Chronic defects, long lead times, low OEE, or any problem whose cause is unknown and needs data. Known-cause fixes go straight to a just-do-it action.

## DMAIC
- Define: charter with metric, baseline, gap, goal, scope in/out, benefit, team, dates; SIPOC; voice of customer to CTQ tree.
- Measure: operational definitions; MSA before baseline data; baseline yield, DPMO, Ppk; data collection plan; process map or VSM.
- Analyze: Pareto, fishbone, 5 Whys, multi-vari, hypothesis tests, regression; confirm each root cause with data.
- Improve: generate and screen options (Pugh matrix), DOE to set factors, poka-yoke, PFMEA on the new process, pilot.
- Control: control plan, SPC, standard work, reaction plan, owner handover, finance-signed benefits.
- Gate each phase with a tollgate review.

## Metrics
- DPMO = defects/(units x opportunities) x 10^6. RTY = product of step yields.
- Sigma level = Z of long-term yield + 1.5 (the conventional shift): 66,807 DPMO = 3, 6,210 = 4, 233 = 5, 3.4 = 6.
- Takt = available time/demand (27,000 s / 450 units = 60 s). Little's law: lead time = WIP/throughput. Process cycle efficiency = value-added time/lead time.
- OEE = availability (run time/planned production time) x performance (ideal cycle time x total count/run time) x quality (good count/total count); 85% is a commonly cited benchmark, not a standard.

## Lean tools
- Eight wastes (TIMWOODS): transport, inventory, motion, waiting, overproduction, overprocessing, defects, unused skills.
- VSM: one product family, door to door, material and information flow; data boxes C/T, C/O, uptime, %C&A, operators, shifts; inventory triangles; timeline of lead time vs processing time. Future state: produce to takt, continuous flow where possible, supermarket pull elsewhere, one pacemaker, heijunka with pitch = takt x pack quantity; then an implementation plan.
- Kaizen: PDCA; 3-5 day events with prepared data and a sponsor; standard work (takt, sequence, standard WIP); SMED (separate internal from external, convert, streamline).
- 5S: sort, set in order, shine, standardize, sustain; scored audits on a schedule.

## Statistics
- Test choice: two means -> Welch t; paired -> paired t; 3+ means -> one-way ANOVA then Tukey; variances -> Levene; proportions -> two-proportion or chi-square; skewed data -> Mann-Whitney, Kruskal-Wallis, Mood's median.
- Before collecting: H0/H1, alpha (0.05), power (0.80), minimum effect delta and sample size. Two means: n per group ~ 2((z_(1-alpha/2) + z_(1-beta)) x sigma/delta)^2, about 16 sigma^2/delta^2 at alpha 0.05 and power 0.80. Report effect size and confidence interval, not just p.
- Regression: adjusted R^2, residual checks (normality, constant variance, independence), VIF under 5-10, leverage points; correlation is not causation.
- DOE: 2^k full or 2^(k-p) fractional factorial (resolution III screens, IV separates main effects from two-factor interactions, V estimates them), randomize, replicate, block, add centre points for curvature; response surface (CCD, Box-Behnken) to optimize. Effect = mean(high) - mean(low).

## Output
A3 or DMAIC storyboard: problem, baseline chart, root-cause evidence, actions, before/after with significance, control plan, benefits.

## Pitfalls
Solution chosen in Define; skipping MSA; p-hacking; claiming savings without finance; no control phase, so gains decay.
