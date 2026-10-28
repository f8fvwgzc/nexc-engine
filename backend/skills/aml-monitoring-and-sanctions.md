---
id: aml-monitoring-and-sanctions
name: Transaction monitoring and sanctions
category: compliance
description: Detects and handles suspicious activity and sanctions exposure: typologies, monitoring scenarios and tuning, alert triage, SAR/STR filing, and screening against OFAC, UN, EU, UK and Japan MOF lists.
keywords: transaction monitoring, alert triage, sar, str, suspicious activity, typologies, red flags, structuring, sanctions screening, ofac, sdn, 50 percent rule, ofsi, uk sanctions list, false positive, trade-based money laundering
source: OFAC - FAQs on entities owned by blocked persons (50 percent rule) | https://ofac.treasury.gov/faqs/topic/1521/print
source: FinCEN and agencies - SAR FAQs, October 2025 | https://www.fincen.gov/system/files/2025-10/SAR-FAQs-October-2025.pdf
source: FCDO - The UK Sanctions List | https://gov.uk/government/publications/the-uk-sanctions-list
source: OFSI - UK financial sanctions general guidance (ownership and control) | https://www.gov.uk/government/publications/financial-sanctions-general-guidance/uk-financial-sanctions-general-guidance
source: Council of the EU - Best practices for the effective implementation of restrictive measures (ST 11623/24) | https://data.consilium.europa.eu/doc/document/ST-11623-2024-INIT/en/pdf
---
## Typologies and red flags
Structuring under cash thresholds (US CTR: over $10,000 cash per business day); pass-through flows; funnel accounts; mules; shell or front companies; round amounts; activity at odds with profile; third-party payments. Trade-based: over or under invoicing, goods that do not match the business. Crypto: mixers, chain-hopping. Terrorist financing: small transfers, misused charities. Sanctions evasion: transshipment, AIS gaps, ship-to-ship transfers, recently restructured ownership.

## Monitoring scenarios and tuning
- Scenarios: thresholds, velocity, patterns, peer-group outliers; segment by customer type; models must be explainable.
- Tune with above- and below-the-line testing; track alert-to-case and case-to-SAR rates; document rationale; validate models and data completeness; keep change control.

## Alert triage
1. Read the alert with the KYC profile and expected activity.
2. Pull transactions over a lookback (3-12 months), counterparties, open sources.
3. Test against typologies; query the business without revealing suspicion.
4. Close with a short rationale (policy and good practice; US law does not require documenting a no-file decision) or escalate to a case.
5. Case narrative: who, what, when, where, why, how; the MLRO or FIU function decides.

## SAR/STR
- US banks: file when $5,000 or more is involved and the bank knows, suspects or has reason to suspect (31 CFR 1020.320); agency rules add insider abuse of any amount and $25,000 or more even with no suspect. Deadline: 30 calendar days from initial detection of facts that may be reportable (the point the review shows suspicion, not the alert date); up to 60 if no suspect is identified.
- October 2025 FAQs: amounts at or near $10,000 alone do not require a SAR; no mandatory separate continuing-activity review; if continuing-activity SARs are used: day 30 initial, day 120 end of review period, day 150 next SAR.
- Elsewhere: STR to the national FIU (Japan: via the supervising authority to JAFIC). A SAR and its existence stay confidential: no tipping off.

## Sanctions screening
- Lists: OFAC SDN and other OFAC lists; UN Security Council Consolidated List; EU Consolidated Financial Sanctions List; UK Sanctions List (sole UK source since the OFSI list closed 28 January 2026); Japan MOF list under the Foreign Exchange and Foreign Trade Act.
- Ownership: OFAC: owned 50% or more, directly or indirectly, in aggregate by blocked persons = blocked though unlisted; control alone does not block but is a red flag; a sell-down below 50% counts only if genuine and outside US jurisdiction. EU (2024 best practices): 50% or more with aggregation, plus control tests. UK: more than 50% or control; holdings of different designated persons are not simply added unless jointly arranged or one controls the other.
- Screen customers, owners, directors, counterparties, vessels, banks and free-text payment fields; payments in real time; re-screen the book on list changes. Fuzzy matching for transliteration, aliases and word order, thresholds set by testing.
- False positives: compare date of birth, nationality, address, ID or registration number; record the disposition.
- True match: block or reject per regime; OFAC blocked or rejected transaction report within 10 business days; UK and EU: freeze and report to OFSI or the national authority; Japan: MOF permission regime (verify reporting locally); licences go to counsel.

## Output
Alert or case memo, screening disposition log, SAR draft (facts only, no conclusions of guilt), tuning report with metrics.

## Pitfalls
Screening only names; ignoring aggregation rules that differ by regime; tipping off; giving legal advice instead of naming the jurisdiction and referring to counsel.
