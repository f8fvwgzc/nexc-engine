---
id: cloud-migration-strategy
name: Cloud migration strategy
category: cloud
description: Plans a cloud migration: portfolio discovery and dependency mapping, a 7 Rs disposition per application, wave planning, cutover with rollback, and the data migration method for each workload.
keywords: cloud migration, 7 rs, rehost, replatform, refactor, lift and shift, discovery, dependency mapping, wave planning, cutover, rollback, data migration, cdc, database migration, data centre exit
source: AWS Prescriptive Guidance - About the migration strategies | https://docs.aws.amazon.com/prescriptive-guidance/latest/large-migration-guide/migration-strategies.html
source: AWS Prescriptive Guidance - Pre-cutover stage | https://docs.aws.amazon.com/prescriptive-guidance/latest/best-practices-migration-cutover/pre-cutover-stage.html
source: AWS Migration Hub User Guide - What is AWS Migration Hub | https://docs.aws.amazon.com/migrationhub/latest/ug/whatishub.html
---
## When to use
Data-centre exit, cloud-to-cloud moves, or the per-application disposition behind a migration business case.

## The 7 Rs
| R | Meaning | Choose when |
|---|---|---|
| Retire | Decommission or archive | No business value; unsupported OS with no owner; zombie apps (average CPU and memory under 5%), idle apps (5-20% over 90 days) or no inbound connections for 90 days |
| Retain | Stay on-premises for now | Residency rules, recent hardware refresh, specialised hardware, mainframe, mid-range or non-x86 Unix, waiting for vendor SaaS, dependencies not moved yet |
| Rehost | Lift and shift unchanged | Speed and volume; optimise after the move |
| Relocate | Move at hypervisor level (VMware to a cloud VMware service) or between VPCs, accounts, regions | Many VMs, no OS change, fastest exit |
| Repurchase | Replace with SaaS | Commodity function (CRM, HR, email) |
| Replatform | Small optimisations: managed database, containers, newer OS, Windows to Linux | Clear gain from limited change |
| Refactor / re-architect | Rebuild cloud-native | Strong business driver; avoid during a large migration, modernise afterwards |

## Method
1. Discover: automated inventory (AWS Transform, as AWS Migration Hub (since 2025-11-07) and Application Discovery Service no longer take new customers; Azure Migrate; Google Migration Center) running 2-4+ weeks to catch month-end jobs (heuristic); reconcile with the CMDB; record utilisation, software, licences, network flows.
2. Map dependencies: server-to-server flows, shared databases and file shares, batch chains, hard-coded IPs, latency-sensitive pairs. Tightly coupled systems form move groups that cut over together.
3. Assess: score business criticality, complexity, technical fit and compliance; assign an R and target platform; size from measured utilisation, not like-for-like; build TCO including licences and egress.
4. Plan waves: a pilot wave of low-risk apps proves tooling and the landing zone; later waves grow in complexity; respect freezes and business calendars; each wave runs build, replicate, test, cutover, hypercare. Server moves: AWS Application Migration Service (MGN), Azure Migrate, Google Migrate to Virtual Machines; VMware estates can relocate to a cloud VMware service.
5. Cutover runbook: tasks with owners (RACI), sequence, planned start and end times, go/no-go criteria, rollback trigger, point of no return, communications. Rehearse and time it against the downtime window, including rollback time and fix-forward contingency.
6. Hypercare: compare against the pre-move baseline (functional, performance, integration tests); keep the source read-only until the rollback window closes; then decommission and record lessons.

## Data migration options
- Online with change data capture (AWS DMS, Azure Database Migration Service, Google Database Migration Service, native replicas or log shipping): small downtime; cut over when lag is near zero.
- Offline bulk: backup and restore, export and import, or a transfer appliance for very large sets, followed by a delta sync.
- Transfer time T = D x 8 / (B x u): D bytes, B link speed in bit/s, u usable share (0.6-0.8). 100 TB over 1 Gbit/s at 80% = 1,000,000 s, about 11.6 days.
- Validate with row counts, checksums, sampled business reconciliations and smoke tests.

## Output
Portfolio table (app, owner, criticality, R, target, move group, wave, downtime window, data method, licence notes); wave plan; cutover runbook; TCO with assumptions; risks.

## Pitfalls
Refactoring mid-migration; dependencies found at cutover; no rehearsal; DNS TTLs not lowered days ahead (to ~300 s); licences that forbid cloud hosting; ignoring egress fees and latency to systems left on-premises; no named owner for the rollback decision.
