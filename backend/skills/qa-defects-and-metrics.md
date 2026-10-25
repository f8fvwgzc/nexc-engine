---
id: qa-defects-and-metrics
name: Defects and quality metrics
category: qa
description: Writes reproducible defect reports, separates severity from priority, runs triage, and computes quality metrics such as defect removal efficiency and escape rate to judge release readiness.
keywords: defect report, bug report, severity, priority, defect triage, defect lifecycle, defect removal efficiency, dre, escape rate, defect leakage, defect density, quality metrics, release readiness, go no-go, root cause analysis
source: ISTQB - CTFL v4.0.1 syllabus | https://www.istqb.org/wp-content/uploads/2024/11/ISTQB_CTFL_Syllabus_v4.0.1.pdf
source: ISTQB - CTAL Test Analyst v4.0 syllabus | https://www.istqb.org/wp-content/uploads/sdm-uploads/ISTQB-CTAL-TA-Syllabus-v4.0-EN-4.pdf
source: DORA - Software delivery performance metrics | https://dora.dev/guides/dora-metrics/
---
## When to use
Writing or reviewing defect reports, running triage, defining quality KPIs, deciding go/no-go, analysing escaped defects.

## Defect report
ISTQB CTFL v4.0.1 fields (ISO/IEC/IEEE 29119-3 calls it an incident report): unique ID; title that says what fails, where and when ("Coupon ignored at checkout when cart mixes currencies"); date, author and role; test object, build and environment (OS, browser, device, configuration, data set); context (test case, technique, phase); preconditions; minimal numbered steps; expected vs actual result (cite the requirement); reproducibility (always, 3 of 10); evidence (logs, screenshots, HAR, trace id, DB state, secrets redacted); severity; priority; status; references (requirement, test, commit). One defect per report, factual and blame-free.

## Severity vs priority
- Severity = impact of the failure: Critical (crash, data loss or corruption, security breach, no workaround); Major (key function broken, hard workaround); Minor (limited impact, easy workaround); Trivial (cosmetic).
- Priority = urgency of the fix, set by the product owner from business value, users affected and release timing.
- They diverge: a misspelt brand name on the home page is low severity but high priority; a crash in a rarely used admin export with a workaround is high severity but can be lower priority.

## Triage
Regular sessions (daily near release) with product owner, dev lead and QA: confirm it reproduces, check for duplicates, works-as-designed or change request; set severity and priority; assign owner and target release; record the risk of any deferral. Lifecycle: new, open, fixed, ready for retest, closed or reopened; also rejected, duplicate, deferred. Every fix gets confirmation testing and nearby regression.

## Metrics (define the formula and time window; never rank individuals)
- Defect removal efficiency DRE = defects found before release / (before + found after release within a fixed window, e.g. 90 days) x 100. Example: 240 before, 60 after: 80%.
- Escape rate (leakage) = post-release defects / total defects x 100 (20% above). Phase containment = share of defects found in the phase that introduced them.
- Defect density = defects / size (KLOC, function points) per component, to find hotspots.
- Mean time to resolve by severity; reopen rate; rejected-report rate (signals report quality); open defects by severity trend (should converge before release).
- Test progress: planned, executed, passed; requirement and risk coverage.
- DORA delivery metrics: throughput (change lead time, deployment frequency, failed deployment recovery time) and instability (change fail rate, deployment rework rate).
- Root cause classes (requirements, design, code, environment, data, test gap; or Orthogonal Defect Classification) feed defect prevention.

## Release readiness
Evidence-based go/no-go: exit criteria met per level; no open critical; any open high has a workaround and a signed waiver; high-risk areas tested at planned depth; regression and non-functional results within SLO; known issues in release notes; rollback rehearsed; monitoring and on-call ready. Present residual risk and confidence, not just counts. The business owner decides.

## Pitfalls
Vague titles; missing build or environment; severity inflated for attention; bug counts as productivity; closing without retest; metrics without definitions; skipping escape analysis.
