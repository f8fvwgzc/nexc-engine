---
id: risk-credit-risk
name: Credit risk
category: risk
description: Quantifies credit risk with PD, LGD, EAD and expected loss, rates obligors and validates scorecards, stages IFRS 9 ECL, measures counterparty exposure and CVA, and checks concentration and Basel capital.
keywords: credit risk, pd, lgd, ead, expected loss, ifrs 9, ecl, staging, scorecard, credit rating, counterparty risk, cva, concentration, basel irb, rwa, cecl
source: Basel Committee - CRE31 IRB approach risk weight functions | https://www.bis.org/committees/bcbs/basel-framework/81350/chapter.pdf
source: PKF - IFRS 9 expected credit losses overview | https://www.pkf-l.com/insights/ifrs-9-expected-credit-losses-an-overview/
source: GARP - FRM program and exams | https://www.garp.org/frm/program-exams
---
## When to use
Credit memos and ratings, loss provisioning (IFRS 9 / CECL), scorecard development or validation, counterparty limits, portfolio concentration and capital estimates.

## Core measures
- EL = PD x LGD x EAD. LGD = 1 - recovery rate, net of workout costs and discounted. EAD = drawn + CCF x undrawn.
- Single-name unexpected loss (simple) = EAD x LGD x sqrt(PD x (1 - PD)).
- Basel IRB, corporate/sovereign/bank (CRE31): K = [LGD x N((1-R)^-0.5 x G(PD) + (R/(1-R))^0.5 x G(0.999)) - PD x LGD] x (1 + (M - 2.5) x b) / (1 - 1.5 x b); R = 0.12 x w + 0.24 x (1 - w), w = (1 - e^(-50 PD)) / (1 - e^(-50)); b = (0.11852 - 0.05478 x ln PD)^2; RWA = K x 12.5 x EAD. N = standard normal CDF, G = its inverse, M = effective maturity. Correlation x1.25 for large regulated (assets USD 100bn+) and unregulated financials. Defaulted: K = max(0, LGD - best-estimate EL) (CRE31.3). PD input floor 0.05% for all but sovereigns (CRE32.4); national implementations differ, so name the rulebook.

## Ratings and scorecards
- Judgemental rating: business risk plus financial risk (debt/EBITDA, EBITDA/interest, FFO/debt, liquidity), then notch for support and country.
- Statistical scorecards: logistic regression with WoE bins. Discrimination: Gini = 2 x AUC - 1, KS. Calibration: predicted PD vs observed default rate (binomial test). Stability: PSI below 0.10 stable, 0.10-0.25 watch, above 0.25 shifted.
- Altman Z = 1.2X1 + 1.4X2 + 3.3X3 + 0.6X4 + 1.0X5 (public manufacturers): below 1.81 distress, above 2.99 safe.
- Merton distance to default = [ln(V/D) + (mu - sigma^2/2)T] / (sigma sqrt(T)).

## IFRS 9 expected credit loss
- Stage 1: 12-month ECL. Stage 2: lifetime ECL after a significant increase in credit risk (SICR) since origination; more than 30 days past due is a rebuttable SICR presumption. Stage 3: credit-impaired, lifetime ECL, interest on net carrying amount; default is rebuttably presumed at 90 days past due. POCI assets: lifetime from purchase.
- ECL = sum over t of marginal PD_t x LGD_t x EAD_t x DF_t, discounted at the effective interest rate and probability-weighted over a range of outcomes (IFRS 9 needs at least the chance of a loss and of none; base, upside and downside scenarios are common practice). Use point-in-time, forward-looking PDs, not regulatory through-the-cycle ones.
- SICR triggers: lifetime PD rise vs origination (relative and absolute), watchlist, forbearance, downgrade. Document overlays. US GAAP CECL: lifetime loss from day one.

## Counterparty risk and CVA
- Metrics: current exposure, PFE at a high quantile, EE, EPE; reduce with netting and collateral (CSA thresholds, margin period of risk).
- SA-CCR: EAD = 1.4 x (RC + PFE).
- CVA = LGD x sum of EE(t_i) x PD(t_i-1, t_i) x DF(t_i); watch wrong-way risk.

## Concentration
HHI = sum of squared exposure shares; single-name, sector and country limits; Basel large exposures cap 25% of Tier 1 per connected group (15% between G-SIBs).

## Output
Credit memo: borrower, purpose, structure, financial analysis, rating and PD, LGD and collateral, covenants, EL and RWA, recommendation and conditions; for portfolios, stage migration and ECL drivers.

## Pitfalls
Mixing point-in-time and through-the-cycle PDs; ignoring undrawn lines; no collateral haircuts; stale macro scenarios; overlays without rationale.
