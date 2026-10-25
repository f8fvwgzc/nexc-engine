---
id: fin-equity-valuation
name: Equity valuation
category: finance
description: Values a company's equity with DCF (FCFF or FCFE), dividend discount, residual income and relative multiples, then cross-checks the terminal value and shows sensitivity to WACC and growth.
keywords: valuation, dcf, fcff, fcfe, wacc, capm, cost of equity, terminal value, multiples, ev/ebitda, residual income, dividend discount, ddm, intrinsic value, sensitivity
source: CFA Institute - Residual income valuation (2026 refresher reading) | https://www.cfainstitute.org/insights/professional-learning/refresher-readings/2026/residual-income-valuation
source: Aswath Damodaran - Current data: ERP, cost of capital, betas and multiples by industry | https://pages.stern.nyu.edu/~adamodar/New_Home_Page/datacurrent.html
---
## When to use
Intrinsic or relative value of a listed or private company, a fairness check on a price, or a reverse DCF of what the market implies.

## Choose the model
- FCFF at WACC: levered firms, changing capital structure, negative FCFE.
- FCFE at cost of equity: stable leverage. DDM: mature payers whose dividends track FCFE.
- Residual income: banks, insurers, negative FCF, meaningful book value.
- Multiples: cross-check, never the only method.

## Formulas
- FCFF = EBIT(1 - t) + D&A - capex - change in NWC = NI + non-cash charges + interest(1 - t) - capex - change in NWC. FCFE = FCFF - interest(1 - t) + net borrowing.
- CAPM: re = rf + beta x ERP (+ a country premium only with evidence). Relever: beta_L = beta_U x [1 + (1 - t) D/E].
- WACC = E/V x re + D/V x rd x (1 - t), target market-value weights.
- Gordon terminal value: TV_n = CF_(n+1) / (r - g), g below r and at or below long-run nominal GDP growth; consistency: g = reinvestment rate x return on new capital. Exit-multiple TV = multiple x metric_n; its implied g = (TV x r - CF_n)/(TV + CF_n).
- Mid-year convention: discount year-t flows by (1 + r)^(t - 0.5).
- Equity = EV - net debt - minorities - preferred - after-tax pension deficit + non-operating assets. Diluted shares (treasury stock method) = basic + in-the-money options x (1 - strike/price).
- DDM: V0 = D1/(r - g). H-model: V0 = D0(1 + gL)/(r - gL) + D0 x H x (gS - gL)/(r - gL), H = half the high-growth fade period.
- Residual income: RI_t = E_t - r x B_(t-1); V0 = B0 + sum over t = 1..T-1 of RI_t/(1 + r)^t + RI_T/[(1 + r - w)(1 + r)^(T-1)], persistence w from 0 (RI stops) to 1 (RI lasts forever).
- Justified leading P/E = payout/(r - g); justified P/B = (ROE - g)/(r - g).

## Method
1. Forecast drivers (growth, margins, reinvestment) from history, guidance and industry; explicit period of 5-10 years until steady state.
2. Build cash flows in nominal terms and the same currency as the discount rate.
3. Derive WACC or re with sourced inputs and dates.
4. Terminal value, then bridge EV to equity per share at the valuation date.
5. Multiples: match numerator and denominator (EV with EBITDA/EBIT/sales; price with EPS/book); peers with similar growth, risk and ROIC; medians; same fiscal basis.
6. Sensitivity: two-way table WACC +/-1 point x g +/-0.5-1 point; bear, base, bull cases with weights.

## Checks
- Terminal value share of EV (often 60-80%); above about 85% means a short explicit period.
- Implied exit multiple from the Gordon value vs peer multiples; terminal ROIC vs WACC; reverse-DCF growth vs history.
- Net debt, share count and cash flows all dated to the valuation date.

## Output
Value range and point estimate, method weights, assumptions table (input, value, source, date), sensitivity table, football-field summary, key risks.

## Pitfalls
FCFE discounted at WACC; book-value weights; double counting cash and interest income; ignoring share-based pay dilution; growth above the economy forever. A valuation is analysis, not a buy or sell instruction for any person.
