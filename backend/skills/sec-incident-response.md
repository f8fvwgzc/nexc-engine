---
id: sec-incident-response
name: Security incident response
category: security
description: Plans and runs cybersecurity incident response per NIST SP 800-61 Rev. 3: playbooks, triage, containment, evidence and chain of custody, notification clocks such as GDPR 72 hours and SEC 8-K Item 1.05, and lessons learned.
keywords: incident response, csirt, playbook, triage, containment, eradication, forensics, chain of custody, breach notification, gdpr article 33, 8-k item 1.05, nis2, ransomware, lessons learned, nist 800-61
source: NIST - SP 800-61 Rev. 3 Incident Response Recommendations | https://csrc.nist.gov/pubs/sp/800/61/r3/final
source: NIST - SP 800-61 Rev. 3 full text (PDF) | https://nvlpubs.nist.gov/nistpubs/SpecialPublications/NIST.SP.800-61r3.pdf
source: SEC - Cybersecurity risk management and incident disclosure rules | https://www.sec.gov/newsroom/press-releases/2023-139
source: SEC - Cybersecurity disclosure small entity compliance guide | https://www.sec.gov/resources-small-businesses/small-business-compliance-guides/cybersecurity-risk-management-strategy-governance-incident-disclosure
---
## When to use
IR plans and playbooks, a live or past security incident, notification decisions, post-incident reports.

## Framework: NIST SP 800-61 Rev. 3 (April 2025, replaces Rev. 2 of 2012)
IR is framed as a CSF 2.0 Community Profile: Govern, Identify, Protect = preparation; Detect, Respond, Recover = response; Improvement (ID.IM) runs throughout. Key outcomes: DE.AE-08 declare incidents against criteria; RS.MA-02 triage and validate; RS.MA-03 categorise and prioritise; RS.MA-04 escalate; RS.MA-05 apply recovery criteria; RS.AN-03 root cause; RS.AN-06/07 record actions, keep integrity and provenance; RS.AN-08 estimate magnitude; RS.CO-02/03 notify and share; RS.MI-01 contain; RS.MI-02 eradicate; RC.RP-03 verify backups before restore; RC.RP-05 verify restored assets; RC.RP-06 declare recovery ended.

## Method
1. **Prepare**: policy, plan, roles (incident lead, technical lead, scribe, comms, legal, privacy, executive sponsor), out-of-band contacts and channel, forensic retainer, log coverage, yearly tabletop.
2. **Triage**: validate; classify by type, functional and information impact, recoverability; set severity; assign a lead; open a record with a UTC timeline, including when the organisation became aware.
3. **Contain**: isolate hosts via EDR (do not power off), disable or reset accounts, revoke sessions and tokens, rotate exposed keys, block indicators. Decide watch versus contain with legal; scope first if acting early would tip off the attacker.
4. **Evidence**: order of volatility (RFC 3227): CPU and cache, memory and network state, temporary files, disk, remote logs, archives. Capture memory before shutdown; hash images (SHA-256); work on copies; chain-of-custody record per item: ID, description, hash, collector, time, location, every transfer signed.
5. **Eradicate**: remove persistence, close the entry point, reset credentials (AD compromise: reset KRBTGT twice, letting replication finish between resets), rebuild from known-good images.
6. **Recover**: restore from verified backups, staged return, heightened monitoring, explicit end decision.
7. **Lessons learned** within about two weeks: blameless timeline, root and contributing causes, what worked, actions with owners and dates; update playbooks, detections and risk register.

## Notification clocks (rules change: verify per jurisdiction and contract)
- GDPR Art. 33: supervisory authority without undue delay and within 72 h of awareness of a personal data breach, unless unlikely to create risk; Art. 34: data subjects without undue delay when high risk; processors tell controllers without undue delay. EU "Digital Omnibus" proposals may change thresholds and channels (verify locally).
- SEC Form 8-K Item 1.05: material incident filed within 4 business days of the materiality determination, itself made without unreasonable delay; Attorney General may delay (30 days, extendable). Annual Reg S-K Item 106 in the 10-K.
- NIS2 Art. 23: early warning 24 h, notification 72 h, final report one month.
- DORA major ICT incident (Delegated Reg. (EU) 2025/301): initial notice 4 h after classification and within 24 h of awareness, intermediate 72 h, final one month (verify locally).
- Also: sector regulators, state breach laws, contracts, cyber insurer.

## Playbook template
Trigger; severity criteria; roles; first-hour actions; containment options and who decides; evidence list; comms templates; notification decision tree; recovery and closure criteria. Core set: ransomware, business email compromise, credential theft, cloud key leak, insider, DDoS, supplier breach.

## Pitfalls
- Wiping or rebooting before imaging memory.
- Coordinating over compromised mail or chat.
- Losing the clock: awareness time never recorded.
- Paying a ransom without legal and sanctions screening.
