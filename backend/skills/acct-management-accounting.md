---
id: acct-management-accounting
name: Management accounting
category: accounting
description: Builds budgets and rolling forecasts, explains variances with standard costing formulas, costs products with absorption or activity-based methods, and runs CVP, break-even and unit economics.
keywords: budget, rolling forecast, variance analysis, standard costing, activity-based costing, abc, contribution margin, cvp, break-even, unit economics, ltv, cac, cost accounting, fp&a
source: Anthropic knowledge-work-plugins - variance-analysis skill | https://raw.githubusercontent.com/anthropics/knowledge-work-plugins/main/finance/skills/variance-analysis/SKILL.md
source: ACCA - Performance Management syllabus and study guide | https://www.accaglobal.com/gb/en/student/exam-support-resources/fundamentals-exams-study-resources/f5/syllabus-study-guide.html
---
## When to use
Budgeting and forecasting, budget-vs-actual reviews, product or customer profitability, pricing, make-or-buy and other short-term decisions, break-even and unit economics.

## Budgets and forecasts
- Static budget (fixed volume), flexible budget (restated to actual volume), rolling forecast (e.g. 12-18 months ahead, refreshed monthly or quarterly).
- Driver-based: revenue = volume x price; costs split into fixed, variable and step; payroll = heads x loaded cost.
- Accuracy: MAPE = mean(|A - F| / |A|); bias = sum(F - A) / sum(A).

## Variance formulas (standard costing)
SP/AP = standard/actual price, AQ = actual quantity, SQ = standard quantity for actual output, SR/AR = standard/actual rate, AH = actual hours, SH = standard hours for actual output.
- Bridge: static budget -> flexed budget (sales volume variance) -> actual (flexible-budget variances below).
- Material price = (AP - SP) x AQ (AQ purchased if inventory is held at standard); material usage = (AQ used - SQ) x SP, split into mix and yield when inputs substitute.
- Labour rate = (AR - SR) x AH; labour efficiency = (AH - SH) x SR.
- Variable overhead spending = AH x (actual rate - standard rate); efficiency = (AH - SH) x standard rate.
- Fixed overhead spending = actual - budgeted; volume = budgeted - applied (SH x fixed rate).
- Sales price = (actual - budget price) x actual units; sales volume = (actual - budget units) x standard contribution per unit, split into mix and quantity when there are several products.
- Sign: for costs, positive = adverse; for sales, positive = favourable. With AQ used in both, price + usage = actual - flexed cost; check it. Investigate when above a set percentage and amount, recurring or controllable; split planning from operational variances when the standard was unrealistic.

## Costing
- Absorption vs variable costing: profit gap = change in inventory units x fixed overhead rate per unit.
- Activity-based costing: activities, cost pools, drivers; rate = pool cost / driver volume; charge products by driver use. Worth it when overhead is large and products differ in complexity.
- Target cost = target price - required margin; life-cycle costing; throughput = sales - totally variable costs.

## CVP and decisions
- Contribution margin (CM) = price - variable cost per unit; CM ratio = CM / price.
- Break-even units = fixed costs / CM per unit; break-even revenue = fixed costs / CM ratio; target-profit units = (fixed costs + target profit) / CM.
- Margin of safety = (sales - break-even sales) / sales; operating leverage = contribution / operating profit.
- Several products: weighted average CM at the planned mix.
- Relevant costing: future incremental cash flows and opportunity costs only; ignore sunk and committed costs. With one scarce resource rank by contribution per unit of that resource.

## Unit economics
- CAC = sales and marketing spend / new customers; LTV = (ARPU x gross margin %) / churn rate per period; CAC payback (months) = CAC / (monthly ARPU x gross margin %). Use margin, not revenue, and cohort data.

## Output
Budget-to-actual bridge with 5-8 drivers, variance table (amount, %, favourable or adverse, driver, owner, action), assumption register, decision memo with sensitivities.

## Pitfalls
Totals without drivers; absorption unit costs used for short-term decisions; ignoring mix; comparing actuals with a static budget; LTV from revenue or without churn by cohort.
