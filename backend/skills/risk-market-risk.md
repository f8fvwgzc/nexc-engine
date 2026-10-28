---
id: risk-market-risk
name: Market risk
category: risk
description: Measures and limits market risk with VaR (historical, parametric, Monte Carlo), expected shortfall under FRTB, stress tests, backtesting and Greeks-based sensitivity limits.
keywords: market risk, var, value at risk, expected shortfall, frtb, stress testing, backtesting, greeks, dv01, sensitivity limits, trading book, monte carlo, historical simulation
source: Basel Committee - MAR32 backtesting and P&L attribution test requirements | https://www.bis.org/committees/bcbs/basel-framework/81303/chapter.pdf
source: Basel Committee - MAR33 internal models approach capital requirements | https://www.bis.org/committees/bcbs/basel-framework/81333/chapter.pdf
source: Basel Committee - MAR99 guidance on use of the internal models approach | https://www.bis.org/committees/bcbs/basel-framework/81307/chapter.pdf
source: GARP - FRM program and exams | https://www.garp.org/frm/program-exams
---
## When to use
Measuring trading or treasury portfolio risk, setting and monitoring limits, designing stress tests, validating or backtesting a VaR/ES model, explaining a risk report.

## Methods
- Parametric VaR = z x sigma_p x sqrt(h) x V, with z = 1.645 (95%), 2.326 (99%); sigma_p = sqrt(w' Sigma w) from weights w and covariance Sigma; h in days. Fast; weak for options and fat tails.
- Historical simulation: reprice today's positions under each past daily move (e.g. 250-500 days) and read the loss quantile. No distribution assumption; only as good as the window.
- Monte Carlo: simulate risk factors with chosen distributions and correlations, full revaluation; handles non-linearity; model risk in the choices.
- Expected shortfall = mean loss beyond VaR. Under normality ES = sigma x phi(z) / (1 - alpha); ES 97.5% is about 2.34 sigma, close to VaR 99%. ES is subadditive; VaR is not.
- sqrt-of-time scaling assumes i.i.d. returns; state it when used.

## Basel FRTB reference (MAR)
- IMA capital: ES at 97.5% one-tailed, base horizon 10 days scaled to liquidity horizons of 10, 20, 40, 60 or 120 days, calibrated to a stressed period; multiplier m_c = 1.5 unless the supervisor raises it, plus the backtesting add-on (MAR33).
- Bank-wide backtest (MAR32.5-32.9): 1-day 99% VaR vs actual and hypothetical P&L over the last 250 days, counted separately; exceptions = the greater count. Green 0-4 (multiplier 1.50), amber 5-9 (1.70, 1.76, 1.83, 1.88, 1.92), red 10+ (2.00). Document a cause for every exception.
- Desk level (MAR32.19): more than 12 exceptions at 99% or 30 at 97.5% in 12 months moves the desk to the standardised approach. P&L attribution: green if Spearman above 0.80 and KS below 0.09; red if Spearman below 0.70 or KS above 0.12 (MAR32).
- National go-live dates and rule variants differ; verify locally.

## Backtesting statistics
Expected exceptions = (1 - confidence) x T, so 2.5 in 250 days at 99%. Kupiec POF: LR = -2 ln[(1-p)^(T-x) p^x] + 2 ln[(1-x/T)^(T-x) (x/T)^x], chi-squared with 1 df, reject above 3.84 at 95%. Christoffersen adds a test for clustered exceptions.

## Stress testing
Historical (2008 crisis, March 2020, 2022 rate shock), hypothetical, and reverse stress (find the scenario that breaks capital or a limit). Shock whole curves, spreads, vols and correlations; include basis and market liquidity; report P&L by risk factor.

## Sensitivities and limits
- Delta, gamma, vega, theta, rho; delta-gamma P&L = delta x dS + 0.5 x gamma x dS^2.
- DV01 = value change for a 1bp rate move; CS01 for spreads; dP/P = -D_mod x dy + 0.5 x C x dy^2.
- Limit set: VaR/ES at firm and desk, DV01 by tenor bucket, vega by expiry, stop-loss, concentration and stress-loss limits; utilisation reporting and a written escalation path for breaches.

## Output
Summary of VaR/ES, stress losses and limit use; method and assumptions (confidence, horizon, window, data date); backtest results; drivers of change; breaches and actions.

## Pitfalls
Treating VaR as a worst case; short windows without stress; mixing actual and hypothetical P&L; linear VaR on option books; ignoring basis and liquidity.
