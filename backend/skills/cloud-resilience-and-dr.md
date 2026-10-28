---
id: cloud-resilience-and-dr
name: Resilience and disaster recovery
category: cloud
description: Sets RTO and RPO from business impact, picks a DR strategy from backup and restore to multi-site active/active, designs multi-AZ and multi-region failover, and plans restore tests and game days.
keywords: disaster recovery, dr, rto, rpo, resilience, high availability, multi-az, multi-region, pilot light, warm standby, active active, backup, failover, game day, chaos engineering, business continuity
source: AWS - Disaster recovery options in the cloud | https://docs.aws.amazon.com/whitepapers/latest/disaster-recovery-workloads-on-aws/disaster-recovery-options-in-the-cloud.html
source: Azure Well-Architected - Reliability design review checklist | https://learn.microsoft.com/en-us/azure/well-architected/reliability/checklist
source: AWS Well-Architected Framework | https://docs.aws.amazon.com/wellarchitected/latest/framework/welcome.html
---
## When to use
Setting recovery targets, choosing or reviewing a DR design, multi-AZ or multi-region decisions, backup policy, resilience tests and the DR plan.

## Definitions and formulas
- RTO: longest acceptable time from disruption to restored service. RPO: largest acceptable data loss, in time. Derive both per critical flow from a business impact analysis (cost per hour down, regulatory limits, maximum tolerable disruption).
- Downtime per 30 days: 99.9% = 43.2 min; 99.95% = 21.6 min; 99.99% = 4.3 min. Per year: 99.9% = 8.76 h; 99.99% = 52.6 min.
- Serial: A = A1 x A2 x ... An. Parallel redundant: A = 1 - (1 - A1)(1 - A2)... Both assume independent failures; challenge that.
- Achieved RPO = backup interval or replication lag. Achieved RTO = detect + decide + fail over + restore data + validate.

## Strategy ladder (cost rises downward; ranges indicative)
| Strategy | Recovery site | RPO / RTO |
|---|---|---|
| Backup and restore | Cross-region, cross-account backups; infrastructure redeployed from IaC | Backup interval (hours) / hours |
| Pilot light | Data replicated live; core infrastructure deployed, app tier off; cannot serve until switched on | Replication lag / tens of minutes |
| Warm standby | Scaled-down, fully working copy that serves at once; scale up on failover | Replication lag / minutes |
| Hot standby or multi-site active/active | Full capacity in 2+ regions, statically stable | Near zero / near zero; corruption still needs point-in-time backups |

## Design rules
- Multi-AZ first: stateless tiers over 2-3 zones with N+1 capacity so one zone loss stays within SLO without scaling (static stability).
- Go multi-region only when the impact analysis or a regulator demands region-level recovery.
- Fail over with data-plane actions (health-checked DNS, Amazon Application Recovery Controller routing controls, global load balancers), not control-plane calls (creating resources, editing DNS weights, traffic dials, restores) that may be impaired. Scaling up a warm standby depends on autoscaling, a control-plane call: pre-provision enough for the first wave of traffic. Prefer a human decision that triggers fully automated steps, to avoid false-alarm failovers.
- Replication is not backup: keep point-in-time, immutable (object lock, immutable vault), cross-account copies against corruption, ransomware and credential theft; follow 3-2-1.
- Recovery region ready: quotas raised, images, secrets and config replicated, IaC parameterised by region, capacity reserved for tier-1.
- Active/active writes: single writer region, multi-writer with conflict rules (last writer wins), or partitioned by key; reads stay local.
- Patterns: timeouts, retries with exponential backoff and jitter, circuit breakers, bulkheads, load shedding, graceful degradation, cells.
- Shared dependencies (IdP, DNS, secrets, CI/CD, monitoring) must survive the same failure.

## Testing and game days
- Restore every backup class quarterly and after major change (heuristic); record measured RTO and RPO.
- Game day: hypothesis, steady-state metric, blast radius, abort criteria, observers; inject faults with AWS Fault Injection Service or Azure Chaos Studio; track findings to closure.
- Full regional failover and failback rehearsal at least yearly for tier-1 (heuristic).

## Output
Impact analysis and tiers; RTO/RPO per flow; chosen strategy and its cost; failover runbook (detect, decide, fail over, validate, fail back); test plan and last results; gaps and owners.

## Pitfalls
Untested plans; no failback; dependencies only in the primary region; assuming zones fail independently; restore never timed.
