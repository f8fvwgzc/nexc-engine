---
id: appsec-secure-sdlc-supply-chain
name: Secure SDLC and supply chain
category: appsec
description: Builds and audits a secure development lifecycle and software supply chain: NIST SSDF practices, SLSA levels, SBOMs, SAST, DAST, SCA, secrets scanning and dependency policy with CI gates.
keywords: ssdf, nist sp 800-218, slsa, sbom, cyclonedx, spdx, sast, dast, sca, secrets scanning, dependency policy, provenance, supply chain security, devsecops, vex, cyber resilience act
source: NIST CSRC - SSDF publications | https://csrc.nist.gov/projects/ssdf/publications
source: SLSA v1.2 - Build track basics | https://slsa.dev/spec/v1.2/build-track-basics
source: SLSA v1.2 - Source track requirements | https://slsa.dev/spec/v1.2/source-requirements
source: CycloneDX - v1.7 released | https://cyclonedx.org/news/cyclonedx-v1.7-released
---
## When to use
Designing or auditing an SDLC, CI/CD security gates, open-source intake, SBOM and provenance obligations.

## Key standards (status October 2026)
- NIST SP 800-218 SSDF v1.1 (Feb 2022) is final; SP 800-218 Rev. 1 (SSDF v1.2) is still an initial public draft (Dec 2025). SP 800-218A (Jul 2024) is the generative-AI profile. Groups: PO Prepare the Organization (roles, toolchains, security criteria); PS Protect the Software (code integrity, provenance, archived releases); PW Produce Well-Secured Software (design to requirements, threat model, vetted components, PW.7 code review/analysis, PW.8 testing, secure defaults); RV Respond to Vulnerabilities (RV.1 identify, RV.2 assess and fix, RV.3 root cause).
- SLSA v1.2 Build track: L0 none; L1 provenance exists (catches mistakes, easy to forge); L2 hosted platform signs provenance (stops tampering after the build); L3 hardened platform isolates runs and keeps signing secrets away from build steps (stops tampering during the build). Source track (new in v1.2): L1 version control; L2 continuous immutable history plus source provenance; L3 enforced technical controls on named branches; L4 two trusted persons approve each change to protected branches.
- SBOM: CycloneDX 1.7 (Oct 2025, patch 1.7.2; standardised as ECMA-424 2nd edition, Dec 2025) covers SBOM, SaaSBOM, CBOM, ML-BOM and VEX; SPDX 3.0.1 (3.1 at release-candidate stage, verify locally) with profiles (Core, Software, Security, Build, AI, Dataset, Licensing, Lite); ISO/IEC 5962:2021 is SPDX 2.2.1. Baseline fields (NTIA 2021): supplier, component name, version, unique id (PURL/CPE), dependency relationship, SBOM author, timestamp. CISA's 2025 update was published as a draft (verify locally).
- EU Cyber Resilience Act: manufacturer reporting of actively exploited vulnerabilities from 11 Sep 2026; essential requirements and technical documentation (incl. SBOM) from 11 Dec 2027.

## Pipeline gates
| Stage | Controls | Example gate |
|---|---|---|
| Commit/PR | secrets scanning with push protection, SAST (Semgrep, CodeQL), IaC scan, peer review | block verified secrets and new high/critical |
| Build | SCA with reachability, licence policy, lockfiles pinned by hash, hermetic build, SBOM from the built artifact, signed provenance (in-toto, Sigstore) | block reachable CISA KEV or high-EPSS vulns, denied licences |
| Test | DAST (ZAP), API fuzzing, image scan | no high findings on new endpoints |
| Release | verify signatures and SLSA provenance, publish SBOM + VEX | unsigned = no deploy |
| Operate | continuous SBOM re-scan, PSIRT intake, disclosure policy, security.txt | fix SLA by severity |

## Dependency policy
Approved registries and mirrors; pin version and hash; quarantine brand-new releases 24-72 h against hijacked or typosquatted packages; OpenSSF Scorecard threshold for new dependencies; a named owner per dependency; remediation SLAs set by risk appetite (example: critical 7 days, high 30, medium 90); upgrade rather than fork; VEX statements for "not affected".

## Output
Control matrix mapped to SSDF task IDs; current vs target SLSA levels; tools with gate thresholds; exception register with expiry dates; metrics (mean time to remediate, % artifacts with provenance, SBOM coverage, secret-leak count).

## Pitfalls
Gates that only report; findings without triage owners; SBOMs from manifests rather than the shipped artifact; CVSS-only prioritisation; leaked secrets deleted but not rotated; SLSA claims without verified provenance.
