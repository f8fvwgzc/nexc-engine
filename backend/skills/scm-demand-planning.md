---
id: scm-demand-planning
name: Demand planning and S&OP
category: supply-chain
description: Builds demand plans with statistical and consensus forecasting, measures accuracy and bias (MAPE, WAPE, MASE), adds demand sensing, and runs the monthly S&OP or IBP cycle that agrees one plan.
keywords: demand planning, forecasting, s&op, ibp, sales and operations planning, integrated business planning, forecast accuracy, mape, wape, forecast bias, tracking signal, demand sensing, consensus forecast, intermittent demand, croston, forecast value added
source: ASCM - 2026 CSCP module content outline (ECM 5.0) | https://www.ascm.org/globalassets/ascm_website_assets/docs/credentials/2026-cscp-module-content-outline.pdf
source: Hyndman and Athanasopoulos - Forecasting: Principles and Practice, evaluating point forecast accuracy | https://otexts.com/fpp3/accuracy.html
source: Nixtla statsforecast - open-source statistical forecasting models | https://github.com/Nixtla/statsforecast
---
## When to use
Forecasting demand by SKU, family or region; designing or running S&OP/IBP; diagnosing poor accuracy or bias; picking models for intermittent items.

## Method
1. Define the forecast: level (SKU-location, family), bucket, horizon (at least cumulative lead time; S&OP 18-24 months) and the lag you measure (lag 1 = forecast frozen one bucket before the actual).
2. Clean history: model demand, not stockout-constrained shipments; move promotions and one-offs into an event layer; flag launches and phase-outs.
3. Segment: ABC by volume or value, XYZ by coefficient of variation; intermittent items by ADI (average interval between demands) and CV squared of non-zero sizes (Syntetos-Boylan cut-offs 1.32 and 0.49: smooth, intermittent, erratic, lumpy).
4. Models: naive and seasonal naive as benchmarks; ETS, ARIMA, Theta for smooth series; Croston, SBA, TSB or ADIDA for intermittent; driver regression (price, promotion, weather) when data supports it. Select by rolling-origin cross-validation at the decision lag, never by in-sample fit.
5. Demand sensing: days to about 6 weeks, using orders, POS, sell-through and channel stock; it refines the near term and must not overwrite the S&OP horizon.
6. Consensus: overrides by sales, marketing or key accounts are logged with owner, reason and size; measure the Forecast Value Added (FVA) of each step; remove steps that add nothing.
7. Reconcile the hierarchy (top-down, bottom-up or optimal reconciliation) and show units and value.

## Formulas
Error e_t = A_t - F_t (actual minus forecast; state the convention).
- MAPE = mean(|e_t| / A_t) x 100. Undefined at zero actuals, explodes on small ones, punishes over-forecasts harder; not for intermittent items.
- WAPE = sum|e_t| / sum A_t x 100. Volume-weighted, best for portfolios; accuracy is often reported as 1 - WAPE.
- Bias = sum(F_t - A_t) / sum A_t. Positive means over-forecast.
- Tracking signal = sum e_t / MAD. With e = A - F, a large positive value means persistent under-forecasting; investigate beyond about +/-4 (set locally).
- MASE = MAE / in-sample MAE of the one-step naive (or seasonal naive) forecast; below 1 beats naive.
- FVA of a step = error of its input forecast - error of its output, same metric, level and lag; negative means the step made the forecast worse.
- Croston: forecast per period = smoothed non-zero demand size / smoothed interval. SBA multiplies it by (1 - alpha/2) to remove Croston's upward bias; TSB smooths the probability of demand, so a dying item decays towards zero.
Report at the level and lag where decisions are made, for example SKU-DC at lead-time lag.

## S&OP / IBP monthly cycle
1. Data and statistical forecast. 2. Demand review: unconstrained plan, assumptions, risks and opportunities. 3. Supply review: capacity and material constraints, constrained plan, scenarios. 4. Integrated reconciliation (pre-S&OP): gap to financial plan, options, escalations. 5. Executive meeting: decide, approve one plan, minute decisions. IBP adds portfolio and financial plans in value over 24-36 months.

## Output
Demand plan by family and month in units and value; assumptions log; WAPE, bias and FVA by lag; risks and opportunities in value; scenarios; gap to budget with closing options; decisions requested.

## Pitfalls
Forecasting constrained shipments; raising the forecast to meet a budget or target instead of showing the gap; averaging MAPE over SKUs of very different volume; good average accuracy hiding bias; frequent tiny overrides; no lag definition.
