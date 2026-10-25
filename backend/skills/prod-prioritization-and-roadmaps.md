---
id: prod-prioritization-and-roadmaps
name: Prioritisation and roadmaps
category: product
description: Ranks product work with RICE, Kano, MoSCoW and WSJF, builds now-next-later and outcome roadmaps, and declines requests with reasons; use when choosing what to build next or presenting a roadmap.
keywords: prioritization, prioritisation, rice, kano, moscow, wsjf, cost of delay, roadmap, now next later, outcome roadmap, backlog, trade-off, okr, saying no
source: Intercom - RICE prioritization | https://www.intercom.com/blog/rice-simple-prioritization-for-product-managers/
source: Agile Business Consortium - MoSCoW prioritisation | https://www.agilebusiness.org/dsdm-project-framework/moscow-prioritisation.html
source: SAFe - Weighted Shortest Job First | https://framework.scaledagile.com/wsjf
source: Kano model | https://en.wikipedia.org/wiki/Kano_model
source: ProdPad - Now-Next-Later roadmap | https://www.prodpad.com/blog/invented-now-next-later-roadmap/
---
## When to use
Choosing between candidate problems or features, cutting scope to a date, or building and presenting a roadmap.

## Methods
- RICE = (Reach x Impact x Confidence) / Effort. Reach = people or events per period (e.g. users per quarter); Impact 3 massive, 2 high, 1 medium, 0.5 low, 0.25 minimal; Confidence 100%, 80%, 50% (below that is a moonshot); Effort in person-months. Example: 4,000 x 2 x 0.8 / 4 = 1,600.
- Kano: ask a functional ("if it had X") and a dysfunctional ("if it lacked X") question per feature, answers like / expect / neutral / tolerate / dislike. Expect + dislike = must-be; like + dislike = one-dimensional (performance); like + neutral = attractive; neutral + neutral = indifferent; dislike + expect = reverse. With A, O, M, I = counts of attractive, one-dimensional, must-be and indifferent answers: Better = (A + O) / (A + O + M + I); Worse = -(O + M) / (A + O + M + I). Delighters decay into must-bes over time.
- MoSCoW (DSDM): Must = without it the release is pointless or unlawful; Should = painful to drop, workaround exists; Could = first to drop; Won't = not this time, recorded. Keep Musts at or below about 60% of effort and Coulds near 20% as contingency.
- WSJF = cost of delay / job size, cost of delay = user-business value + time criticality + risk reduction or opportunity enablement, each scored relatively (1, 2, 3, 5, 8, 13, 20; smallest = 1).

| Situation | Method |
|---|---|
| Many comparable features, usage data exists | RICE |
| Unsure what customers expect versus value | Kano survey |
| Fixed date, flexible scope | MoSCoW |
| Flow of epics competing for one train | WSJF |
| Strategic bets | written rationale tied to strategy, not a score |

## Roadmaps
- Now-next-later: Now = committed and detailed, Next = shaped, Later = problems kept in view. Cards carry problems or outcomes with linked ideas, not feature lists; every item ties to an objective (e.g. an OKR).
- Outcome roadmap item: problem, target customer, outcome metric and target, evidence, confidence, candidate solutions.
- Dates only where a real external commitment exists (contract, regulation, event); label each commitment and its confidence.
- Keep one source roadmap and derive audience views (executives, sales, customers with a no-commitment note).
- Reserve capacity for reliability and tech debt (often 15-25%, set locally).

## Saying no
- Tie the answer to strategy and the published criteria: "not now, because it does not move <outcome>".
- Frame trade-offs: "yes, if we drop <item>".
- Check single-customer asks for a pattern: how many accounts, revenue at stake, fit with the target segment.
- Log declined items with a revisit trigger; never promise a date to win a deal without capacity and evidence.

## Output
Scoring table with inputs and sources, sensitivity (does the order change if confidence drops a level?), chosen order with rationale, roadmap view, list of what was declined and why.

## Pitfalls
False precision; inflated confidence; one stakeholder's opinion overriding data; scoring strategic bets like small features; feature-and-date roadmaps; ignoring dependencies and operating cost.
