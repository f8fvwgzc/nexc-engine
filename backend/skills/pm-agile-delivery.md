---
id: pm-agile-delivery
name: Agile delivery
category: project-management
description: Sets up and runs agile and hybrid delivery: Scrum events and commitments, Kanban flow metrics and WIP limits, SAFe PI planning and WSJF, and mapping agile work onto PRINCE2 7 governance.
keywords: agile, scrum, sprint, kanban, wip limit, flow metrics, cycle time, throughput, safe, pi planning, wsjf, hybrid, prince2, velocity, burn-up, definition of done
source: The 2020 Scrum Guide | https://scrumguides.org/scrum-guide.html
source: The Kanban Guide (May 2025) | https://kanbanguides.org/english/
source: SAFe - Weighted Shortest Job First | https://framework.scaledagile.com/wsjf
source: SAFe - PI Planning | https://framework.scaledagile.com/pi-planning
source: PRINCE2 principles, practices and processes | https://en.wikipedia.org/wiki/PRINCE2
---
## When to use
Choosing a delivery approach, setting up a team's cadence, forecasting agile work, or reconciling agile teams with stage-gated governance.

## Scrum (Scrum Guide, November 2020, still the current text)
- One Scrum Team, typically 10 or fewer: Product Owner, Scrum Master, Developers; self-managing.
- Sprints of one month or less. Timeboxes for a one-month Sprint (shorter Sprints, usually shorter events): Sprint Planning 8 h (why, what, how), Daily Scrum 15 min, Sprint Review 4 h, Retrospective 3 h.
- Artifacts and commitments: Product Backlog with Product Goal, Sprint Backlog with Sprint Goal, Increment with Definition of Done.
- Values: commitment, focus, openness, respect, courage.
- Rules: the Sprint Goal and quality hold while scope is renegotiated with the Product Owner; only the Product Owner may cancel a Sprint; work not meeting the DoD is not part of the Increment.

## Kanban (Kanban Guide v2025.5)
- Definition of Workflow (DoW): work item (unit of value), start and finish points, states between them, how WIP is controlled, explicit flow policies, service level expectation (SLE).
- Practices: define and visualise the workflow, actively manage items in it, improve it.
- Measures: WIP, throughput, work item age, cycle time.
- Little's Law (stable system): average cycle time = average WIP / average throughput. Halve WIP at the same throughput and cycle time halves.
- SLE from history, e.g. 85% of items finish within 8 days. Items whose age passes the SLE get swarmed.
- Forecast with Monte Carlo on throughput samples ("how many by date", "when will 40 be done"), quoting the 85th percentile.

## Scaling (SAFe 6.0, March 2023; check locally for a later release)
- Agile Release Train (ART) of several teams on a common cadence; PI (Planning Interval) of 8-12 weeks; PI Planning is a 2-day event producing committed PI objectives (business value 1-10) and an ART planning board of features and dependencies.
- WSJF = cost of delay / job size (a proxy for duration); cost of delay = user-business value + time criticality + risk reduction or opportunity enablement. Score each column relatively on a modified Fibonacci scale (1, 2, 3, 5, 8, 13, 20) with the smallest item = 1, and re-score every PI.

## Hybrid and PRINCE2 7 mapping
| PRINCE2 7 principle | Agile practice |
|---|---|
| Ensure continued business justification | value hypothesis per release; stop when not viable |
| Learn from experience | retrospectives, lessons log |
| Define roles, responsibilities and relationships | Product Owner as senior user's delegate; team manager role held by the team |
| Manage by stages | stage = release or set of Sprints, gate at a review |
| Manage by exception | tolerances on time and cost fixed, scope flexes |
| Focus on products | backlog items with acceptance criteria and DoD |
| Tailor to suit the project | cadence, artifacts and reports chosen per project |
Use predictive plans for fixed elements (hardware, contracts, regulatory milestones) and agile for the uncertain parts; join them with a milestone plan and rolling-wave detail.

## Output
Approach rationale, team and cadence, DoD, flow or velocity baseline, forecast with range and confidence, governance mapping, risks and dependencies.

## Pitfalls
Comparing velocity across teams or using it as a target; story points turned into hours; no WIP limits; Sprint Goals never set; hardening Sprints hiding quality debt; SAFe roles added without changing decision rights; ignoring item age.
