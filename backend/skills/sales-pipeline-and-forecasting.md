---
id: sales-pipeline-and-forecasting
name: Pipeline and forecasting
category: sales
description: Designs sales stages with exit criteria and measures pipeline coverage, win rate, velocity and forecast accuracy, then runs forecast categories and pipeline reviews.
keywords: pipeline, sales forecast, forecasting, pipeline coverage, win rate, sales velocity, sales stages, exit criteria, commit, best case, weighted pipeline, deal review, quota, crm, slippage
source: HubSpot - Sales velocity formula | https://blog.hubspot.com/sales/sales-velocity
source: MEDDICC - MEDDPICC sales methodology and process | https://www.meddicc.com/meddpicc-sales-methodology-and-process
---
## When to use
Designing sales stages, inspecting pipeline health, building or challenging a forecast, and running deal, pipeline and forecast reviews.

## Stage design
Stages follow the buyer's process and close on verifiable buyer actions, not seller activity. Example for B2B:

| Stage | Exit criteria (evidence) | Default probability |
|---|---|---|
| 1 Qualify | ICP fit, pain confirmed, first meeting held | 10% |
| 2 Discover | metrics, decision process and champion identified | 20% |
| 3 Validate | demo or POC success criteria agreed and met | 40% |
| 4 Propose | economic buyer has seen pricing and business case; mutual plan agreed | 60% |
| 5 Commit | verbal selection; paper process under way | 80% |
| Closed | signed (won) or loss reason recorded | 100% / 0% |

Replace default probabilities with your own historical stage-to-close conversion by segment.

## Formulas
- Win rate = won / (won + lost) for deals closed in the period, by count and by value; state how early-stage disqualifications are treated.
- Coverage = open qualified pipeline due this period / (quota - closed to date). Coverage needed is about 1 / win rate (25% win rate gives 4x); 3x is only a rule of thumb.
- Weighted pipeline = sum of (amount x stage probability).
- Sales velocity per day = (opportunities x average deal size x win rate) / average cycle days. Example: 40 x 25,000 x 0.25 / 90 = 2,778 per day.
- Stage ageing: median days in stage; flag deals above 1.5x the median.
- Forecast accuracy = 1 - |actual - forecast| / actual; track signed bias per rep.
- New pipeline needed = (target - closed - expected from current pipeline) / win rate.

## Forecast categories
Salesforce defaults (orgs can rename; verify locally): Pipeline (open, unlikely this period), Best case (could close with effort), Commit (rep would stake their name: economic buyer approval, paper process running, close plan dated), Closed (won), Omitted (lost or excluded). Call = closed + commit, best case as upside. Managers roll up with overrides recorded and explained.

## CRM design (Salesforce-style)
- One sales process per record type; each stage carries a default probability and forecast category, overridable only with a reason.
- Enforce exit criteria with required fields or validation rules (e.g. Commit needs economic buyer contact role, close plan date, next step).
- Opportunity contact roles for the buying group; products and price books so amounts are not typed by hand; close date and amount history tracked.
- Forecast hierarchy matches the role hierarchy; manager adjustments logged; lead conversion creates account, contact and opportunity, with duplicate rules on.

## Review cadence
- Weekly deal review: top deals by value and risk; next step and date, MEDDICC gaps, what changed since last week.
- Hygiene: close dates in the past, no next step, no activity for 14+ days, pushed twice or more, amount changes.
- Forecast call: commit versus last week, movement in, out and pushed, upside and risks.

## Output
Pipeline report: coverage, weighted pipeline, waterfall (start, created, won, lost, pushed, end), stage conversion, velocity, forecast by category with confidence and an assumptions table, by segment.

## Pitfalls
Advancing stages on optimism; sandbagging or over-committing; close dates defaulted to quarter end; coverage that counts unqualified or next-quarter deals; averaging segments with different cycle lengths.
