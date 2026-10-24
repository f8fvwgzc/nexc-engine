---
id: fin-financial-statement-analysis
name: Financial statement analysis
category: finance
description: Analyses the income statement, balance sheet and cash flow statement with ratios, DuPont, cash-flow and earnings-quality tests, and adjusts for IFRS vs US GAAP differences before comparing companies.
keywords: financial statement analysis, ratio analysis, dupont, roe, roic, cash flow, free cash flow, quality of earnings, accruals, ifrs, us gaap, leases, ifrs 18, working capital, fra
source: IFRS Foundation - IFRS 18 Presentation and Disclosure in Financial Statements | https://www.ifrs.org/issued-standards/list-of-standards/ifrs-18-presentation-and-disclosure-in-financial-statements/
source: Deloitte DART - FASB clarifies effective date of ASU 2024-03 (expense disaggregation) | https://dart.deloitte.com/USDART/home/news/all-news/2025/jan/fasb-effective-date-asu-disaggregation-income-statement
source: IFRS Foundation - IFRS 18 project summary (April 2024), incl. IAS 7 amendments | https://www.ifrs.org/content/dam/ifrs/project/primary-financial-statements/ifrs-standard/projectsummary-ifrs18-april2024.pdf
---
## When to use
Reading or comparing company accounts, building a model's history, or testing earnings quality before valuation or credit work.

## Method
1. Tie the statements: net income less dividends moves retained earnings; the cash flow statement explains the change in balance-sheet cash; non-cash items and working-capital changes bridge net income to CFO.
2. Common-size (income statement as % of revenue, balance sheet as % of assets); trend 3-5 years; compare 2-4 peers on the same basis.
3. Ratios (use average balances for flows over stocks):
   - Liquidity: current = CA/CL; quick = (cash + short-term investments + receivables)/CL; cash conversion cycle = DSO + DIO - DPO, with DSO = 365 x receivables/revenue, DIO = 365 x inventory/COGS, DPO = 365 x payables/COGS.
   - Solvency: debt/EBITDA, debt/equity, interest cover = EBIT/interest expense.
   - Returns: ROA = NI/assets; ROIC = NOPAT/(debt + equity - cash), NOPAT = EBIT x (1 - t).
   - DuPont: ROE = NI/sales x sales/assets x assets/equity. Five-way: tax burden (NI/EBT) x interest burden (EBT/EBIT) x EBIT margin x asset turnover x leverage.
4. Cash flow, interest paid inside CFO (US GAAP): FCFF = CFO + interest x (1 - t) - capex; FCFE = CFO - capex + net borrowing. Interest paid in financing (IFRS 18 default): FCFF = CFO - interest x t - capex; FCFE = CFO - interest - capex + net borrowing. Cash conversion = CFO/NI; persistently below 1 flags accruals.
5. Earnings quality: balance-sheet accrual ratio = change in net operating assets / average NOA. Red flags: receivables or inventory growing faster than sales, capitalised operating costs, longer useful lives, falling allowances, recurring "non-recurring" charges, bill-and-hold or related-party sales, auditor change, widening gap to adjusted measures.

## IFRS vs US GAAP
| Item | IFRS | US GAAP |
|---|---|---|
| Inventory | LIFO banned; write-downs reversible | LIFO allowed; no reversal |
| PP&E, intangibles | revaluation model allowed | cost model |
| Impairment reversal | allowed except goodwill | not allowed for assets held and used |
| Development costs | capitalised when IAS 38 criteria are met | expensed (software rules aside) |
| Lessee leases | IFRS 16: depreciation + interest | ASC 842: operating leases show one straight-line cost |
| Interest and dividends in cash flows | IAS 7 choice until IFRS 18; then most entities: paid in financing, received in investing; indirect method starts from operating profit | interest paid and received, dividends received in operating; dividends paid in financing |
| Presentation | IFRS 18 replaces IAS 1 for annual periods from 1 Jan 2027 (comparatives restated): operating, investing, financing categories; operating profit and profit before financing and income taxes; MPM note; by-function presenters disclose D&A, staff costs, impairments, inventory write-downs | ASU 2024-03 expense disaggregation (ASU 2025-01): annual periods beginning after 15 Dec 2026, interim after 15 Dec 2027 |

## Analyst adjustments
- Treat lease liabilities as debt; restate EBITDA consistently across lease models.
- LIFO to FIFO: inventory + LIFO reserve; COGS - change in reserve; equity + reserve x (1 - t).
- Strip one-offs, normalise the tax rate, add after-tax pension deficits to debt, treat share-based pay as a cost.

## Output
Findings first; ratio table with formula, period and source page; each adjustment with amount; red flags with evidence; open questions.

## Pitfalls
Mixing year-end and average balances; comparing across GAAP without adjusting; treating EBITDA as cash; ignoring leases and dilution.
