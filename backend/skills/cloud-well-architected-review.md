---
id: cloud-well-architected-review
name: Well-Architected review
category: cloud
description: Reviews one cloud workload against the AWS, Azure and Google Cloud Well-Architected frameworks and turns gaps into ranked findings with owners and an improvement plan.
keywords: well-architected, waf, architecture review, design review, pillars, reliability, security, cost optimization, operational excellence, performance efficiency, sustainability, high risk issue, aws, azure, google cloud
source: AWS Well-Architected Framework | https://docs.aws.amazon.com/wellarchitected/latest/framework/welcome.html
source: AWS Well-Architected Framework - The review process | https://docs.aws.amazon.com/wellarchitected/latest/framework/the-review-process.html
source: Microsoft Azure Well-Architected Framework pillars | https://learn.microsoft.com/en-us/azure/well-architected/pillars
source: Azure Well-Architected - Reliability design review checklist | https://learn.microsoft.com/en-us/azure/well-architected/reliability/checklist
source: Google Cloud Well-Architected Framework | https://docs.cloud.google.com/architecture/framework
---
## When to use
- Early design (before one-way-door decisions), pre-go-live gate, after a major change or incident, or a yearly health check.
- The unit is one workload: the components that together deliver one business outcome. Never a whole estate in one pass. A conversation of hours, not an audit.

## Key standards
| Framework | Pillars | Notes |
|---|---|---|
| AWS Well-Architected (latest revision 2024-11-06) | Operational excellence, Security, Reliability, Performance efficiency, Cost optimization, Sustainability | Questions OPS/SEC/REL/PERF/COST/SUS with best-practice IDs (REL09-BP04 = test restores against RTO/RPO); lenses (Serverless, SaaS, Generative AI and others) add domain questions; the WA Tool records high and medium risk issues (HRI/MRI) and milestones |
| Azure Well-Architected | Reliability, Security, Cost Optimization, Operational Excellence, Performance Efficiency | Design principles and tradeoffs per pillar; checklists coded RE:01-RE:10, SE:, CO:, OE:, PE:; Azure Well-Architected Review assessment; Advisor score split by pillar |
| Google Cloud Well-Architected (formerly Architecture Framework) | Operational excellence; Security, privacy and compliance; Reliability; Cost optimization; Performance optimization; Sustainability | Perspectives for AI/ML and financial services; core principles: design for change, document, simplify with managed services, decouple, stay stateless |

## Method
1. Scope: workload, owner, criticality tier, regions, data classification and targets (SLO, RTO/RPO, latency, budget, compliance). No targets, no risk rating.
2. Evidence: diagram, IaC, pipeline, runbooks, dashboards, recent postmortems, cost by tag, IAM and network config. Log unknowns as open questions; never assume.
3. Walk each pillar: per best practice mark implemented / partial / missing / not applicable and cite the evidence file.
4. Rate each gap by likelihood x impact against the targets:
   - High: likely material harm soon. Single point of failure on a critical flow, backups never restored, publicly readable data store, root or long-lived keys in use, no alert on user-facing symptoms, tier-1 flow with no RTO/RPO.
   - Medium: weakens posture but a compensating control exists or impact is bounded.
   - Not a finding: preferences with no target behind them.
5. Record tradeoffs (redundancy vs cost, control vs latency, commitment vs flexibility) and who accepted each risk, with a review date.
6. Improvement plan: owner, effort S/M/L, due date, framework reference; save a milestone and re-measure (HRI count per pillar is the trend).
7. Turn gaps that recur across reviews into platform fixes (guardrail, golden module, training).

## Checklist
- Ops: everything in IaC, small reversible deploys, runbooks, readiness review, learning from incidents.
- Security: federated identity, least privilege, no static keys, encryption, central logs, detective controls.
- Reliability: failure mode analysis, multi-AZ state, quotas watched, timeouts and backoff, tested restore, DR to target.
- Performance and cost: load tests, autoscaling headroom, caching; tags, budgets, rightsizing, commitments, unit cost.
- Sustainability: region choice, utilisation, managed services, data lifecycle.

## Output
- Summary: workload, date, frameworks and versions, High/Medium counts per pillar, top five actions.
- Findings table: ID, pillar, reference, observation, evidence, risk, fix, owner, effort, due. Example: F-03 | Reliability | REL09-BP04, RE:09 | snapshots exist, restore never tested | no restore log | High | quarterly automated restore, RTO measured | DB lead | M | date.
- Accepted risks with owner and review date; open questions.

## Pitfalls
Blame sessions; rating everything High; advice with no evidence; never re-measuring.
