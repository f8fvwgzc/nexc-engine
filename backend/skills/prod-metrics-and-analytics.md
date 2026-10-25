---
id: prod-metrics-and-analytics
name: Product metrics and analytics
category: product
description: Defines a north star and input metrics, analyses AARRR funnels, activation and retention cohorts, sets guardrails, and reads A/B test results with SRM checks and confidence intervals; use for product KPIs and experiment readouts.
keywords: product metrics, north star metric, kpi, aarrr, pirate metrics, activation, retention, cohort analysis, churn, guardrail metrics, a/b test, experiment, statistical significance, sample ratio mismatch, srm, confidence interval
source: Amplitude - Product North Star Metric | https://amplitude.com/blog/product-north-star-metric
source: Amplitude - The North Star Playbook | https://amplitude.com/books/north-star
source: Microsoft ExP - Diagnosing sample ratio mismatch in A/B testing | https://www.microsoft.com/en-us/research/group/experimentation-platform-exp/articles/diagnosing-sample-ratio-mismatch-in-a-b-testing/
source: Kohavi, Tang, Xu - Trustworthy Online Controlled Experiments | https://experimentguide.com/
---
## When to use
Choosing what to measure, diagnosing growth or churn, or deciding whether an experiment result justifies shipping.

## North star and inputs
- One north star metric that captures value customers actually receive, leads revenue, and the team can move. Not vanity counts (sign-ups, raw DAU) and not lagging revenue.
- Pick the game: attention (time spent), transaction (purchases completed), productivity (work done efficiently).
- 3-5 input metrics the team controls, along breadth (how many users), depth (how much they do), frequency (how often), efficiency (how fast they succeed).

## Funnel and cohorts
- AARRR: acquisition, activation, retention, referral, revenue. One metric per stage; fix the leakiest step, usually retention before acquisition.
- Activation: the early action that predicts retention ("did X within Y days"), validated against retained versus churned cohorts, then tested causally.
- Retention: cohort users by start week; plot % active in week N. Say whether retention is N-day (active on that day) or unbounded (active on or after). A curve that flattens above zero signals a retained core.
- Stickiness = DAU / MAU. Revenue: NRR = (start MRR + expansion - contraction - churn) / start MRR; GRR excludes expansion. LTV is about ARPA x gross margin / churn rate; CAC payback (months) = CAC / (monthly ARPA x gross margin). Keep ARPA and churn in the same period.

## Guardrails
Metrics that must not get worse while you chase the goal: latency, error and crash rates, unsubscribes, refunds, support contacts, revenue per user. Trust guardrails check the experiment itself (SRM, logging loss). Agree the tolerated degradation before launch.

## Reading an A/B test
1. Before: one primary metric, guardrails, minimum detectable effect (MDE), alpha 0.05 (two-sided), power 80%. Sample per arm is then about 16 x variance / MDE^2; for a rate p, variance = p(1 - p). Example: baseline 10%, MDE 0.5 pp gives 16 x 0.09 / 0.005^2 = 57,600 users per arm.
2. SRM first: chi-square test of users per arm against the planned split; Microsoft ExP flags p below 0.0005. With SRM, do not read results; find the cause (assignment, redirects, bot filters, triggering, joins).
3. Effect and interval: difference, 95% CI, p-value. Example: 10.0% vs 10.6% with 20,000 users per arm gives SE 0.30 pp, z 1.97, p about 0.05, CI +0.004 to +1.20 pp: barely significant, and the true lift may be below the MDE.
4. Run whole weeks; no peeking unless the test is sequential; correct for many metrics or variants; treat segment cuts as hypotheses.
5. Twyman's law: a surprising win is a bug until proven otherwise. Watch novelty and primacy effects.
6. Decide: ship (primary wins, guardrails hold), iterate, or stop; record the learning.

## Output
Metric tree from north star to inputs; definitions (formula, source table, window, owner); funnel and cohort tables; experiment readout with SRM result, effect, CI, guardrails and a decision.

## Pitfalls
Ratio metrics with moving denominators; averages hiding skew; mixing calendar and cohort views; declaring wins on p alone; changing the primary metric after the fact; dashboards with no decision attached.
