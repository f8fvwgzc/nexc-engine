---
id: mkt-measurement-and-attribution
name: Marketing measurement and attribution
category: marketing
description: Measures marketing impact with funnel KPIs, CAC, LTV and payback, picks attribution models, and validates spend with marketing mix modelling and incrementality or geo tests under privacy limits.
keywords: marketing measurement, attribution, data-driven attribution, cac, ltv, clv, payback, roas, mer, marketing mix modelling, mmm, incrementality, geo test, lift test, meridian, robyn
source: Google Analytics Help - Get started with attribution (GA4 models) | https://support.google.com/analytics/answer/10596866
source: Google Ads Help - About attribution models | https://support.google.com/google-ads/answer/6259715
source: google/meridian - open-source Bayesian MMM | https://github.com/google/meridian
source: facebookexperimental/Robyn - MMM package | https://github.com/facebookexperimental/Robyn
source: facebookincubator/GeoLift - geo experiments | https://github.com/facebookincubator/GeoLift
---
## When to use
Setting KPIs, proving or allocating budget, comparing channels, reconciling platform-reported results, or designing tests that show what marketing actually caused.

## Formulas
- CAC = (marketing + sales cost in period, incl. people and tools) / new customers in period. Report blended and paid CAC separately.
- Subscription LTV = ARPA x gross margin % / churn rate (same period basis). With a discount rate d and retention r per period: LTV = margin x r / (1 + d - r).
- LTV:CAC (rule of thumb >= 3); CAC payback (months) = CAC / (monthly ARPA x gross margin %).
- ROAS = attributed revenue / spend; break-even ROAS = 1 / gross margin % (use contribution margin when fulfilment, payment or returns costs are material); iROAS = incremental revenue / spend; MER = total revenue / total marketing spend.
- Funnel: impressions -> clicks (CTR) -> leads (CVR) -> MQL -> SQL -> opportunity -> won (win rate). Track stage conversion and velocity per cohort.

## Attribution
- Rule-based: last click, first click, linear, time decay, position-based. GA4 removed first click, linear, time decay and position-based in November 2023; it now offers data-driven, paid and organic last click, and Google paid channels last click. Google Ads keeps data-driven (default for most actions) and last click.
- Data-driven models compare converting and non-converting paths, but each platform sees only its own touchpoints; summed platform claims usually exceed real conversions.
- Attribution is correlational. Use it to steer within a channel (keywords, creatives), not to split budget across channels.

## Marketing mix modelling
- Data: 2-3 years of weekly data (104+ weeks), by geo when possible: spend or impressions per channel, price, promotions, distribution, seasonality, macro and competitor factors.
- Carryover (geometric adstock): A_t = x_t + lambda x A_(t-1), lambda in [0,1). Saturation (Hill): f(A) = A^s / (A^s + K^s), K = half-saturation point, s = shape.
- Tools: Google Meridian (Bayesian, geo-level, reach and frequency, experiment results as priors; v2.1.0 per the repo's 2026 citation), Meta Robyn (ridge regression with Nevergrad multi-objective hyperparameter search; R, Python API in beta).
- Outputs: contribution, ROI and marginal ROI per channel, response curves, budget scenarios. Validate with holdout error, posterior predictive checks, priors versus experiments, and stable decompositions across refits.

## Incrementality
- Lift = (test - control) / control; incremental conversions = test - scaled control.
- Designs: user-level holdouts (platform conversion lift), geo experiments (matched markets or synthetic control, e.g. GeoLift), time-based switchbacks. Run a power analysis first: MDE, duration (at least two purchase cycles), number of geos.
- Triangulate: experiments calibrate MMM, MMM sets budget, attribution steers daily.

## Privacy-safe measurement
Consent mode modelling, enhanced conversions with hashed first-party data, server-side tagging, data clean rooms, aggregated app attribution (SKAdNetwork, AdAttributionKit), MMM on aggregates. Disclose the modelled share of any reported number.

## Pitfalls
- ROAS above break-even on attributed, not incremental, revenue.
- LTV from early cohorts with no churn history; mixing monthly and annual rates.
- Retargeting and brand search credited for demand that existed anyway.
