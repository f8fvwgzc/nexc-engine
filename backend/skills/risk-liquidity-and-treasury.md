---
id: risk-liquidity-and-treasury
name: Liquidity and treasury risk
category: risk
description: Assesses liquidity and treasury risk: Basel LCR and NSFR, ALM repricing and maturity gaps, cash forecasting, funding plans, FX and interest-rate hedging, and contingency funding plans.
keywords: liquidity risk, lcr, nsfr, hqla, alm, gap analysis, cash forecasting, funding plan, treasury, fx hedging, interest rate risk, irrbb, contingency funding plan, alco
source: Basel Committee - LCR standard (consolidated Basel Framework) | https://www.bis.org/committees/bcbs/basel-framework/LCR/standard.pdf
source: BIS FSI - Net stable funding ratio executive summary | https://www.bis.org/publications/fsi-summary-net-stable-funding-ratio-nsfr-executive-summary
source: BIS FSI - Liquidity coverage ratio executive summary | https://www.bis.org/publications/fsi-summary-liquidity-coverage-ratio-lcr-executive-summary
source: Basel Committee - SRP supervisory review process, incl. SRP31 IRRBB | https://www.bis.org/committees/bcbs/basel-framework/SRP/standard.pdf
---
## When to use
Bank or corporate liquidity reviews, ALCO packs, regulatory ratio checks, cash forecasts, funding and hedging plans, contingency funding plans.

## Basel ratios
- LCR = stock of HQLA / total net cash outflows over the next 30 calendar days, at least 100%. Level 1 (cash, central bank reserves, qualifying sovereigns): no haircut or cap. Level 2A: 15% haircut. Level 2B: 25% (eligible RMBS) to 50% (corporate debt, equities). Caps after haircuts: Level 2 at most 40% of HQLA, Level 2B at most 15%.
- Outflow run-off floors (LCR40): stable retail and small business deposits 5% (3% where allowed), less stable 10%, operational deposits 25%, non-financial corporates and sovereigns 40% (20% if fully insured), other legal entities including financials 100%. Inflows count only up to 75% of outflows.
- NSFR = available stable funding / required stable funding, at least 100%, one-year view; ASF weights run from 100% (capital, funding over one year) down to 0%, RSF weights from 0% (unencumbered cash-like assets) up to 100%.
- IRRBB (Basel SRP31, Pillar 2): EVE and NII sensitivity under the six prescribed shocks (parallel up and down, steepener, flattener, short rates up and down; sizes by currency, SRP31.90-31.93). Outlier test: the largest EVE decline across the six is compared with 15% of Tier 1 capital (SRP31.82); some regimes add stricter tests, so check the local rule.

## ALM and gaps
- Repricing gap per bucket = rate-sensitive assets - rate-sensitive liabilities; change in NII ~ gap x rate change x fraction of the year remaining.
- Maturity gap: contractual and behavioural flows per bucket, cumulative gap, survival horizon = days until stressed cumulative net outflow exceeds the buffer.
- Duration gap = D_A - (L/A) x D_L; change in EVE ~ -duration gap x A x dy / (1 + y).
- Document behavioural assumptions (core vs volatile deposits, prepayments, line drawdowns) and back-test them.

## Corporate treasury
- Cash forecast: 13-week direct (receipts and payments) for liquidity, indirect 12-24 months for planning; weekly actual vs forecast. Cash conversion cycle = DSO + DIO - DPO.
- Headroom = cash + undrawn committed facilities - minimum operating cash; test covenants (leverage, interest cover) under downside cases.
- Funding plan: diversify sources and tenors, maturity ladder, limit refinancing concentration in any 12 months, consider ratings and cost of funds.

## Hedging
- Classify exposures: transactional, translational, economic. Policy sets hedge ratios by horizon, instruments (forwards, swaps, options, cross-currency swaps), counterparties and approvals.
- Forward = spot x (1 + r_quote x T) / (1 + r_base x T). Swaps turn floating into fixed or back.
- Hedge accounting (IFRS 9, ASC 815) needs designation and effectiveness evidence; otherwise P&L volatility.

## Contingency funding plan
Early warning indicators (deposit outflows, wider CDS, rating outlook, margin calls, funding cost spikes); idiosyncratic, market-wide and combined scenarios; triggers and escalation to ALCO or a crisis team; actions ranked by time-to-cash and cost (repo of HQLA, central bank facilities, asset sales, deposit pricing); communications plan; test at least yearly.

## Output
Ratio table with components, gap tables by bucket, stress survival horizon, funding and hedge positions vs policy, breaches, actions.

## Pitfalls
Counting encumbered assets as HQLA; ignoring caps after haircuts; contractual-only flows for non-maturity deposits; hedges with no documented exposure; a CFP never tested.
