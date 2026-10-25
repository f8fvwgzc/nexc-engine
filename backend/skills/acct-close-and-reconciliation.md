---
id: acct-close-and-reconciliation
name: Close and reconciliation
category: accounting
description: Runs a month-end close: calendar and dependencies, accruals and deferrals, cut-off, account reconciliations, journal entry controls and close KPIs, for any ledger or ERP.
keywords: month-end close, close calendar, reconciliation, account reconciliation, bank reconciliation, intercompany, accruals, deferrals, cut-off, journal entries, flux analysis, days to close
source: Anthropic knowledge-work-plugins - close-management skill | https://raw.githubusercontent.com/anthropics/knowledge-work-plugins/main/finance/skills/close-management/SKILL.md
source: Anthropic knowledge-work-plugins - reconciliation skill | https://raw.githubusercontent.com/anthropics/knowledge-work-plugins/main/finance/skills/reconciliation/SKILL.md
source: CBIZ - Monthly close best practices | https://www.cbiz.com/insights/article/monthly-close-best-practices-for-growing-companies
---
## When to use
Designing, running or fixing a month, quarter or year-end close; preparing accrual or deferral entries; reviewing reconciliations; setting close KPIs.

## Close calendar (T = business days after period end)
| Day | Work |
|---|---|
| T-3 to T-1 | Publish calendar and cut-off rules; confirm subledger jobs; review open POs and contracts for accruals; preliminary bank rec |
| T+1 | Close AP, AR, payroll, fixed assets; post cash, depreciation, prepaid amortisation, intercompany charges |
| T+2 | Revenue and deferred revenue; remaining accruals; inventory; FX revaluation; start balance-sheet recs |
| T+3 | Finish recs; intercompany matching and eliminations; preliminary trial balance; flux analysis |
| T+4 | Tax provision; equity roll-forward; consolidation; draft statements; management review |
| T+5 | Final adjustments; lock the period; reporting package; retrospective |
Critical path: cash/AP/AR entries, subledger recs, balance-sheet recs, tax, draft statements, review, hard close.

## Accruals and deferrals
- Accrued expense: received, not invoiced (Dr expense, Cr accrued liabilities); estimate from receipts, contracts or run-rate; auto-reverse next period.
- Performed, not billed: contract asset (right still conditional on further performance) or receivable (only time must pass). Billed or paid before performance: contract liability (deferred revenue).
- Prepaid: amortise over the benefit period. Keep basis, owner and last month's true-up (estimate accuracy).

## Cut-off
Test items either side of period end: shipping terms and control transfer, goods-received vs AP, last cheque and deposit numbers, quarter-end revenue spikes, credit notes raised after period end.

## Reconciliations
- Types: bank (outstanding cheques, deposits in transit, bank charges); GL to subledger (AR, AP, fixed assets, inventory); intercompany pairs that must net to zero; substantiation of accruals, prepaids, suspense and clearing accounts.
- Bank rec: adjusted bank = statement balance + deposits in transit - outstanding cheques +/- bank errors; adjusted book = GL cash + unrecorded receipts and interest - unrecorded charges +/- book errors; both must agree, and only book-side items are posted.
- Steps: same-date balances, compare, classify each item as timing (clears in days), adjustment (post an entry) or investigate (unexplained, aged, recurring), attach support, preparer and reviewer sign-off with dates.
- Aging: 0-30 days monitor, 31-60 investigate, 61-90 escalate to supervisor, over 90 to management for adjustment or write-off.
- Frequency by risk: material accounts monthly, low-risk quarterly; suspense monthly and expected at zero.

## Journal entry controls
Preparer differs from approver; amount-based approval limits; support attached; restricted posting rights; system period locks; post-close entries need controller approval. Review high-risk entries: unusual or rarely used accounts, unexpected users, posted at or after period end, round amounts, blank descriptions, top-side consolidation entries, early reversals.

## Output
Calendar table (task, owner, day, dependency, status), reconciliation pack, JE log, flux analysis (e.g. over 10 percent and over a set amount, with driver narrative), close memo. KPIs: days to close, post-close adjustments, late tasks, recs on time, aged items, restatements (target zero).

## Pitfalls
Reconciling to a number instead of substantiating it; plugging to suspense; sign-off without evidence; timing items that never clear; one person posting and reconciling the same account.
