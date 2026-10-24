---
id: sec-security-architecture
name: Security architecture
category: security
description: Designs and reviews security architecture: defence in depth, segmentation, encryption at rest and in transit, key management, logging and SIEM, and hardened baselines such as CIS Benchmarks.
keywords: security architecture, defence in depth, defense in depth, segmentation, microsegmentation, encryption, tls, key management, kms, hsm, siem, logging, cis benchmarks, hardening, baseline, post-quantum
source: CIS - CIS Benchmarks | https://www.cisecurity.org/cis-benchmarks
source: NIST - SP 800-57 Part 1 Rev. 5 Recommendation for Key Management | https://csrc.nist.gov/pubs/sp/800/57/pt1/r5/final
source: OWASP - Logging Cheat Sheet | https://cheatsheetseries.owasp.org/cheatsheets/Logging_Cheat_Sheet.html
source: NIST - SP 800-53 Rev. 5 Security and Privacy Controls | https://csrc.nist.gov/pubs/sp/800/53/r5/upd1/final
---
## When to use
Designing or reviewing an enterprise, cloud or system security architecture; landing zones; segmentation; encryption and key management; logging and SIEM design; hardening standards.

## Principles
Defence in depth (layered preventive, detective and corrective controls so no single failure means compromise), least privilege, secure by default, fail secure, complete mediation, minimal attack surface, separation of duties, assume breach.

## Method
1. **Context**: crown-jewel assets, data classification, threat model, availability targets, regulatory scope.
2. **Zones**: internet edge, user, server, management, OT; default-deny between zones; micro-segment east-west traffic around crown jewels; admin plane reachable only from bastions or privileged access workstations. Cloud: separate accounts or subscriptions per environment, organisation guardrail policies, private endpoints, storage private by default.
3. **In transit**: TLS 1.2 minimum, TLS 1.3 preferred (RFC 8446), per NIST SP 800-52 Rev. 2 (under NIST review since May 2026); disable TLS 1.1 and older and weak suites; HSTS on web; mTLS between services; enable hybrid post-quantum key exchange (ML-KEM) where client and server both support it.
4. **At rest**: AES-256 (GCM for data, XTS for disks). Envelope encryption: a data key per object or volume, wrapped by a key-encryption key held in KMS or a FIPS 140-3 validated HSM. Customer-managed keys where contracts require.
5. **Key management** (SP 800-57 Pt 1 Rev. 5): generate, register, distribute, use, rotate, revoke, destroy; set a cryptoperiod per key type (e.g. symmetric data-encryption keys used to encrypt for up to about 2 years, private signing keys 1-3 years); dual control and split knowledge for root keys; never store keys beside the data they protect; keep a cryptographic inventory. Post-quantum: FIPS 203 ML-KEM, FIPS 204 ML-DSA, FIPS 205 SLH-DSA (Aug 2024); draft NIST IR 8547 (Nov 2024, not final) proposes deprecating 112-bit-strength RSA and ECC after 2030 and disallowing all of them after 2035 (verify locally).
6. **Logging and SIEM**: collect identity provider, EDR, DNS, proxy, firewall, cloud control plane, SaaS audit, database and application security events. Each event records when (UTC, NTP-synced), where, who, what and outcome. Never log passwords, tokens, keys or full card numbers. Store immutably in a separate account; log access to logs; alert when a source goes silent. Map use cases to ATT&CK and tune for precision. Retention follows law and contract (verify locally).
7. **Baselines**: CIS Benchmarks (consensus-built; Level 1 = low-impact essentials, Level 2 = defence in depth with more operational impact; STIG profiles exist), hardened images, CIS-CAT Pro to assess, drift detection, policy-as-code in IaC pipelines.
8. **Resilience**: immutable or offline backup copy, restores tested on a schedule.
Control catalogue for depth: NIST SP 800-53 Rev. 5 (release 5.2.0, Aug 2025).

## Checklist
- Trust boundaries drawn and every crossing authenticated and encrypted.
- No flat network path from user zone to crown jewels.
- Keys separated from data; rotation and revocation tested.
- Critical log sources onboarded with silence alerts.
- Baseline exceptions documented with owner and expiry.

## Output
Architecture decision record (context, decision, options, consequences), diagram notes with zones and trust boundaries, control matrix (layer, control, owner, evidence), residual risks and assumptions.

## Pitfalls
- VPN granting whole-network access.
- Keys in the same store or repository as the encrypted data.
- Logging everything and alerting on nothing.
- Applying benchmarks without testing, then silently rolling them back.
