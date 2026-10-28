---
id: itsm-change-and-release
name: Change and release management
category: itsm
description: Runs change enablement (standard, normal, emergency changes), risk-based approval and the CAB, release and deployment management, and post-implementation reviews, with DORA delivery evidence.
keywords: change management, change enablement, cab, change advisory board, standard change, normal change, emergency change, release management, deployment, rollback, post-implementation review, pir, change risk, dora metrics, change failure rate
source: AWS - Change enablement in the cloud: What is ITIL | https://docs.aws.amazon.com/wellarchitected/latest/change-enablement-in-the-cloud/what-is-itil.html
source: DORA - Streamlining change approval | https://dora.dev/capabilities/streamlining-change-approval/
source: DORA - Software delivery performance metrics | https://dora.dev/guides/dora-metrics/
source: GLOCERT - ISO/IEC 20000-1 requirements explained | https://www.glocertinternational.com/resources/guides/iso-20000-1-requirements-explained/
---
## Definitions
- **Change**: adding, modifying or removing anything that could affect services directly or indirectly.
- **Change enablement** (ITIL 4, formerly change control) aims to maximise successful changes by assessing risk, authorising, and managing the change schedule.
- **Standard**: low risk, well understood, pre-authorised, follows a documented procedure or pipeline; logged, not individually approved.
- **Normal**: assessed and authorised by the change authority that matches its risk.
- **Emergency**: must go in as soon as possible (security fix, incident fix); expedited authority (ECAB or delegate), records finished afterwards, always reviewed.
- **Change authority**: the person or group allowed to approve a type of change; push it as close to the work as risk allows.

## Risk-based approval
Score impact if it fails (service criticality, users, data) and likelihood of failure (complexity, novelty, test coverage, history of similar changes), adjusted for tested rollback, timing (peaks, freezes) and dependencies. Example: risk = impact (1-5) x likelihood (1-5); up to 6 = low (peer review or automated gate); 8-12 = medium (change manager or technical authority); 15+ = high (CAB, business owner). Check the change schedule for collisions; respect freeze windows.

## Evidence on CABs (DORA = DevOps Research and Assessment, not the EU act)
Heavyweight approval from outside the team is linked to slower delivery, bigger batches and no lower change fail rate. Prefer peer review at commit plus automated tests and monitoring; the reviewer is not the author, which also satisfies segregation of duties. Flag high-risk changes for extra review; keep the CAB for cross-team coordination and business trade-offs. DORA's five metrics: throughput (change lead time, deployment frequency, failed deployment recovery time) and instability (change fail rate = deployments needing immediate intervention / deployments; deployment rework rate = unplanned deployments caused by production incidents / deployments).

## Release and deployment
- **Release**: a version of a service or other items made available for use. **Deployment**: moving components into an environment. Decouple them with feature flags or dark launches.
- Strategies: phased, big bang, rolling, blue/green, canary with automated rollback on SLO breach.
- Readiness checklist: tests and security scans passed, rollback tested, runbook, monitoring and alerts live, support briefed, known errors documented, early life support agreed, approvals recorded.
- ISO/IEC 20000-1:2018: 8.5.1 change management (policy defines items under control, major-impact criteria, emergency changes), 8.5.2 service design and transition, 8.5.3 release and deployment.

## Post-implementation review
Required for failed, emergency and major changes, sampled for the rest: objectives met? incidents caused? rollback used? was the risk estimate right? Feed findings into the risk model, and promote repeatedly successful normal changes to standard.

## Metrics
Change success rate = successful / total implemented; emergency changes % (keep low, e.g. under 10%); changes causing incidents; unauthorised changes found by drift detection; lead time to approve.

## Output
Change record (description, reason, CIs, risk score and rationale, test evidence, rollback plan, schedule, approver), release plan, PIR with actions.

## Pitfalls
- Every change to CAB: big batches, slow, riskier.
- Emergency route used to dodge planning.
- Rollback never rehearsed; database changes that cannot roll back.
- Unauthorised changes left uninvestigated.
