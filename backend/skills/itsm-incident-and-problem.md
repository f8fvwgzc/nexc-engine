---
id: itsm-incident-and-problem
name: Incident and problem management
category: itsm
description: Runs IT incident and problem management: incident versus problem, impact x urgency priority, the major incident process, root-cause analysis, known errors and workarounds, and the metrics that prove it works.
keywords: incident management, problem management, major incident, priority matrix, impact, urgency, root cause analysis, rca, known error, workaround, kedb, 5 whys, ishikawa, mttr, service desk
source: Google SRE Book - Managing Incidents | https://sre.google/sre-book/managing-incidents/
source: Google SRE Book - Postmortem Culture | https://sre.google/sre-book/postmortem-culture/
source: GLOCERT - ISO/IEC 20000-1 requirements explained | https://www.glocertinternational.com/resources/guides/iso-20000-1-requirements-explained/
---
## Definitions
- **Incident**: an unplanned interruption to a service or a drop in its quality. Goal: restore normal service fast.
- **Service request**: a predefined user ask (access, laptop); never log it as an incident.
- **Problem**: a cause, or potential cause, of one or more incidents.
- **Known error**: a problem that has been analysed but not yet fixed.
- **Workaround**: reduces or removes impact without fixing the cause.

## Priority = impact x urgency
Impact: users or sites affected, business criticality, financial, safety or regulatory harm. Urgency: how fast the harm grows, deadlines.

| Impact \ Urgency | High | Medium | Low |
|---|---|---|---|
| High | P1 | P2 | P3 |
| Medium | P2 | P3 | P4 |
| Low | P3 | P4 | P5 |

Example targets (agree in the SLA): P1 respond 15 min / restore 4 h; P2 30 min / 8 h; P3 4 h / 3 business days; P4-P5 next business day / 5-10 business days. Define when the clock pauses (waiting on the user or a supplier) and log every pause. Upgrade priority as impact grows; downgrades need the incident owner's reason on the record.

## Incident flow
Log from every channel > categorise > prioritise > initial diagnosis (known error database, scripts) > functional or hierarchical escalation, or swarming > investigate > resolve and recover > confirm with the user > close with correct category and links to problem and change records.

## Major incident
- Declare when: P1, many customers, safety or regulatory impact, a second team is needed, or no fix after about an hour. Declare early.
- Roles: major incident manager or incident commander; technical lead (only this group changes systems); communications lead; scribe.
- One live incident document; stakeholder updates on a fixed cadence (e.g. 30 min for P1) kept apart from the technical bridge; explicit, acknowledged handover of command.
- Suspected security breach: hand to security incident response and preserve evidence.
- Post-incident review within about five business days: summary, impact (users, duration, money), UTC timeline, detection gap, root and contributing causes, what went well, actions with owners and dates.

## Problem management
1. **Identification**: incident trends, repeats, every major incident, supplier and developer notices, proactive risk review.
2. **Control**: prioritise by risk, analyse, document workaround, record known error.
3. **Error control**: find a permanent fix, raise a change, re-check workaround effectiveness, close once verified.
RCA tools: 5 Whys (branch for several causes; stop at a cause you can control); Ishikawa (people, process, technology, suppliers, environment); Kepner-Tregoe is/is-not (what, where, when, extent); fault tree (AND/OR); timeline and change correlation (many incidents follow a change); Pareto across categories. Reviews are blameless: fix systems, not people.

## Metrics and formulas
- MTRS = total downtime / number of interruptions; MTBF = uptime / number of failures; MTBSI = MTBF + MTRS.
- Availability % = (agreed service time - downtime) / agreed service time x 100.
- Also: MTTA, % resolved in SLA, first contact resolution, reopen rate, backlog age, repeat incident rate, % major incidents with a problem record, problems closed with cause removed.

## Output
Incident or problem record summary, timeline in UTC, impact, priority rationale, root and contributing causes, workaround, actions with owners and dates, linked changes.

## Pitfalls
- Closing on a workaround with no problem record.
- Priority inflation; enforce the matrix.
- Single-cause stories for multi-cause failures.
- Problem records that never raise a change.
