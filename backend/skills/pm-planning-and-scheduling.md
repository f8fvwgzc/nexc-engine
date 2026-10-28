---
id: pm-planning-and-scheduling
name: Project planning and scheduling
category: project-management
description: Turns agreed scope into a resourced, baselined schedule (WBS, network logic, critical path, levelling) and controls it with earned value; use for project plans, schedule reviews and cost or date forecasts.
keywords: project plan, schedule, wbs, work breakdown structure, critical path, cpm, float, resource levelling, earned value, evm, cpi, spi, eac, baseline, gantt, pmbok
source: PMBOK Guide Eighth Edition principles, domains and focus areas | https://pmstudycircle.com/pmbok-guide-8th-edition/
source: Project Management Body of Knowledge (editions) | https://en.wikipedia.org/wiki/Project_Management_Body_of_Knowledge
source: GAO-16-89G Schedule Assessment Guide | https://www.gao.gov/products/gao-16-89g
source: GAO-20-195G Cost Estimating and Assessment Guide | https://www.gao.gov/products/gao-20-195g
source: Earned value management | https://en.wikipedia.org/wiki/Earned_value_management
---
## When to use
Building a plan from scope, checking a schedule's health, or reporting progress with a cost and finish forecast.

## Frame
- PMBOK Guide 8th edition (PMI, November 2025): 6 principles (adopt a holistic view, focus on value, embed quality, lead accountably, integrate sustainability, build an empowered culture), 7 performance domains (Governance, Scope, Schedule, Finance, Stakeholders, Resources, Risk), 5 focus areas (Initiating, Planning, Executing, Monitoring and Controlling, Closing) and 40 non-prescriptive processes. The 7th edition (2021: 12 principles, 8 domains) is still widely cited; say which edition you apply.

## Method
1. WBS: decompose deliverables, not activities. 100% rule: children add up exactly to the parent. Work packages one owner, estimable, about one reporting period or less. WBS dictionary: scope, acceptance criteria, owner, estimate.
2. Logic: every activity has a predecessor and a successor except the start and end milestones. Prefer finish-to-start; use SS/FF with documented lags; avoid leads and hard date constraints.
3. Durations: three-point ranges on a working calendar (see plan-estimation-and-risk).
4. Critical path: forward pass EF = ES + D; backward pass LS = LF - D. Total float TF = LS - ES; free float = earliest successor ES - EF. Critical path = longest path (lowest float). Watch near-critical paths and drag (how much one activity lengthens the project).
5. Resources: smoothing moves work only within float; levelling may move the finish and change the critical path.
6. Compression: crash the cheapest critical activity first (cost slope = (crash cost - normal cost) / (normal time - crash time)); fast-track by overlapping, accepting rework risk.
7. Baseline through change control; update actuals each period; re-forecast, never overwrite the baseline.

## Schedule health (GAO-16-89G)
Ten practices: capture all activities, sequence them, assign resources, set durations, trace horizontally and vertically, confirm a valid critical path, keep float reasonable, run schedule risk analysis, update with actuals and logic, keep a baseline. Reliable = comprehensive, well-constructed, credible, controlled. Screening thresholds from the DCMA 14-point check (verify locally): missing logic, lags, hard constraints, high float and high duration (over 44 working days) each at most 5% of tasks; no leads, no negative float, no invalid dates (actuals after the data date, forecasts before it); at least 90% finish-to-start links; BEI and CPLI at least 0.95.

## Earned value
BAC budget at completion; PV planned value (budget of work scheduled to date); EV earned value = % complete x that work's budget; AC actual cost.
- CV = EV - AC; SV = EV - PV; CPI = EV / AC; SPI = EV / PV.
- EAC = BAC / CPI (trend continues); AC + (BAC - EV) (rest at plan rate); AC + (BAC - EV) / (CPI x SPI) (cost and schedule pressure); AC + bottom-up ETC.
- ETC = EAC - AC; VAC = BAC - EAC; TCPI = (BAC - EV) / (BAC - AC). TCPI far above current CPI means the budget is unreachable.
- SPI drifts to 1.0 at the end even on late projects; use earned schedule: SPI(t) = ES / AT, forecast duration = planned duration / SPI(t).
- Claim EV by objective rules (0/100, 50/50, weighted milestones, units done), never by spend.
- Example: BAC 500k, PV 200k, EV 180k, AC 225k gives CPI 0.80, SPI 0.90, EAC 625k, VAC -125k.

## Output
WBS and dictionary; network or Gantt with the critical path marked; milestone table; resource histogram; EVM table (PV, EV, AC, CV, SV, CPI, SPI, EAC, VAC, TCPI) with commentary; assumptions and data dates.

## Pitfalls
Activity lists posing as a WBS; dangling logic; constraints forcing dates; gut-feel percent complete; quoting SPI near the finish; re-baselining to hide variance; ignoring calendars and shared resources.
