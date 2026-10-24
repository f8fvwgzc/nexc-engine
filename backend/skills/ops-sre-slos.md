---
id: ops-sre-slos
name: SLOs and error budgets
category: devops
description: Defines SLIs and SLOs for a service, computes error budgets and burn rates, writes the error budget policy and multi-window burn-rate alerts, and targets toil, following the Google SRE books.
keywords: slo, sli, sla, error budget, burn rate, slo alerting, availability, latency, reliability target, error budget policy, toil, site reliability engineering, sre
source: Google SRE Workbook - Implementing SLOs | https://sre.google/workbook/implementing-slos/
source: Google SRE Workbook - Alerting on SLOs | https://sre.google/workbook/alerting-on-slos/
source: Google SRE Workbook - Example error budget policy | https://sre.google/workbook/error-budget-policy/
source: Google SRE Book - Eliminating toil | https://sre.google/sre-book/eliminating-toil/
---
## When to use
Setting reliability targets, designing paging, deciding feature versus reliability work, sizing an SLA margin, or cutting operational load.

## Definitions
- SLI = good events / valid events x 100%, measured near the user (load balancer logs, client telemetry, probes). Write the specification (what users feel) before choosing the implementation.
- SLI menu: request-driven services = availability, latency, quality; pipelines = freshness, correctness, coverage; storage = durability.
- SLO = target for an SLI over a window, e.g. 99.9% of valid requests succeed over a rolling 28 days (whole weeks). SLA = contract with penalties; keep it looser than the SLO.
- Error budget = 1 - SLO. In events: (1 - SLO) x valid events (99.9% of 3,000,000 = 3,000 failures). In time: 99.9% of 30 days = 43.2 minutes.

## Formulas
- Burn rate b = observed error ratio / (1 - SLO). b = 1 spends the budget exactly over the window; time to exhaustion = window / b.
- Budget spent in an alert window = b x window / SLO period (14.4 x 1 h / 30 d = 2%).
- Fire when the error ratio over the window > b x (1 - SLO).

## Multi-window, multi-burn-rate alerts (99.9%, 30 days)
| Severity | Long window | Short window | Burn rate | Budget spent |
|---|---|---|---|---|
| Page | 1 h | 5 min | 14.4 | 2% |
| Page | 6 h | 30 min | 6 | 5% |
| Ticket | 3 d | 6 h | 1 | 10% |

Both windows must breach. The short window is 1/12 of the long one so the alert clears soon after recovery. Judge alert rules on precision, recall, detection time and reset time.

Prometheus shape for the first row (ratio = errors / requests over the window):
`(rate(errors[1h]) / rate(requests[1h]) > 14.4 * 0.001) and (rate(errors[5m]) / rate(requests[5m]) > 14.4 * 0.001)`
Use recording rules per window; low-traffic services need synthetic traffic or longer windows, because a few failures swing the ratio.

## Method
1. List critical user journeys; pick 1-3 SLIs each.
2. Baseline 4 weeks of data; set the SLO at what users accept and the system can hold. Never 100%.
3. SLO document: service, SLIs and implementation, window, targets, rationale, budget maths, authors, approvers, review date.
4. Error budget policy agreed by product, development and SRE: budget spent over the window means releases stop except P0 and security fixes until back in SLO; one incident burning over 20% of the four-week budget gets a postmortem with at least one P0 action; one outage class burning over 20% in a quarter gets a P0 item in next quarter's plan; disputes escalate to a named executive. State exclusions up front (company-wide network outage, load tests, miscategorised errors).
5. Build the burn-rate alerts; delete cause-based pages that map to no SLO.
6. Report budget remaining and burn weekly; review targets quarterly and move them only with evidence (user complaints at the current level, or budget never touched for a year).

## Toil
Manual, repetitive, automatable, tactical, no lasting value, grows linearly with load. Keep it under 50% of each engineer's time. Measure (tickets, pages, hours), rank by frequency x duration, automate or design it out.

## Output
SLO table (journey, SLI spec and implementation, target, window, budget), alert rules, error budget policy, current budget status and burn, toil inventory with automation plan.

## Pitfalls
SLOs on metrics users never feel; average latency instead of thresholds (99% of requests under 300 ms); too many SLOs; a budget with no policy behind it; paging on blips; inconsistent treatment of planned maintenance.
