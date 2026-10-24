---
id: scm-network-design-and-resilience
name: Network design and resilience
category: supply-chain
description: Designs and stress-tests supply networks with the SCOR Digital Standard, landed-cost and facility trade-offs, tier-n risk mapping, single-source exposure, bullwhip diagnosis and time-to-recover vs time-to-survive analysis.
keywords: network design, scor, scor ds, supply chain resilience, risk mapping, single source, sole source, bullwhip effect, time to recover, time to survive, ttr, tts, landed cost, nearshoring, facility location, supply chain risk
source: Wikipedia - Supply chain operations reference (SCOR Digital Standard) | https://en.wikipedia.org/wiki/Supply_chain_operations_reference
source: Simchi-Levi, Schmidt, Wei - From superstorms to factory fires (HBR, 2014) | https://hbr.org/2014/01/from-superstorms-to-factory-fires-managing-unpredictable-supply-chain-disruptions
source: Lee, Padmanabhan, Whang - The bullwhip effect in supply chains (MIT SMR, 1997) | https://sloanreview.mit.edu/article/the-bullwhip-effect-in-supply-chains/
---
## When to use
Designing or reviewing a supply and distribution network, mapping supply risk, sizing single-source exposure, setting resilience targets or explaining order amplification.

## SCOR Digital Standard (ASCM)
- Process areas: Orchestrate, Plan, Order, Source, Transform, Fulfill, Return (Transform extends Make; Order and Fulfill split the old Deliver).
- Performance attributes in three groups: resilience (reliability, responsiveness, agility), economic (cost, profit, assets), sustainability (environmental, social); confirm metric codes and definitions in the ASCM SCOR DS (verify locally). Typical level-1 metrics: perfect order fulfilment, order fulfilment cycle time, upside and downside adaptability, total supply chain management cost, cash-to-cash cycle time, return on assets and working capital, emissions.
- Use it to benchmark, choose a strategic profile (e.g. superior on reliability, parity on cost) and decompose to level-2/3 processes.

## Network design method
1. Baseline demand by region, facilities, lanes, costs, capacities and service; the model must reproduce actual cost within a few percent before scenarios.
2. Scenarios: number and place of plants and DCs, source countries, modes, nearshoring, tariff cases.
3. Model: centre of gravity for a first cut (x* = sum(w_i x_i) / sum w_i, same for y; w_i = volume x freight rate per km), mixed-integer optimisation for location and flows, simulation for variability and service.
4. Landed cost = price + freight + duty and tariffs + insurance + handling + pipeline and safety stock carrying + facility fixed cost.
5. Trade-offs: more DCs cut outbound freight and lead time but add fixed and inventory cost (safety stock grows roughly with sqrt of the number of sites); offshore cuts unit cost but adds long, variable lead time, pipeline stock and risk.
6. Stress-test each scenario and choose the robust option, not only the cheapest.

## Risk mapping and exposure
- Map critical parts from tier 1 to tier n: sites, sub-tier sources, ports, countries.
- Single source (alternatives exist, one used) vs sole source (only one qualified). Flag one-site, one-country or long-requalification parts.
- Time to recover (TTR): time for a node to return to full output after disruption. Time to survive (TTS): how long the network can meet demand without that node, using stock and alternates. If TTR > TTS, lost output = (TTR - TTS) x demand rate; rank nodes by lost profit, not spend: cheap parts often top the list.
- Mitigation: dual or alternate sources, strategic buffers sized to cover TTR - TTS, multi-site production, flexible capacity contracts, postponement, visibility.
- Use likelihood x impact for frequent risks; TTR/TTS for rare, severe ones.

## Bullwhip effect
Order variance grows upstream; measure variance(orders) / variance(demand) per echelon. Causes: demand forecast updating, order batching, price fluctuation, rationing and shortage gaming. Counter with shared POS and stock data, VMI or CPFR, smaller frequent orders, stable pricing, allocation by past sales, shorter lead times.

## Output
Scenario table (cost, service, lead time, risk, CO2e), recommended network with sensitivities, risk heat map, TTR/TTS table, mitigation plan with cost and owner.

## Checklist
Baseline reproduces actual cost; every scenario has the same service constraint; duty and tariff cases included; tier-n map covers parts with TTR > TTS; each mitigation has cost, owner and review date.

## Pitfalls
Optimising cost without service or risk constraints; designing for average not peak demand; ignoring tariff scenarios; equating low spend with low risk; mapping once and never refreshing.
