---
id: scm-inventory-optimization
name: Inventory optimisation
category: supply-chain
description: Sets order quantities, reorder points and safety stock (EOQ, ROP, cycle service level vs fill rate), segments items with ABC/XYZ and positions stock across echelons to balance service against inventory investment.
keywords: inventory optimisation, inventory optimization, safety stock, reorder point, eoq, economic order quantity, service level, fill rate, cycle service level, abc analysis, xyz analysis, multi-echelon, meio, newsvendor, order-up-to
source: stockpyl - Python package for single- and multi-echelon inventory optimisation | https://github.com/LarrySnyder/stockpyl
source: stockpyl documentation - module list | https://stockpyl.readthedocs.io/en/latest/
source: ASCM - 2026 CSCP module content outline (ECM 5.0) | https://www.ascm.org/globalassets/ascm_website_assets/docs/credentials/2026-cscp-module-content-outline.pdf
---
## When to use
Setting reorder points, lot sizes and safety stock; choosing service targets; segmenting SKUs; deciding where in a network to hold stock.

## Formulas
D annual demand; S cost per order; H holding cost per unit-year = i x unit cost (carrying rate i often 15-30%); d and sd mean and SD of demand per period; L mean lead time in periods; sL SD of lead time; z standard normal quantile.
- EOQ Q* = sqrt(2DS / H). Cost DS/Q + QH/2 is flat near Q*: 20% off Q* adds only about 2%.
- Reorder point (continuous review): ROP = d x L + SS.
- Safety stock, demand varies: SS = z x sd x sqrt(L).
- Demand and lead time vary (independent): SS = z x sqrt(L x sd^2 + d^2 x sL^2).
- Periodic review (R, S): order-up-to S = d(R + L) + z x sd x sqrt(R + L).
- Cycle service level (CSL, type 1) = P(no stockout in a cycle) = Phi(z): 90% 1.28, 95% 1.645, 97.5% 1.96, 99% 2.33.
- Fill rate (type 2) = 1 - ESC / Q. ESC = sLT x G(z) is expected units short per cycle, with G(z) = phi(z) - z(1 - Phi(z)) the standard normal loss function, sLT the SD of lead-time demand and Q the order quantity (d x R under periodic review, with the SD taken over R + L). Fill rate is usually well above CSL at the same z; confirm which one the business means.
- Newsvendor: critical ratio = Cu / (Cu + Co); stock the demand quantile at that ratio.
- Pooling: n independent equal locations centralised need about 1/sqrt(n) of the total safety stock.
- Worked check: d = 100/day, sd = 30, L = 4 days, z = 1.645 gives SS = 1.645 x 30 x 2 = 99 and ROP = 400 + 99 = 499.

## Segmentation
- ABC on annual usage value: A about top 70-80% of value (10-20% of SKUs), B next 15-20%, C the rest.
- XYZ on CV of period demand: X below 0.5, Y 0.5-1.0, Z above 1.0 (cut-offs set locally).
- AX: tight control, frequent review, statistical SS. CZ: make or buy to order, or delist. Set A-item service on margin and criticality, not class alone.

## Multi-echelon basics
- Echelon stock = stock at a node plus all stock downstream of it.
- Guaranteed-service model: each stage quotes a service time; its SS covers demand over net replenishment time = inbound service time + processing time - outbound service time. Stochastic-service models (Clark-Scarf base stock) treat delays as random.
- Pooled upstream stock costs less; customer-facing stock serves faster. Optimise jointly, not node by node.

## Method
1. Agree the service metric (CSL, fill rate, OTIF) and target per segment.
2. Measure variability from data; when a forecast exists use the SD (or RMSE) of forecast error at the lead-time lag, not of raw demand.
3. Compute SS, ROP and Q; round to pack sizes and MOQs.
4. Back-test or simulate the policy on history (run_python).
5. Plot inventory investment vs service and pick the knee.
6. Recalculate monthly or when demand or lead time shifts.

## Output
Policy table per SKU-location (segment, service metric and target, d, sd, L, sL, SS, ROP or S, Q after rounding), expected average stock = Q/2 + SS in units and value, turns, the service vs investment curve, back-test results and items needing review.

## Pitfalls
Mixing time units (weekly SD with daily lead time); calling CSL a fill rate; normal approximations when lead-time demand is only a few units (use Poisson, negative binomial or bootstrap); ignoring lead-time variability, which often dominates for overseas supply; safety stock at every node of a network.
