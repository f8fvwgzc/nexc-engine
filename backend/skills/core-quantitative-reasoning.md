---
id: core-quantitative-reasoning
name: Quantitative reasoning
category: core
description: Makes numeric work checkable: carries units, shows arithmetic, runs order-of-magnitude and sum checks, separates percent from percentage points, rounds correctly and adds a sensitivity table.
keywords: calculation, arithmetic, units, percent, percentage points, basis points, rounding, significant figures, sanity check, order of magnitude, sensitivity, cagr, growth rate, estimate
source: NIST SP 811 (2008 ed.) Guide for the Use of the SI, sections 7.10 and B.7 | https://nvlpubs.nist.gov/nistpubs/Legacy/SP/nistspecialpublication811e2008.pdf
source: NIST - Special Publication 811 status page | https://www.nist.gov/pml/special-publication-811
source: Mathwords - Percentage points | https://www.mathwords.com/p/percentage_points.htm
---
## When to use
Any step with a number in it: sizing, forecasts, budgets, ratios, unit conversions, survey results, comparisons.

## Method
1. Units first. Write the unit beside every value and carry it through each line; check the result's dimensions. Convert everything to one basis before combining: currency with ISO code and price year (USD 2025, nominal vs real), time base (per month vs per year), gross vs net.
2. Show the arithmetic: formula, then values substituted with units, then the result. Keep full precision in intermediate steps; round only what you report. Re-derive key results by a second route (top-down vs bottom-up, or reverse calculation).
3. Sanity checks:
   - Order of magnitude: break the quantity into factors (users x frequency x price) and check each is within a factor of about 2-3 of a known anchor.
   - Parts sum to the total; shares sum to 100 % within rounding; signs and directions make sense.
   - Per-unit tests: per head, per customer, per day. Compare with a benchmark you can cite.
4. Percent vs percentage points. A rate moving from 4 % to 6 % rose 2 percentage points (pp) and 50 % in relative terms. 1 basis point = 0.01 pp. Successive changes compound: +10 % then -10 % is -1 %, not 0.
5. Rounding (NIST SP 811 App. B.7.1): if the dropped digits start below 5, keep the last digit; if they start with 5 and any later digit is non-zero (or start above 5), round up; for an exact 5 followed only by zeros, round to the even digit. Report no more significant figures than the weakest input supports. For a unit conversion (B.7.2), keep the digits whose relative rounding error best matches the original's: 36 ft = 10.9728 m is reported as 11.0 m, because 11 m would lose information (about 4.5 % rounding error against 1.4 % for "36"). Note "totals may not sum due to rounding" when they do not.
6. Sensitivity: name the 3-5 drivers; vary each over a plausible low/base/high range with others held; rank by swing (tornado order); give a two-way table for the top two; state the breakeven value at which the conclusion flips.

## Formulas
- Relative change = (new - old) / old.
- CAGR = (end / start)^(1/n) - 1, n = number of years.
- Weighted mean = sum(w_i x_i) / sum(w_i); never average percentages with different bases unweighted.
- Breakeven volume = fixed cost / (price - variable cost per unit).
- Expected value = sum(p_i x outcome_i), probabilities summing to 1.
- Rate conversion: annual = (1 + r_month)^12 - 1; never multiply a compound monthly rate by 12.
- Real value = nominal value x (price index of base year / price index of the value's year).

## Style
NIST SP 811 (sec. 7.2, 7.10.2) puts a space between number and unit, including "25 %"; business documents often write 25%. Pick one and keep it. Avoid "ppm/ppb" in technical text (sec. 7.10.3); write µg/g or 1 x 10^-6. Note that SP 811 predates the 2019 SI redefinition; NIST SP 330 (2019) carries the current SI.

## Output
Assumptions table (input | value | unit | source | range), worked calculation, result with units and rounding stated, sensitivity table, and checks performed.

## Pitfalls
- "Fell 2 %" when it fell 2 percentage points.
- Average of ratios vs ratio of averages; ignoring the denominator or base year.
- Mixing nominal and real money, or monthly and annual rates.
- False precision on soft inputs; Simpson's paradox when pooling groups.
