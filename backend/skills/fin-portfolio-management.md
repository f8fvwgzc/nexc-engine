---
id: fin-portfolio-management
name: Portfolio management
category: finance
description: Builds and reviews portfolios from an investment policy statement: return and risk objectives, constraints, MPT and CAPM, asset allocation, rebalancing, risk-adjusted measures, attribution and behavioural biases.
keywords: portfolio management, investment policy statement, ips, asset allocation, rebalancing, mpt, capm, sharpe ratio, sortino, information ratio, tracking error, performance attribution, behavioural finance, risk tolerance
source: CFA Institute - Basics of portfolio planning and construction (2026) | https://www.cfainstitute.org/insights/professional-learning/refresher-readings/2026/basics-of-portfolio-planning-and-construction
source: CFA Institute - Code of Ethics and Standards of Professional Conduct | https://cfainstitute.org/standards/professionals/code-ethics-standards
---
## When to use
Drafting an IPS, proposing or testing an allocation, judging a manager, explaining performance, or spotting biases in a client or committee.

## Investment policy statement
- Client profile, purpose, duties of each party, review schedule.
- Return objective: required (to fund goals) vs desired; multiplicative build: (1 + spending)(1 + inflation)(1 + costs) - 1.
- Risk objective: ability (wealth, horizon, income stability, liabilities) and willingness (attitude); when they conflict use the lower and educate the client.
- Constraints (LTTLU): liquidity, time horizon, taxes, legal and regulatory, unique circumstances incl. ESG preferences.
- Benchmark, strategic allocation with ranges, rebalancing policy, reporting.

## Theory and formulas
- Two-asset variance: sigma_p^2 = w1^2 sigma1^2 + w2^2 sigma2^2 + 2 w1 w2 rho sigma1 sigma2. Diversification gains rise as rho falls.
- Utility U = E(R) - 0.5 A sigma^2 (A = risk aversion). Optimal portfolio sits where the capital allocation line touches the highest indifference curve.
- CAPM: E(Ri) = rf + beta_i [E(Rm) - rf]; beta = Cov(i, m)/Var(m). Jensen's alpha = Rp - [rf + beta_p (Rm - rf)].
- Sharpe = (Rp - rf)/sigma_p. Treynor = (Rp - rf)/beta_p (for a sleeve of a diversified whole). Sortino = (Rp - MAR)/downside deviation below MAR. Information ratio = (Rp - Rb)/tracking error, TE = stdev(Rp - Rb). M-squared = rf + Sharpe_p x sigma_m; M-squared alpha = M-squared - Rm. Roy's safety-first = (E(Rp) - R_L)/sigma_p: pick the highest for a shortfall threshold R_L. Also report maximum drawdown.

## Asset allocation and rebalancing
- Strategic allocation: mean-variance with constraints, resampling or Black-Litterman to tame input error; liability-relative (surplus) for pensions and insurers; goals-based buckets for individuals; risk budgeting.
- Tactical tilts and security selection are separate, measurable decisions.
- Rebalancing: calendar or corridor. Widen corridors with higher costs, higher risk tolerance, higher correlation with the rest of the portfolio; narrow them with higher volatility.

## Performance evaluation
- Time-weighted return for managers, money-weighted for investor experience.
- Brinson-Fachler by segment i: allocation = (wp_i - wb_i)(Rb_i - Rb); selection = wb_i (Rp_i - Rb_i); interaction = (wp_i - wb_i)(Rp_i - Rb_i). Effects sum to active return.
- Valid benchmark: specified in advance, appropriate, measurable, unambiguous, reflects current manager views, accountable, investable.

## Behavioural biases
- Cognitive (moderate with education and process): conservatism, confirmation, representativeness, illusion of control, hindsight, anchoring, mental accounting, framing, availability.
- Emotional (adapt to them): loss aversion, overconfidence, self-control, status quo, endowment, regret aversion.

## Output
IPS or proposal with objectives and constraints table, allocation with expected return and risk (stated assumptions and dates), rebalancing rule, attribution table, risks.

## Pitfalls
Optimising on raw historical means; Sharpe ratio on non-normal or smoothed returns; ignoring taxes and costs; recommending products to a specific person instead of giving general analysis and pointing to a licensed adviser.
