---
id: perf-paid-search-and-social
name: Paid search and social
category: marketing
description: Builds and runs paid search and paid social: account structure, match types, Smart Bidding (tCPA, tROAS), Performance Max, Quality Score diagnostics, Meta Advantage+ campaigns, creative testing and pacing.
keywords: paid search, ppc, sem, google ads, match types, smart bidding, target cpa, target roas, performance max, pmax, quality score, paid social, meta ads, advantage+, creative testing, budget pacing
source: Google Ads Help - About keyword matching options | https://support.google.com/google-ads/answer/7478529
source: Google Ads Help - About Smart Bidding | https://support.google.com/google-ads/answer/7065882
source: Google Ads Help - About Performance Max campaigns | https://support.google.com/google-ads/answer/10724817
source: Google Ads Help - About Quality Score | https://support.google.com/google-ads/answer/6167118
source: Meta for Business - Meta Advantage+ | https://www.facebook.com/business/ads/meta-advantage
---
## When to use
Planning, building, auditing or optimising Google Ads, Microsoft Ads or Meta campaigns; setting bid targets and budgets; diagnosing a drop in volume or efficiency.

## Search structure
- Campaign = budget, bid strategy, geo, language, networks. Split only where these must differ (brand vs non-brand, margin tiers, markets); fewer, larger campaigns feed bidding more data.
- Match types: broad (no notation; related searches, pair with Smart Bidding), phrase ("quotes"; searches carrying the keyword's meaning), exact ([brackets]; same meaning or intent). A broader type also covers narrower ones. An exact keyword identical to the query outranks phrase, broad and Performance Max; phrase and broad share priority with identical PMax search themes.
- Responsive search ads: up to 15 headlines (30 chars) and 4 descriptions (90 chars), each distinct; pin only for legal text. Review search terms weekly; maintain shared negative lists and brand exclusions.

## Bidding
- Smart Bidding (auction-time): Maximize conversions, Target CPA, Maximize conversion value, Target ROAS. Needs working conversion tracking. From June 2026 Google labels them simply Target CPA and Target ROAS; behaviour is unchanged.
- Start the target at the last 30 days' average CPA or ROAS adjusted for conversion delay (Google's recommendation); move it in steps of about 15-20 % (rule of thumb, not Google guidance); judge over at least 30 conversions (50 for tROAS) once the conversion lag has closed. Test big changes with Save as experiment.
- Feed real value: margin or LTV-weighted conversion values, offline conversion import, enhanced conversions. Use seasonality adjustments for short promotions and data exclusions for tracking outages.

## Performance Max
One campaign across Search, Shopping, YouTube, Display, Discover, Gmail and Maps. Asset groups by theme; audience signals are hints, not hard targeting. Decide final URL expansion deliberately and add URL exclusions. Use brand exclusions and negatives to stop it harvesting brand demand. Read channel performance, placement and asset reports; Merchant Center feed quality drives Shopping results. Prove incrementality with a geo or holdout test.

## Quality Score
1-10 per keyword from expected CTR, ad relevance and landing page experience, each rated above average, average or below average against the last 90 days. It is a diagnostic, not an auction input or KPI. Fix the weakest component: tighter ad groups, message match, faster and more relevant landing pages.

## Paid social (Meta)
- Advantage+ campaigns: sales (formerly Advantage+ shopping), app and leads; Advantage+ audience, placements and creative let delivery find people.
- Send server events with Conversions API alongside the pixel, deduplicated by matching event_name and event_id; watch event match quality.
- Few campaigns, broad audiences, enough budget for each ad set to leave learning (Meta guidance is about 50 optimisation events a week; verify locally).
- Creative is the targeting: test distinct concepts (hook, angle, format, offer), not small tweaks. Refresh when frequency rises, CTR falls and CPM climbs. Use the platform A/B test or a holdout for clean reads.

## Pacing
Planned daily spend = remaining budget / remaining days. Pacing index = spend to date / (budget x share of period elapsed); act outside 0.9-1.1. Google can spend up to twice the average daily budget on a day, but no more than 30.4 times it in a month.

## Output
Audit or plan: structure map, keywords and negatives, bid strategy and targets with rationale, budget and pacing table, creative test matrix, tracking checks, KPIs, next actions.

## Pitfalls
- Optimising to cheap leads that never become revenue.
- PMax and brand search claiming the same buyers.
