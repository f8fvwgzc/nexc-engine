---
id: appsec-owasp-top10-asvs
name: OWASP Top 10 and ASVS
category: appsec
description: Applies the OWASP Top 10:2025 and ASVS 5.0 to set security requirements, scope reviews, map findings and cite verifiable requirement IDs, with OWASP Cheat Sheets for implementation.
keywords: owasp top 10, owasp top 10 2025, asvs, asvs 5.0, application security verification standard, security requirements, cheat sheet series, wstg, cwe, web security, api security, verification level
source: OWASP Top 10:2025 - Introduction | https://top10.owasp.org/2025/0x00_2025-Introduction
source: OWASP ASVS - GitHub repository (5.0.0) | https://github.com/OWASP/ASVS
source: OWASP Cheat Sheet Series - Password Storage | https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html
---
## When to use
Setting security requirements, scoping a review or test, mapping findings to a recognised baseline, writing security acceptance criteria.

## Key standards (checked October 2026)
- OWASP Top 10:2025 (8th edition, first released November 2025): A01 Broken Access Control (now includes SSRF); A02 Security Misconfiguration; A03 Software Supply Chain Failures (widens 2021's vulnerable components); A04 Cryptographic Failures; A05 Injection; A06 Insecure Design; A07 Authentication Failures; A08 Software or Data Integrity Failures; A09 Security Logging and Alerting Failures (was Logging and Monitoring); A10 Mishandling of Exceptional Conditions (new). It is an awareness list of CWE groups by root cause, not a testable standard.
- OWASP ASVS 5.0.0 (May 2025): about 350 requirements in 17 chapters: V1 Encoding and Sanitization, V2 Validation and Business Logic, V3 Web Frontend Security, V4 API and Web Service, V5 File Handling, V6 Authentication, V7 Session Management, V8 Authorization, V9 Self-contained Tokens, V10 OAuth and OIDC, V11 Cryptography, V12 Secure Communication, V13 Configuration, V14 Data Protection, V15 Secure Coding and Architecture, V16 Security Logging and Error Handling, V17 WebRTC.
- Levels: L1 is about 20% of requirements, first-line defences for every app; L2 adds about 50% (about 70% in total) and is the target for most apps holding sensitive data; L3 is the remaining ~30%, mostly defence in depth, for high-value systems such as online banking. The owner picks the level from risk.
- Cite with the version: v5.0.0-chapter.section.requirement, e.g. v5.0.0-1.2.5 (OS command injection). IDs without a version mean the latest release; 4.0.3 numbers do not map 1:1.
- Many chapters open with documentation requirements (record the decision, e.g. allowed upload types) paired with implementation requirements; verify both separately.
- Companions: Cheat Sheet Series (how to build), WSTG (how to test; v4.2 is the latest stable release, v5 in development), MASVS for mobile.

## Method
1. Classify the app (data sensitivity, exposure, user base) and choose an ASVS level; tailor out chapters that do not apply (no OAuth, no WebRTC) and record why.
2. Turn applicable requirements into backlog acceptance criteria and test cases.
3. Verify by design review, code review, configuration review and testing; mark each requirement pass, fail or not applicable with evidence.
4. Report gaps by risk with the fix, the ASVS ID and the Top 10 category for management reporting.

## Cheat sheets for frequent topics
Authentication; Password Storage (Argon2id m=19 MiB, t=2, p=1; scrypt N=2^17, r=8, p=1; bcrypt cost >= 10 with a 72-byte input limit; PBKDF2-HMAC-SHA256 600,000 iterations); Session Management; Authorization; IDOR Prevention; Input Validation; Query Parameterization; XSS and DOM XSS Prevention; CSRF Prevention; SSRF Prevention; File Upload; Deserialization; Secrets Management; Logging; REST and GraphQL Security; Content Security Policy; HTTP Headers.

## Checklist
Level chosen and justified; tailoring documented; every finding carries an ASVS ID and a Top 10 category; documentation and implementation checked separately; fixes re-verified.

## Pitfalls
Claiming "Top 10 compliance"; using old 4.0.3 numbering against 5.0; treating L1 as enough for sensitive data; relying on scanners when many requirements need design or code review; missing business-logic flaws.
