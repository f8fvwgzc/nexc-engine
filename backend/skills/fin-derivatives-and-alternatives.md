---
id: fin-derivatives-and-alternatives
name: Derivatives and alternatives
category: finance
description: Prices and uses forwards, futures, swaps and options by no-arbitrage (put-call parity, binomial, Black-Scholes-Merton, Greeks) and analyses private equity, real estate, hedge fund and commodity fees and valuation.
keywords: derivatives, forward, futures, swap, option, put-call parity, binomial, black-scholes, greeks, delta hedge, hedging, private equity, hedge fund fees, real estate, commodities, carried interest
source: CFA Institute - Option replication using put-call parity (2026) | https://www.cfainstitute.org/insights/professional-learning/refresher-readings/2026/option-replication-using-put-call-parity
source: CFA Institute - Alternative investment performance and returns (2026) | https://www.cfainstitute.org/insights/professional-learning/refresher-readings/2026/alternative-investment-performance-and-returns
---
## When to use
Pricing or checking a derivative, designing a hedge, or analysing fees, returns and valuation of an alternative fund.

## Forwards, futures, swaps
- Forward price: F0 = (S0 - PV(income) + PV(costs)) x (1 + r)^T; continuous: S0 x e^((r - q)T). FX: F = S x (1 + r_price)^T / (1 + r_base)^T.
- Value of a long forward at t: V_t = S_t - PV_t(income) + PV_t(costs) - F0/(1 + r)^(T - t).
- Futures settle daily (initial, maintenance and variation margin); basis = spot - futures converges to zero.
- Par swap rate = (1 - Z_N) / sum of Z_i, Z = discount factors. Receive-fixed value later = (fixed rate - current par rate) x sum of remaining Z x notional.

## Options
- Put-call parity (European, no income): S0 + p0 = c0 + X/(1 + r)^T. Forward form: F0/(1 + r)^T + p0 = c0 + X/(1 + r)^T. A gap means arbitrage: buy the cheap side, sell the rich one.
- One-period binomial: pi = (1 + r - d)/(u - d); c0 = [pi c_u + (1 - pi) c_d]/(1 + r); hedge ratio = (c_u - c_d)/(S_u - S_d).
- With known income: S0 - PV(income) + p0 = c0 + PV(X).
- BSM: c = S N(d1) - X e^(-rT) N(d2); p = X e^(-rT) N(-d2) - S N(-d1); d1 = [ln(S/X) + (r + sigma^2/2)T]/(sigma sqrt T); d2 = d1 - sigma sqrt T (r continuous). Dividend yield q: use S e^(-qT) for S throughout, so d1 = [ln(S/X) + (r - q + sigma^2/2)T]/(sigma sqrt T). Assumes lognormal prices, constant rate and volatility, no frictions, European exercise.
- Greeks: delta (call 0 to 1, put -1 to 0); gamma highest at the money near expiry; vega positive for long options; theta usually negative for long options; rho. Delta hedge: options needed = -position delta / option delta; gamma forces rebalancing.

## Hedging
- Minimum-variance ratio h = rho x sigma_S / sigma_F; contracts = h x exposure / futures notional.
- Duration: contracts = (target BPV - portfolio BPV) / futures BPV, with futures BPV ~= CTD BPV / conversion factor.
- Equity beta: contracts = (beta_target - beta_portfolio)/beta_futures x portfolio value / (futures price x multiplier).

## Alternatives
- Hedge funds: management fee (often 1-2% of assets) plus incentive fee (often 10-20%); hard hurdle charges only above the hurdle, soft charges on all gains once cleared; high-water mark; usual order: management fee first, incentive on the net gain (confirm in the offering documents). Watch lockups, gates, survivorship and backfill bias.
- Private equity: commitments, capital calls, J-curve. DPI = distributions/paid-in; RVPI = NAV/paid-in; TVPI = DPI + RVPI; IRR is the preferred return measure, MOIC ignores timing. Waterfall: return of capital, preferred return (often 8%), GP catch-up, carry split (often 80/20); European (whole-fund) vs American (deal-by-deal) with clawback.
- Real estate: value = NOI_1 / cap rate; DCF with exit cap; appraisal smoothing understates volatility; REITs use FFO and AFFO.
- Commodities: total return = spot change + roll yield + collateral yield; contango gives negative roll to longs, backwardation positive.
- Desmooth stale NAV returns before mixing with liquid assets in risk models.

## Output
Inputs and assumptions table, pricing result with an arbitrage or sanity check, hedge size with the residual risk, fee waterfall worked to net investor return.

## Pitfalls
American options priced with BSM; ignoring dividends in parity; hedges that add basis risk; quoting gross fund returns as investor returns.
