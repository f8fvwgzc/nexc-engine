---
id: audit-it-general-controls
name: IT general controls
category: audit
description: Audits IT general controls (access, change, operations, SDLC), reviews SOC 1 and SOC 2 reports with bridge letters and CUECs, and uses COBIT 2019 and the CISA domains as criteria.
keywords: itgc, it general controls, access management, change management, sdlc, it operations, soc 1, soc 2, bridge letter, cuec, cobit 2019, cisa, it audit, privileged access, user access review
source: ISACA - COBIT resources | https://www.isaca.org/resources/cobit
source: ISACA - CISA exam updated (2024 job practice) | https://www.isaca.org/about-us/newsroom/press-releases/2024/isacas-cisa-exam-updated-to-reflect-innovations-and-evolving-technologies-impacting-it-audit
source: AICPA-CIMA - SOC 2 | https://www.aicpa-cima.com/topic/audit-assurance/audit-and-assurance-greater-than-soc-2
source: TCSA - SOC 1 bridge letters | https://www.tcsa.in/learn/soc-1-bridge-letters
---
## When to use
ITGC testing for SOX or ICFR reliance, IT audits of governance and operations, reviewing service organisation reports, judging the effect of an IT control failure on business controls.

## ITGC domains and typical controls
- Access to programs and data: provisioning approved by the data owner; leavers removed within an agreed time; periodic user access reviews (quarterly for privileged and financial roles); privileged and admin access restricted and logged; segregation of duties rules; password, MFA and session settings; generic and service accounts owned and controlled; direct database changes logged and reviewed.
- Program change: requests authorised, tested (UAT) and approved before production; developers cannot migrate their own code; emergency changes approved after the fact; system change logs reconciled to tickets to prove completeness.
- SDLC and new systems: governance, requirements, testing, data conversion reconciliation, go-live sign-off, post-implementation review.
- Computer operations: job scheduling and failure follow-up, backup and restore tests, incident and problem management, interface monitoring.
- Cover every layer that supports the financial data: application, database, operating system, network where relevant.
- Test design: take populations from the system itself, check completeness, sample by frequency, inspect configurations directly.

## When an ITGC fails
Reliance on automated controls, calculations and system reports is in doubt for the period. Determine whether the gap was exploited (e.g. review change logs, privileged activity), look for compensating controls, test affected application controls directly, and evaluate severity with the process owners.

## SOC reports
- SOC 1 (AICPA AT-C 320, from SSAE 18): service organisation controls relevant to user entities' financial reporting. Type 1 = design at a date; Type 2 = design and operating effectiveness over a period, usually 6-12 months.
- SOC 2: Trust Services Criteria (2017, points of focus revised 2022): security plus optional availability, processing integrity, confidentiality and privacy.
- Review checklist: systems and locations in scope match the services you use; period overlaps your year; opinion type; every exception and management's response; map each complementary user entity control (CUEC) to a control you run; subservice organisations carved out (get their reports, check complementary subservice controls) or included; auditor standing.
- Bridge letter: a management statement covering the gap from report end to your year end, often up to about three months; the service auditor tests nothing in that gap, so add inquiry and your own monitoring.

## COBIT 2019 and CISA
- COBIT 2019: 40 governance and management objectives in five domains, EDM (5), APO (14), BAI (11), DSS (6), MEA (4); design factors tailor the system; process capability levels 0-5. Use as audit criteria for IT governance.
- CISA job practice (exam from 1 Aug 2024): 1 IS auditing process 18%; 2 governance and management of IT 18%; 3 IS acquisition, development and implementation 12%; 4 IS operations and business resilience 26%; 5 protection of information assets 26%.

## Output
ITGC matrix (system, layer, control, frequency, owner, test, result, dependent controls affected) and a SOC review memo with gaps, CUEC mapping and conclusion.

## Pitfalls
Testing only the application layer; trusting a ticket list as the population; unmanaged service accounts; ignoring the SOC period gap; CUECs never mapped.
