---
id: sec-risk-and-threat-modeling
name: Security risk and threat modelling
category: security
description: Assesses information security risk with NIST SP 800-30, models threats with STRIDE, attack trees and MITRE ATT&CK, and maintains a risk register with treatment decisions and owners.
keywords: risk assessment, threat modeling, threat modelling, stride, attack tree, mitre att&ck, nist 800-30, risk register, risk treatment, ale, fair, likelihood, impact, residual risk, data flow diagram
source: NIST - SP 800-30 Rev. 1 Guide for Conducting Risk Assessments | https://csrc.nist.gov/pubs/sp/800/30/r1/final
source: OWASP - Threat Modeling Cheat Sheet | https://cheatsheetseries.owasp.org/cheatsheets/Threat_Modeling_Cheat_Sheet.html
source: MITRE ATT&CK - Enterprise tactics | https://attack.mitre.org/tactics/enterprise/
source: MITRE ATT&CK - Versions | https://attack.mitre.org/resources/versions/
---
## When to use
Risk assessment of an organisation, system, supplier or change; design-time threat modelling; building or auditing a risk register; mapping detections to adversary behaviour.

## Method: NIST SP 800-30 Rev. 1 (2012, still the current edition)
1. **Prepare**: purpose, scope, assumptions, sources, risk model and scales (qualitative Very Low to Very High, or semi-quantitative 0-10 / 0-100).
2. **Conduct**: threat sources (adversarial with capability, intent, targeting; accidental; structural; environmental) > threat events > vulnerabilities and predisposing conditions > likelihood (of initiation and of adverse impact) > impact (operations, assets, individuals, other organisations, nation) > risk.
3. **Communicate** results to decision makers. 4. **Maintain**: monitor and reassess on change.
Apply at three tiers: organisation, mission/business process, system.

## Formulas
- Matrix: score = L x I on 1-5 scales (1-25); publish bands (e.g. 15-25 high). Ordinal scales: label bands, do not over-read the arithmetic.
- ALE = SLE x ARO, with SLE = AV x EF (asset value x exposure factor 0-1) and ARO = expected events per year. Net control value = ALE_before - ALE_after - annual control cost. Example: AV 1,000,000 x EF 0.3 = SLE 300,000; ARO 0.5 gives ALE 150,000.
- FAIR-style: Risk = LEF x LM; LEF = TEF x Vulnerability (probability a threat event becomes a loss). Use min / most likely / max ranges and Monte Carlo rather than point values.
- Residual risk = risk after controls; compare with stated appetite and tolerance.

## Threat modelling
Four questions: what are we working on, what can go wrong, what will we do about it, did we do a good enough job.
- Model with a DFD: external entities, processes, data stores, flows, trust boundaries. Every flow crossing a boundary gets examined.
- **STRIDE**: Spoofing (authentication), Tampering (integrity), Repudiation (non-repudiation, logging), Information disclosure (confidentiality), Denial of service (availability), Elevation of privilege (authorisation). Per element: external entity S, R; process all six; data store T, I, D (R if it holds logs); data flow T, I, D.
- **Attack trees**: root = attacker goal; AND/OR children; annotate leaves with cost, skill, likelihood; the cheapest feasible path gets fixed first.
- Alternatives: PASTA (risk-centric), LINDDUN (privacy), misuse cases (business logic).
- **MITRE ATT&CK** (Enterprise v20, current since 13 Oct 2026): 15 tactics; Defense Evasion is gone, replaced by Stealth (TA0005, the old ID) and Defense Impairment (TA0112), so recheck older mappings to TA0005. Map each scenario to technique IDs (Txxxx.yyy), then to mitigations (Mxxxx) and detection strategies; show coverage in ATT&CK Navigator; weight techniques used by groups that target the sector.

## Risk register and treatment
Fields: ID; statement "If <threat> exploits <weakness>, <impact> on <asset>"; owner; inherent L/I; existing controls; residual L/I; treatment; actions; due date; status; review date; KRI. Treatments (ISO/IEC 27005:2022 terms): modify, retain (accept), avoid, share. Acceptance is signed by an owner with authority, recorded and time-bound.

## Output
Scope and method, scales used, threat scenarios with ATT&CK IDs, DFD notes per trust boundary, risk table sorted by residual risk, treatment plan with owners, accepted risks and who accepted them, assumptions.

## Pitfalls
- Likelihood without a time frame ("likely" per year? per decade?).
- Transfer through insurance cuts financial loss, not likelihood; residual risk stays with an owner.
- One-off threat models never updated after design changes.
- Counting ATT&CK techniques "covered" without testing detections through emulation.
