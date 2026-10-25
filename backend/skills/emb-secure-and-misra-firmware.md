---
id: emb-secure-and-misra-firmware
name: Secure and MISRA-compliant firmware
category: embedded
description: Hardens embedded C code and connected devices: MISRA C and CERT C compliance with static analysis, secure boot and signed OTA, IEC 62443-4-2 component requirements, and EU Cyber Resilience Act duties and dates.
keywords: misra c, misra c:2023, misra c:2025, cert c, static analysis, secure boot, signed firmware, ota update, iec 62443-4-2, cyber resilience act, cra, sbom, vulnerability reporting, root of trust, deviation
source: MISRA - Publications | https://misra.org.uk/publications/
source: Qt Group - MISRA C:2025 changes | https://www.qt.io/quality-assurance/blog/misra-c-2025
source: SEI CERT C Coding Standard | https://cmu-sei.github.io/secure-coding-standards/sei-cert-c-coding-standard/
source: IEC Webstore - IEC 62443-4-2:2019 | https://webstore.iec.ch/en/publication/34421
source: European Commission - Cyber Resilience Act | https://digital-strategy.ec.europa.eu/en/policies/cyber-resilience-act
---
## When to use
Setting a coding standard, triaging analyzer findings, writing a compliance claim, designing secure boot and OTA, or mapping a device to IEC 62443 or the CRA.

## MISRA C
- MISRA C:2023 consolidates MISRA C:2012 with its amendments (C90, C99, C11/C18). MISRA C:2025 is the newest edition: an incremental update adding rules 8.18, 8.19, 11.11 and 19.3 and disapplying the single-exit rule (15.5). Name the contracted edition; never mix.
- Directives (not fully checkable from source) and rules; categories mandatory (no deviation), required (deviation with a record), advisory; rules are decidable or undecidable.
- MISRA Compliance:2020 claim needs: Guideline Enforcement Plan (how each guideline is checked), Re-categorization Plan (raise a category or disapply an advisory one; never lower), deviation records (rule, scope, rationale, risk, approval), and a Guideline Compliance Summary. Adopted and generated code is declared separately.

## CERT C
- SEI CERT C 2016 edition (C11): normative rules and advisory recommendations in families PRE, DCL, EXP, INT, FLP, ARR, STR, MEM, FIO, ENV, SIG, ERR, API, CON, MSC, POS, WIN.
- Priority = severity x likelihood x remediation cost (each 1-3): 12-27 = L1, 6-9 = L2, 1-4 = L3. Fix L1 first: INT30-C/INT32-C overflow, ARR30-C bounds, STR31-C overflow, MEM30-C use after free, EXP33-C uninitialized reads, FIO30-C format strings.

## Analysis pipeline
1. Compiler warnings as errors (-Wall -Wextra -Wconversion).
2. Qualified MISRA checker plus a sound analyzer for undecidable rules and runtime errors, on every merge.
3. Baseline legacy findings, block new ones; triage order: fix, then formal deviation, then justified suppression.
4. Unit tests with coverage; fuzz parsers and protocol handlers on a host build.

## Secure boot and OTA
- Root of trust: immutable first stage, key hashes in OTP/eFuse, unique device key in a secure element or TrustZone, JTAG/SWD locked in production.
- Each stage verifies the next: SHA-256 plus ECDSA P-256, Ed25519 or RSA-3072 PSS; monotonic anti-rollback counter.
- Updates: signed in CI with HSM-held keys; manifest carries version, hardware ID, digest and dependencies (IETF SUIT model, RFC 9019); TLS protects transport, the signature protects the image; A/B slots, test-then-confirm, power-fail tests; plan key rotation and revocation.

## IEC 62443-4-2 (Ed. 1.0, 2019)
Component requirements (CR, plus EDR/HDR/NDR/SAR for embedded, host, network and software components) under seven FRs: identification and authentication, use control, system integrity, data confidentiality, restricted data flow, timely response to events, resource availability. Capability levels SL-C 1-4. Pair with IEC 62443-4-1 for the secure development lifecycle.

## EU Cyber Resilience Act
Regulation (EU) 2024/2847, in force 10 Dec 2024. Notified-body rules apply from 11 Jun 2026. From 11 Sep 2026 manufacturers report actively exploited vulnerabilities and severe incidents to the coordinating CSIRT and ENISA via the Single Reporting Platform: early warning within 24 h, notification within 72 h, final report 14 days after a fix is available (vulnerabilities) or one month after notification (incidents), including products already on the market. Everything else applies from 11 Dec 2027: Annex I essential requirements, vulnerability handling with an SBOM, a support period of at least five years unless use is shorter, technical documentation, conformity assessment by product class, CE marking. Fines reach EUR 15 million or 2.5% of worldwide turnover, whichever is higher. Legal scope questions go to counsel.

## Pitfalls
Suppressions without records; claiming compliance for one edition while checking another; unsigned debug images; shared signing keys across products.
