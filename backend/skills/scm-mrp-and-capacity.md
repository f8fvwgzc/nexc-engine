---
id: scm-mrp-and-capacity
name: MRP and capacity planning
category: supply-chain
description: Builds and checks the master production schedule, MRP explosion and lot sizing, tests feasibility with rough-cut and detailed capacity planning, manages lead times and explains DDMRP buffers.
keywords: mrp, material requirements planning, mps, master production schedule, lot sizing, rccp, rough-cut capacity, crp, capacity requirements planning, lead time, bill of materials, bom, time fence, available to promise, ddmrp, demand driven
source: Demand Driven Institute - DDMRP components | https://www.demanddriveninstitute.com/ddmrp
source: Wikipedia - Material requirements planning | https://en.wikipedia.org/wiki/Material_requirements_planning
source: frePPLe - open-source production planning and scheduling | https://github.com/frePPLe/frepple
---
## When to use
Building an MPS, auditing MRP output, choosing lot sizes, testing capacity (RCCP, CRP), cutting lead times or assessing DDMRP.

## Planning hierarchy
S&OP (families, 18-24 months) -> MPS (end items or planning bills, weekly) -> MRP (components) -> CRP -> shop floor and purchasing; each level reconciles with the one above.

## MPS
- Projected available balance: PAB_t = PAB_(t-1) + MPS_t - demand_t, where demand is customer orders inside the demand time fence and max(forecast, orders) beyond it.
- Discrete ATP = MPS receipt - customer orders until the next MPS receipt (first period also adds on hand).
- Demand time fence: inside it forecast drops out and the schedule is effectively frozen; changes need senior approval. Planning time fence: inside it the system does not replan on its own; planners firm and change orders.

## MRP explosion (per item, per period)
Inputs: MPS, BOM (quantity per, scrap, yield), inventory records (on hand, allocations, scheduled receipts, safety stock), lead times, lot rules. Process items by low-level code.
- Gross requirement = independent demand + parent planned order releases x quantity per.
- Net requirement = max(0, gross - projected on hand - scheduled receipts + safety stock).
- Planned order receipt covers the net requirement per the lot rule; planned order release = receipt offset by lead time.
- Exceptions: release, expedite, de-expedite, cancel. Pegging traces demand to parents; firm planned orders are not replanned.

## Lot sizing
Lot-for-lot (least stock, most setups); fixed quantity or EOQ; POQ = EOQ / average period demand, rounded; Wagner-Whitin (optimal); Silver-Meal, least unit cost, part-period balancing (heuristics). Then apply MOQ, multiples and scrap.

## Capacity
- Rated capacity = available time x utilisation x efficiency.
- RCCP tests the MPS on key resources: overall factors (CPOF), bill of capacity, or time-phased resource profiles.
- CRP loads planned and open orders through routings (setup + run per lot) onto each work centre by period; load % = load / rated capacity.
- Overloads: overtime, alternate routing, subcontracting, levelling, lot splitting or a changed MPS. Underloads: pull work forward.

## Lead time
Manufacturing lead time = queue + setup + run + wait + move. Queue usually dominates: cut WIP, not run time (Little's law: WIP = throughput x flow time). Cumulative lead time is the longest BOM path; the planning horizon must exceed it.

## DDMRP basics
Components (Demand Driven Institute): strategic decoupling, buffer profiles and levels, dynamic buffer adjustments, demand driven planning, visible and collaborative execution, tactical adaptation.
- Decoupled lead time (DLT): longest unprotected path to the next buffer. ADU: average daily usage.
- Yellow = ADU x DLT. Red = red base (ADU x DLT x lead-time factor) + red safety (red base x variability factor). Green = max(ADU x DLT x lead-time factor, MOQ, ADU x order cycle).
- Net flow position = on hand + on order - qualified demand (past due, due today, qualified spikes). If NFP is at or below top of yellow, order top of green - NFP.
- Generate orders by planning priority (NFP as % of top of green, lowest first) and manage open orders with buffer status alerts on on-hand vs top of red, not by due date.

## Output
Time-phased grid per item (gross, scheduled receipts, projected on hand, net, planned receipts and releases), exception list with actions, load profile per work centre with overload options, and changes needing fence approval.

## Pitfalls
Inaccurate BOMs or stock records (common targets: 95%+ inventory record and 98%+ BOM accuracy); safety stock at every BOM level; nervousness from replanning inside fences; releasing infinite-capacity plans without CRP; padded lead times that inflate WIP.
