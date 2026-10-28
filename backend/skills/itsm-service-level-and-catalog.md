---
id: itsm-service-level-and-catalog
name: Service levels and catalogue
category: itsm
description: Defines and reports service levels (SLA, OLA, underpinning contracts, XLAs, SLOs), builds the service catalogue, CMDB and IT asset records, and aligns them with ISO/IEC 20000-1.
keywords: sla, ola, underpinning contract, xla, slo, service level management, service catalogue, service catalog, cmdb, configuration item, it asset management, itam, service reporting, iso 20000, availability, error budget
source: GLOCERT - ISO/IEC 20000-1 requirements explained | https://www.glocertinternational.com/resources/guides/iso-20000-1-requirements-explained/
source: ISO - ISO/IEC 20000-1:2018/Amd 1:2024 | https://www.iso.org/standard/88434.html
source: PeopleCert - ITIL certifications catalogue | https://www.peoplecert.org/browse-certifications/it-governance-and-service-management/ITIL-1
---
## Agreements
- **SLA** (provider and customer): services, service hours, targets (availability, response, restore, throughput), measurement method and data source, exclusions, responsibilities of both sides, reporting, credits, review cadence. Structures: service-based, customer-based, multi-level (corporate, customer, service).
- **OLA**: between internal teams that support an SLA. **Underpinning contract (UC)**: with an external supplier. Their targets must be tighter than the SLA: e.g. SLA restore 4 h > network team OLA 2 h, hardware UC parts on site 3 h.
- **XLA**: experience targets (user satisfaction, lost productive time, digital experience score) beside SLAs, to catch the "watermelon" report that is green outside and red inside.
- **SLI/SLO**: SLI = good events / valid events; SLO = target over a window; error budget = 1 - SLO.

## Formulas
- 30-day month = 43,200 min. 99.9% allows 43.2 min; 99.95% 21.6 min; 99.99% 4.32 min. 99.9% per year allows about 8.76 h.
- Availability % = (agreed service time - downtime) / agreed service time x 100. State whether planned maintenance counts.
- Components in series: A = A1 x A2 x ... ; n identical redundant components in parallel: A = 1 - (1 - A)^n. Two 99.9% parts in series give about 99.8%; the same pair in parallel gives 99.9999% (before failover faults). Without redundancy, end-to-end availability is lower than the weakest serial component.
- Report percentiles (p90, p95) for response and restore times, not only averages.

## Service catalogue
Part of the service portfolio (pipeline, catalogue, retired). Customer view: name, description, outcomes, owner, eligible users, hours, how to request, price or charge, SLA reference. Technical view: supporting services, CIs, OLAs, dependencies. Requestable items link to fulfilment workflows. ISO/IEC 20000-1:2018 8.2.4: catalogue(s) describe the services, their intended outcomes and the dependencies between services.

## CMDB and IT assets
- **CI**: anything that must be managed to deliver a service; record ID, class, owner, status, version, location, relationships (runs on, depends on, connects to).
- Scope by value: start with business services and their critical dependencies; do not model everything.
- Activities: planning, identification, control (changes only through change enablement), status accounting, verification and audit (reconcile discovery tools against records; set an accuracy target, e.g. 95%+ for critical CIs).
- **ITAM**: plan, acquire, deploy, operate, retire and dispose, with cost, contract, warranty and licence data (ISO/IEC 19770 family); sanitise media at disposal.
- ISO/IEC 20000-1:2018 with Amd 1:2024 (check locally for a newer edition): 8.2.5 asset management, 8.2.6 configuration management, 8.3.3 service level management, 8.3.4 supplier management, 9.4 service reporting.

## Method
1. Learn the business outcomes and vital business functions.
2. Base targets on measured history and on the cost of each extra nine.
3. Agree OLAs and UCs before signing the SLA.
4. Baseline, monitor, report monthly, review targets at least yearly.

## Service report
Performance against each target with trend, breaches with cause and action, workload, major incidents, changes, satisfaction or XLA scores, nonconformities, improvement actions. Define source data and calculation for every figure.

## Pitfalls
- Targets nobody can measure, or maintenance silently excluded.
- Averages hiding long tails.
- A CMDB nobody trusts: stale data is worse than none.
- Credits too small to change behaviour.
