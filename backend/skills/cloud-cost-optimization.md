---
id: cloud-cost-optimization
name: Cloud cost optimisation
category: cloud
description: Optimises cloud spend with the FinOps Framework: allocation and tagging, rightsizing and waste removal, commitment and spot strategy, unit economics and governance, with every saving quantified.
keywords: finops, cloud cost, cost optimization, tagging, cost allocation, showback, chargeback, rightsizing, savings plans, reserved instances, committed use discounts, spot, unit cost, focus, effective savings rate
source: FinOps Foundation - FinOps Framework | https://www.finops.org/framework/
source: FOCUS - FinOps Open Cost and Usage Specification | https://focus.finops.org/focus-specification/
source: AWS Well-Architected - Cost Optimization Pillar | https://docs.aws.amazon.com/wellarchitected/latest/cost-optimization-pillar/welcome.html
source: AWS Savings Plans User Guide - Savings Plans types | https://docs.aws.amazon.com/savingsplans/latest/userguide/plan-types.html
source: Google Cloud - Committed use discounts overview | https://docs.cloud.google.com/compute/docs/instances/committed-use-discounts-overview
---
## When to use
Bill review, budget overrun, cost anomaly, commitment purchase, allocation design, unit-economics reporting, or design choices with cost impact.

## Framework (FinOps Foundation, 2026 edition)
- Phases, iterated: Inform (visibility, allocation, benchmarks), Optimize (usage and rate), Operate (governance, automation, culture).
- Domains: Understand Usage & Cost; Quantify Business Value; Optimize Usage & Cost; Manage the FinOps Practice. Capabilities include Allocation, Anomaly Management, Forecasting, Budgeting, Unit Economics, Usage Optimization, Rate Optimization, Licensing & SaaS, Invoicing & Chargeback. Maturity: crawl, walk, run.
- Normalise multi-cloud billing to FOCUS (version 1.4 published): BilledCost, EffectiveCost, ListCost, ContractedCost, ChargeCategory, ServiceCategory, Tags.

## Method
1. Allocate first: mandatory tags or labels (owner, cost centre, application, environment) enforced at creation; account/subscription/project as the primary key; split shared costs proportionally, evenly or by fixed ratio and document the rule. Aim for 90%+ of spend allocated (heuristic).
2. Usage optimisation: delete idle or unattached volumes, IPs, load balancers, old snapshots; schedule non-production (12 h x 5 days = 60 of 168 hours, about 64% saved); rightsize on p95 CPU and memory over at least 14 days, including a month-end (heuristic); move to current generations; storage tiering and lifecycle rules; fix data transfer paths (NAT, cross-AZ, internet egress).
3. Rate optimisation after cleanup: commit the steady baseline (size to roughly the lowest sustained hourly eligible spend of the last 30-60 days, heuristic), leave peaks on demand or spot; ladder purchases over time instead of one big buy.
   - AWS: Compute Savings Plans up to 66% (any family, region, Fargate, Lambda); EC2 Instance Savings Plans up to 72% (one family in one region); Database Savings Plans up to 35% (Aurora, RDS, DynamoDB, ElastiCache and others); SageMaker AI Savings Plans; RIs where no plan applies. Terms cannot be changed after purchase.
   - Azure reservations and savings plan for compute. Google resource-based CUDs (up to 55%, 70% memory-optimised, cannot be cancelled) and Compute flexible CUDs (about 17-28% for 1 year, 38-63% for 3 years). Terms of 1 or 3 years.
   - Spot capacity: up to 90% off, can be reclaimed; only for stateless, checkpointed, fault-tolerant work across diversified instance pools.
4. Unit economics: unit cost = allocated cost / business driver (orders, active users, GB processed); manage the trend.
5. Operate: budgets with forecast alerts, anomaly alerts to owners, monthly engineering and finance review, policy as code against untagged or oversized resources.

## Formulas
- Coverage = eligible usage covered by commitments / total eligible usage.
- Utilisation = commitment used / commitment purchased (target 95%+, heuristic).
- Break-even utilisation for discount d: u = 1 - d (a 30% discount pays above 70% use).
- Effective savings rate = (on-demand equivalent cost - actual cost) / on-demand equivalent cost.
- Annual saving = (current monthly cost - projected monthly cost) x 12, net of effort.

## Output
Spend and trend; allocation coverage; opportunity table (action, owner, monthly saving, effort, risk, confidence); commitment proposal (coverage, term, payment, break-even); KPIs (effective savings rate, unit cost, allocation %, waste %); data period and assumptions. Prices vary by region and date: mark them verify locally.

## Pitfalls
Committing before rightsizing; 3-year terms on workloads being re-architected; ignoring egress and support fees; savings measured from list price; unenforced tag policies; cutting cost below an SLO.
