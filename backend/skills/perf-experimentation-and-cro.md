---
id: perf-experimentation-and-cro
name: Experimentation and CRO
category: marketing
description: Plans and reads A/B tests and conversion rate optimisation: hypotheses, sample size and MDE, significance and the peeking problem, sample ratio checks, CRO heuristics and landing page tests.
keywords: a/b testing, ab test, experimentation, cro, conversion rate optimisation, conversion rate optimization, hypothesis, sample size, minimum detectable effect, mde, statistical significance, peeking, sequential testing, landing page, split test, sample ratio mismatch
source: Evan Miller - How not to run an A/B test (peeking) | https://www.evanmiller.org/how-not-to-run-an-ab-test.html
source: Evan Miller - Sample size calculator | https://www.evanmiller.org/ab-testing/sample-size.html
source: Microsoft ExP - Diagnosing sample ratio mismatch in A/B testing | https://www.microsoft.com/en-us/research/group/experimentation-platform-exp/articles/diagnosing-sample-ratio-mismatch-in-a-b-testing/
source: Nielsen Norman Group - 10 usability heuristics | https://www.nngroup.com/articles/ten-usability-heuristics/
source: web.dev - Core Web Vitals | https://web.dev/articles/vitals
---
## When to use
Designing, sizing, monitoring or reading an A/B test; auditing a funnel or landing page for conversion problems; building a test roadmap.

## Hypothesis and metrics
- Format: Because we saw <evidence>, changing <element> for <audience> will move <primary metric> by at least <MDE>, measured over <duration>.
- One primary metric tied to value (conversion rate, revenue per visitor); guardrails (refunds, lead quality, page speed, unsubscribes); secondary metrics are exploratory. Prioritise ideas with ICE or PIE scores.

## Sample size and MDE
- Per variant: n = 2 x (z_(1-alpha/2) + z_(1-beta))^2 x p(1-p) / delta^2; p = baseline rate, delta = absolute MDE. At alpha 0.05 two-sided and 80 % power, (1.96 + 0.84)^2 is about 7.85, so 2 x 7.85 gives the rule of thumb n = 16 x p(1-p) / delta^2.
- Example: p = 5 %, 10 % relative lift, delta = 0.5 pp: n = 16 x 0.0475 / 0.000025 = 30,400 per variant.
- Duration = total sample / eligible daily traffic, rounded up to whole weeks (at least one full business cycle). Rule of thumb: cap at about 6 weeks, because cookie loss re-buckets returning users and dilutes the effect.
- Too little traffic: test bolder changes, a higher-funnel metric, fewer variants, or reduce variance with pre-period covariates (CUPED).

## Significance and peeking
- The p-value is the chance of data this extreme if there is no true difference; it is not the chance the variant wins. Report the lift with a 95 % confidence interval: (p2 - p1) +/- 1.96 x sqrt(p1(1-p1)/n1 + p2(1-p2)/n2).
- Peeking: checking repeatedly and stopping at the first p < 0.05 inflates false positives far above 5 % (Miller's simulation: about 26 % when testing after every observation, up to 150). Remedies: fix the sample size in advance; sequential designs with planned looks and alpha spending; always-valid sequential methods; or Bayesian rules set before launch.
- Many variants or metrics: Bonferroni (alpha / m) or Holm correction.
- Sample ratio mismatch: chi-square goodness-of-fit test of users per arm against the planned split, chi2 = sum((observed - expected)^2 / expected), before reading results; Microsoft ExP flags p < 0.0005. Find the cause (bucketing, redirects in one arm, bots, logging joins) before trusting anything.
- Watch novelty effects, segment fishing after the fact, and winner's curse (observed lifts shrink after rollout).

## CRO heuristics
- Research before ideas: funnel drop-offs, form analytics, heatmaps and recordings, on-site polls, five-user usability tests per key flow.
- Levers: value proposition clear above the fold; message match with the ad or keyword; less friction (fields, steps, forced sign-up); less anxiety (trust marks, guarantees, clear pricing, security); fewer distractions; urgency only when true.
- Usability audit with Nielsen's 10 heuristics (system status, real-world match, user control, consistency, error prevention, recognition over recall, flexibility, minimalist design, error recovery, help).
- Speed: Core Web Vitals good at the 75th percentile: LCP <= 2.5 s, INP <= 200 ms, CLS <= 0.1.

## Landing page tests
One hypothesis per test; A/B before multivariate; split paid traffic with platform experiments so the traffic mix stays equal; QA every variant on devices and browsers; freeze other changes during the test.

## Output
Test brief (hypothesis, metrics, MDE, sample, duration, risks); results table: users per arm, SRM check, conversion, lift with CI, p-value, guardrails; decision (ship, iterate, stop) and the learning logged.

## Pitfalls
- Stopping when it looks significant; calling a flat test a loss.
- Before-and-after comparisons presented as experiments.
