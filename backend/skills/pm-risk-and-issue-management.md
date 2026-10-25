---
id: pm-risk-and-issue-management
name: Project risk and issues
category: project-management
description: Runs a project risk register and issue log: cause-event-effect risks, probability x impact and EMV scoring, threat and opportunity responses, escalation by tolerance, RAID upkeep and contingency versus management reserve.
keywords: risk register, risk management, issue log, raid log, probability impact matrix, emv, risk response, mitigation, escalation, contingency reserve, management reserve, tolerance, exception report, roam, assumptions, dependencies
source: HM Treasury - The Orange Book: management of risk, principles and concepts | https://www.gov.uk/government/publications/orange-book
source: PRINCE2 principles, practices and processes | https://en.wikipedia.org/wiki/PRINCE2
source: SAFe - PI Planning | https://framework.scaledagile.com/pi-planning
source: GAO-20-195G Cost Estimating and Assessment Guide | https://www.gao.gov/products/gao-20-195g
source: ISO 31000 risk management | https://en.wikipedia.org/wiki/ISO_31000
---
## When to use
Setting up or reviewing a risk register, RAID log or issue log; sizing reserves; deciding whether something must be escalated.

## Risk register fields
ID; statement "Because <cause>, <event> may happen, causing <effect on an objective>"; category (technical, external, organisational, delivery); owner (one named person); probability; impact per objective (time, cost, scope, quality, benefits, safety, reputation); score; proximity; trend; response type and actions with owner and date; trigger or early-warning indicator; residual score; secondary risks; status. Opportunities go in the same register.

## Assessment
- Qualitative: 5 x 5 matrix. Define the bands before scoring, e.g. probability 1 = under 10%, 3 = 30-50%, 5 = over 70%; impact bands tied to money or days (cost as % of budget, delay in weeks). Score = P x I; set high/medium/low cut-offs from the organisation's risk appetite.
- Quantitative: EMV = p x impact (opportunities negative); Monte Carlo on the schedule and cost model for P50/P80; tornado charts for drivers; decision trees for options.
- ISO 31000:2018 process (confirmed 2023; check locally for a newer edition): communicate and consult (6.2); scope, context, criteria (6.3); assess = identify, analyse, evaluate (6.4.2-6.4.4); treat (6.5); monitor and review (6.6); record and report (6.7).
- Re-assess at every reporting cycle and at stage boundaries.

## Responses
- Threats: avoid, mitigate (cut p or impact), transfer (contract, insurance, warranty), accept actively (contingency) or passively, escalate (beyond the team's authority; the higher level must accept ownership).
- Opportunities: exploit, enhance, share, accept, escalate.
- PRINCE2 also uses fallback (a plan run only if the risk occurs) and share; check the current edition's list locally.
- SAFe trains ROAM PI risks: Resolved, Owned, Accepted, Mitigated.
- Response cost must be lower than the exposure it removes; record the residual risk.

## Issues and escalation
- Issue = happening now. Log: ID, type (request for change, off-specification, problem or concern), severity, priority, owner, date raised, decision needed, due date, status.
- Manage by exception: agree tolerances per level (time, cost, scope, quality, benefits, risk). A forecast breach of stage tolerance goes to the project board as an exception report; the board may approve an exception plan, change scope, or close.
- Escalation note: what happened, impact in days and money, options with consequences, recommendation, decision needed and by when.
- Age the log: an issue past its due date without a decision escalates one level.

## RAID log
Risks, Assumptions, Issues, Dependencies (some teams use D for Decisions). An assumption proved false or a risk that occurs becomes an issue. Each dependency has a provider, a need-by date and a status.

## Reserves
- Contingency reserve: for identified risks (known unknowns), inside the cost baseline, used by the project manager. Size it from the sum of threat EMVs net of opportunities, or the chosen confidence level (e.g. P80) minus the base estimate.
- Management reserve: for unknown unknowns, outside the cost (performance measurement) baseline but inside the project budget, released by the sponsor through change control.
- Cost baseline = work package estimates + contingency reserve; project budget = cost baseline + management reserve. Do not hide padding inside activities.

## Output
Register table sorted by score, heat map, top-five risks with responses and owners, open issues with decisions needed, reserve calculation, changes since last report.

## Pitfalls
Vague risks ("resources"), issues logged as risks, owners who are teams not people, scores never revisited, mitigations without dates, contingency spent on scope creep.
