---
id: fin-fixed-income
name: Fixed income analysis
category: finance
description: Prices bonds and measures their rate and credit risk with yields, Macaulay, modified and effective duration, convexity, spreads and the term structure; covers credit analysis and securitised products.
keywords: fixed income, bond, yield to maturity, duration, modified duration, convexity, pvbp, dv01, spread, oas, z-spread, yield curve, credit analysis, default probability, mbs, abs
source: CFA Institute - Yield-based bond duration measures and properties (2026) | https://www.cfainstitute.org/insights/professional-learning/refresher-readings/2026/yield-based-bond-duration-measures-and-properties
source: CFA Institute - Curve-based and empirical fixed-income risk measures (2026) | https://www.cfainstitute.org/insights/professional-learning/refresher-readings/2026/curve-based-and-empirical-fixed-income-risk-measures
---
## When to use
Pricing a bond, estimating price moves for rate or spread shocks, hedging or immunising a portfolio, assessing an issuer's credit, or reading an ABS or MBS deal.

## Pricing and yields
- Price = sum of C/(1 + y/m)^k + F/(1 + y/m)^N, where C = coupon per period, m = periods per year, N = periods. Full price = flat price + accrued interest (coupon x days since last coupon / days in period).
- Current yield = annual coupon / flat price. YTM is the IRR of the cash flows; yield to worst is the lowest of yield to each call date and to maturity.
- Convert periodicity: (1 + r_m/m)^m = (1 + r_n/n)^n.

## Rate risk
- Macaulay duration = sum of t x PV(CF_t) / price (in periods; divide by m for years). A zero's equals its maturity.
- Modified duration = MacDur / (1 + y/m). Approximation: (PV_down - PV_up) / (2 x dy x PV_0).
- Convexity approx = (PV_down + PV_up - 2 PV_0) / (dy^2 x PV_0).
- %dP ~= -ModDur x dy + 0.5 x Convexity x dy^2 (dy in decimals).
- Effective duration and convexity: same formulas with shifts of the benchmark curve and a model price; use for callable (negative convexity when rates fall), putable and MBS.
- Money duration = ModDur x full price x position; PVBP (DV01) = (PV_down - PV_up)/2 for a 1 bp shift.
- Duration usually rises with maturity (long deep-discount bonds can be exceptions), falls with higher coupon and higher yield. Portfolio duration ~= market-value-weighted average. Key rate durations capture non-parallel shifts.
- Spread risk: %dP ~= -spread duration x d(spread) + 0.5 x convexity x d(spread)^2.

## Immunisation
- Single liability: Macaulay duration = horizon, PV of assets >= PV of liability, lowest convexity (dispersion) that meets it; rebalance as time and yields move.
- Several liabilities: match money duration (BPV), PV assets >= PV liabilities, asset convexity above liability convexity. Remaining risks: non-parallel shifts, defaults, calls.

## Spreads and curve
- G-spread over the government curve; I-spread over swaps; Z-spread: constant spread over every spot rate; OAS = Z-spread - option value (callable: OAS < Z).
- Spot and forward: (1 + S2)^2 = (1 + S1)(1 + f_1,1). Curve theories: expectations, liquidity preference, segmented markets, preferred habitat.

## Credit analysis
- Expected loss = PD x LGD (LGD = 1 - recovery rate) x exposure. Spread compensates for expected loss, liquidity and a risk premium.
- Four Cs: capacity, collateral, covenants, character. Ratios: debt/EBITDA, EBITDA/interest, FFO/debt, FCF/debt. Investment grade is BBB-/Baa3 or above.
- Seniority drives recovery: secured, senior unsecured, subordinated; check structural subordination at holding companies.

## Securitised products
- ABS, RMBS, CMBS, CLO; covered bonds keep recourse to the issuer. Prepayment risk: contraction when rates fall, extension when they rise; SMM = 1 - (1 - CPR)^(1/12); 100% PSA = CPR rising 0.2 points a month to 6% at month 30.
- Credit enhancement: subordination, overcollateralisation, excess spread, reserve funds. Tranching: sequential pay, PAC with support tranches.

## Output
Price and yield table, duration and convexity with the shock used, scenario P&L, credit view with ratios and covenants, and assumptions (day count, compounding, curve date).

## Pitfalls
Using modified duration for bonds with options; mixing flat and full prices; wrong day count; ignoring convexity for shocks above about 100 bp; treating a rating as a default probability.
