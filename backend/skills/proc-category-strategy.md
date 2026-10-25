---
id: proc-category-strategy
name: Category strategy
category: procurement
description: Turns spend data into category plans: spend cube and opportunity sizing, Kraljic and supplier-preferencing positions, make-vs-buy appraisal and bottom-up should-cost models that set target prices.
keywords: category management, category strategy, category plan, spend analysis, spend cube, kraljic matrix, portfolio analysis, make or buy, make vs buy, outsourcing, should cost, cost model, tail spend, maverick spend, supplier preferencing, unspsc
source: Kraljic - Purchasing must become supply management (HBR, 1983) | https://hbr.org/1983/09/purchasing-must-become-supply-management
source: UK Government - The Sourcing and Consultancy Playbooks (should cost and delivery model guidance) | https://www.gov.uk/government/publications/the-sourcing-and-consultancy-playbooks
source: UK Government - The Sourcing Playbook (version 4) | https://www.gov.uk/government/publications/the-sourcing-and-consultancy-playbooks/the-sourcing-playbook-html
---
## When to use
Building a category plan, analysing spend, positioning suppliers, deciding make vs buy or outsourcing, or building a should-cost model for targets and negotiation.

## Spend analysis
1. Extract 12-36 months of AP, PO and card data across all entities.
2. Cleanse: normalise supplier names, link parents and children, remove duplicates, convert currency.
3. Classify to a taxonomy (UNSPSC or internal) at the level a category manager acts on.
4. Build the cube: category x supplier x business unit x time; add contract coverage and payment terms.
5. Analyse: Pareto concentration, tail spend, off-contract (maverick) spend, unit price variance, demand drivers, supplier fragmentation.
6. Size opportunities by lever: demand management, specification change, consolidation, competition, process and payment terms. Savings = (baseline unit price - new unit price) x forecast volume; budget-neutral cost avoidance is reported separately.

## Kraljic matrix
Axes: profit impact (spend, value added, effect on quality or margin) and supply risk (number of suppliers, substitutes, switching cost, market power, scarcity). Score each criterion 1-5, weight it, and split each axis at an agreed cut-off (often 3), not by opinion.
- Strategic (high impact, high risk): partnerships, long-term agreements, joint development, risk and gain sharing.
- Leverage (high impact, low risk): tender, consolidate volume, target pricing, e-auctions.
- Bottleneck (low impact, high risk): secure supply with buffer stock and longer contracts; develop alternative sources or specifications.
- Non-critical (low impact, low risk): simplify with catalogues, cards, fewer suppliers, automation.
Cross-check with supplier preferencing (how the supplier sees your account: core, development, exploitable, nuisance); a strategic item where you are a nuisance customer needs a different plan.

## Category plan structure
Scope and definition; spend baseline; stakeholder needs; supply market analysis (five forces, cost structure, capacity, innovation, PESTLE); incumbent performance; positioning; strategy and levers; value targets (price, cost avoidance, TCO, risk, ESG); roadmap; KPIs; risks; sign-off.

## Make vs buy
Compare strategic fit (core capability, IP, control), economics (avoidable cost to make vs full cost to buy including management, transition and exit), capacity, quality, risk and flexibility. In UK central government the Sourcing Playbook (version 4) makes a Delivery Model Assessment mandatory for new services, significant new developments and re-evaluation of an existing delivery model, and expects a proportionate one before outsourcing, insourcing or re-procuring.

## Should-cost model
Bottom-up: materials (BOM x index price / yield) + conversion (cycle time x machine rate + labour hours x labour rate) + overhead + SG&A + margin + packaging, freight and duty. Uses: target prices, negotiation fact base, spotting abnormally low or high bids, index-linked pricing. Log each assumption with source and date; run sensitivities on the top three drivers. UK central government requires a Should Cost Model estimate for every complex outsourcing project (first-generation outsourcing, major transformation, thin markets, novel or contentious services); elsewhere it is good practice.

## Output
Spend cube summary, opportunity table (lever, value, confidence, owner), Kraljic chart with scores, category plan, should-cost workbook.

## Pitfalls
Classifying by supplier name only; reporting cost avoidance as savings; full absorbed cost in make vs buy; positions without scoring; plans with no stakeholder sign-off.
