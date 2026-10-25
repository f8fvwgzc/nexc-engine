---
id: sec-governance-frameworks
name: Security governance frameworks
category: security
description: Builds, maps and gap-assesses a security programme against NIST CSF 2.0, ISO/IEC 27001:2022 and 27002, CIS Controls v8.1 and SOC 2, and sets up the policy hierarchy that governs it.
keywords: nist csf, csf 2.0, iso 27001, iso 27002, isms, statement of applicability, cis controls, soc 2, trust services criteria, security policy, governance, gap assessment, control mapping, security programme
source: NIST - Cybersecurity Framework | https://www.nist.gov/cyberframework
source: CIS - CIS Controls v8.1 | https://www.cisecurity.org/controls/v8-1
source: CIS - Implementation Groups | https://www.cisecurity.org/controls/implementation-groups
source: AICPA - 2017 Trust Services Criteria with revised points of focus 2022 | https://www.aicpa-cima.com/resources/download/2017-trust-services-criteria-with-revised-points-of-focus-2022
source: ISMS.online - ISO/IEC 27002:2022 control list | https://www.isms.online/iso-27002/
---
## When to use
Framework choice or mapping, gap assessment, policy work, ISO/IEC 27001 certification or SOC 2 readiness, board security reporting.

## Key standards (check locally for newer editions)
- **NIST CSF 2.0** (Feb 2024): 6 Functions, 22 Categories, 106 Subcategories. GOVERN frames the rest: GV.OC context, GV.RM risk strategy, GV.RR roles and authority, GV.PO policy, GV.OV oversight, GV.SC supply-chain risk. Then ID (AM, RA, IM), PR (AA, AT, DS, PS, IR), DE (CM, AE), RS (MA, AN, CO, MI), RC (RP, CO). Posture = Current vs Target Organizational Profile; Tiers 1-4 (Partial, Risk Informed, Repeatable, Adaptive) describe rigour, not a score to chase. Map with NIST Informative References.
- **ISO/IEC 27001:2022** + Amd 1:2024 (climate in 4.1, 4.2): certifiable ISMS, clauses 4-10. Artefacts: scope 4.3, policy 5.2, risk assessment 6.1.2, risk treatment 6.1.3 with the **Statement of Applicability** (each Annex A control: included or excluded, justification, implemented or not), objectives 6.2, internal audit 9.2, management review 9.3, corrective action 10.2. Certification: stage 1 (design, readiness) and stage 2 (operation) audits, yearly surveillance, recertification every 3 years.
- **ISO/IEC 27002:2022**: guidance for 93 controls in 4 themes: organisational 37 (5.x), people 8 (6.x), physical 14 (7.x), technological 34 (8.x). Attributes: control type, information security properties, cybersecurity concepts, operational capabilities, security domains.
- **CIS Controls v8.1**: 18 Controls, 153 Safeguards; IG1 = 56 essential-hygiene Safeguards, IG2 adds 74, IG3 adds 23. v8.1 added a Govern function and revised asset classes.
- **SOC 2** (AICPA 2017 TSC, points of focus revised 2022): Security common criteria always in scope: CC1-CC5 follow COSO 2013, CC6 logical and physical access, CC7 system operations, CC8 change management, CC9 risk mitigation. Availability, Processing Integrity, Confidentiality, Privacy optional. Type 1 = design at a date; Type 2 = design and operating effectiveness over a period (usually 3-12 months). A CPA attestation, not a certificate.

## Method
1. Scope: entities, systems, data classes, sites, suppliers; legal and contractual drivers.
2. Anchor: ISO 27001 for a certifiable ISMS, SOC 2 for North American customer assurance, CSF for board language, CIS IG1/IG2 for a prioritised technical baseline. Map once, test once, report many.
3. Gap table per control: not / partly / fully implemented, evidence, owner, risk if absent.
4. Prioritise by risk reduction per effort; roadmap at 30/90/365 days.
5. Policy hierarchy: policy (executive intent, reviewed at least yearly) > standards (mandatory, measurable) > procedures (steps) > guidelines (advice) > baselines (configurations). Each has owner, version, review date and an exception process with expiry and compensating controls.
6. Oversight: board KPIs/KRIs such as phishing-resistant MFA coverage, EDR coverage of critical assets, patch SLA attainment, overdue high risks.

## Checklist
- Scope statement and asset inventory current.
- Risk method approved before the assessment runs.
- SoA covers all 93 controls with reasons.
- Every control has an owner and dated evidence.
- Exceptions register with expiry dates.
- Internal auditors never audit their own work.

## Output
Executive summary, scope, framework choice and rationale, gap table (control id, requirement, state, evidence, gap, risk, owner, due), roadmap, policy index, metrics, assumptions to verify.

## Pitfalls
- Reading a certificate as proof of security: it shows the ISMS runs, not that every control is strong.
- Excluding Annex A controls without a risk-based reason in the SoA.
- Template policies nobody follows; tools bought but never shown to operate.
- Confusing SOC 2 Type 1 with Type 2.
