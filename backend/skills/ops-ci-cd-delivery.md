---
id: ops-ci-cd-delivery
name: CI/CD and delivery
category: devops
description: Designs CI/CD pipelines and delivery practice: trunk-based development, progressive delivery with canary, blue-green and feature flags, DORA metrics, and signed artifacts with build provenance.
keywords: ci/cd, continuous integration, continuous delivery, pipeline, trunk-based development, canary, blue-green, feature flags, progressive delivery, dora metrics, deployment frequency, lead time, change failure rate, artifact signing, slsa, sbom
source: DORA - Software delivery performance metrics | https://dora.dev/guides/dora-metrics/
source: Trunk Based Development | https://trunkbaseddevelopment.com/
source: SLSA v1.2 - Build track basics | https://slsa.dev/spec/v1.2/build-track-basics
source: Argo Rollouts - Canary deployment strategy | https://argo-rollouts.readthedocs.io/en/stable/features/canary/
source: Sigstore - Signing containers with Cosign | https://docs.sigstore.dev/cosign/signing/signing_with_containers/
---
## When to use
Designing or reviewing a pipeline, choosing a release strategy, measuring delivery performance, securing the software supply chain.

## Pipeline stages
1. Commit stage, under ~10 minutes (heuristic): build once, unit tests, lint, SAST, dependency and licence scan, secret scan.
2. Package: immutable artifact or image tagged with the commit SHA and deployed by digest; SBOM (SPDX or CycloneDX); provenance attestation; signature (cosign keyless through OIDC, or a KMS key).
3. Acceptance: deploy the same artifact to a production-like environment; integration, contract, performance and DAST tests; IaC plan and policy checks.
4. Release: progressive rollout with automated analysis; promote the artifact, never rebuild per environment.
5. Admission: verify signature and provenance (for keyless, pin --certificate-identity and --certificate-oidc-issuer) before anything runs.
Pipeline hardening: OIDC federation to clouds (no static keys), least-privilege ephemeral runners, third-party actions pinned by SHA, protected branches with required review, separate deploy identity per environment.

## Trunk-based development
Everyone merges to trunk at least daily; branches live hours to a day or two, for review only; trunk stays releasable; flags hide unfinished work; branch by abstraction for big changes; release branches cut late, fixes land on trunk first then cherry-pick.

## Progressive delivery
- Canary: small share first (1-5%), then 25, 50, 100%; at each step compare canary with baseline on error rate, p99 latency and saturation; abort and roll back automatically when analysis fails (Argo Rollouts AnalysisTemplate, Flagger).
- Blue-green: full parallel stack, switch at the router, roll back by switching back; needs double capacity briefly and backward-compatible schema.
- Rolling: batches via maxSurge and maxUnavailable; cheapest, slowest to undo.
- Feature flags (OpenFeature as the vendor-neutral API): separate deploy from release, cohort and percentage targeting, kill switches; retire stale flags.
- Schema changes: expand, migrate, contract across separate releases so old and new code both work.

## DORA metrics
DORA now lists five. Throughput: change lead time (commit to running in production), deployment frequency, failed deployment recovery time (replaced MTTR / time to restore). Instability: change fail rate (share of deployments needing immediate intervention such as rollback or hotfix) and deployment rework rate (share of deployments that are unplanned and caused by a production incident). Example: 40 deploys, 3 rolled back = 7.5% change fail rate. Measure per service from pipeline and incident data, watch trends, never rank individuals or teams.

## Supply chain (SLSA v1.2 build track)
L0 none; L1 provenance exists; L2 signed provenance from a hosted build platform (stops tampering after the build); L3 hardened platform isolating builds and signing keys (stops tampering during the build). Reach L2 fast, L3 for critical artifacts. v1.2 also has a Source track for the repository side (check its levels before citing them).

## Output
Pipeline diagram and gates, release strategy with rollback trigger and owner, metric baseline and target, supply-chain controls, risks.

## Pitfalls
Rebuilding per environment; manual gates with no criteria; ignored flaky tests; canaries without automated analysis; rollbacks that cannot undo schema changes; long-lived branches; flag debt.
