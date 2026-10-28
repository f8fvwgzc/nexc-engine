---
id: qa-performance-and-accessibility
name: Performance and accessibility testing
category: qa
description: Plans and analyses load, stress, soak, spike and breakpoint tests with percentile-based criteria, and tests web and mobile accessibility against WCAG 2.2 Level AA with automated and manual checks.
keywords: performance testing, load testing, stress testing, soak testing, spike testing, percentiles, p95, p99, k6, jmeter, accessibility testing, wcag 2.2, a11y, screen reader, axe, en 301 549
source: Grafana k6 - Load test types | https://grafana.com/docs/k6/latest/testing-guides/test-types/
source: W3C WAI - What's new in WCAG 2.2 | https://www.w3.org/WAI/standards-guidelines/wcag/new-in-22/
source: Deque - EN 301 549 v4.1.1 is final | https://www.deque.com/blog/en-301-549-v4-1-1-is-final-what-changed-what-it-means-and-what-you-should-do/
source: Deque - Automated testing study identifies 57 percent of issues | https://www.deque.com/blog/automated-testing-study-identifies-57-percent-of-digital-accessibility-issues
---
## When to use
Setting performance acceptance criteria, planning or reviewing load tests, and testing web or mobile UIs for WCAG 2.2 AA conformance.

## Performance test types
| Type | Load | Duration | Answers |
|---|---|---|---|
| Smoke | minimal | 1-5 min | script and system work |
| Average-load | expected production load | 5-60 min | SLOs hold at expected load |
| Stress | above normal (e.g. 1.5-2x) | 5-60 min | how it degrades |
| Spike | sudden surge | minutes | survives bursts, autoscaling, recovery |
| Soak | average | hours | leaks, drift, exhaustion |
| Breakpoint | ramp until failure | as needed | capacity ceiling |

## Performance method
1. Workload model from production data: transaction mix, arrival rate, think time, data volume. Open model (arrival rate) for public traffic; closed model (fixed users) for fixed populations.
2. Criteria from SLOs, e.g. p95 < 300 ms, p99 < 800 ms, errors < 0.1%, target throughput met, CPU < 70% at target load.
3. Production-like environment and data; monitor every tier (APM, database, queues, GC); warm up; ramp, plateau, ramp down; check the load generator is not the bottleneck.
4. Correlate latency, throughput and resources; find the knee where latency climbs; change one thing per rerun; keep a CI baseline with thresholds.
- Report p50, p90, p95, p99 and max, never the mean alone. Percentiles cannot be averaged across hosts or intervals; merge histograms. Closed-loop tools hide stalls (coordinated omission): use arrival-rate executors.
- Little's law: concurrent users = throughput x (response time + think time).
- Tools: k6, JMeter, Gatling, Locust; browser side Lighthouse and Core Web Vitals (LCP <= 2.5 s, INP <= 200 ms, CLS <= 0.1 at p75).

## Accessibility: WCAG 2.2 AA
- WCAG 2.2 became a W3C Recommendation on 5 Oct 2023; ISO/IEC 40500:2025 is identical to that version. AA = every Level A and AA criterion; 4.1.1 Parsing is obsolete.
- New at A/AA: 2.4.11 Focus Not Obscured (Minimum); 2.5.7 Dragging Movements; 2.5.8 Target Size (Minimum), 24 x 24 CSS px or enough spacing; 3.2.6 Consistent Help; 3.3.7 Redundant Entry; 3.3.8 Accessible Authentication (Minimum): no cognitive function test (recall, transcription, puzzles, calculation) unless an alternative, an assist such as paste or password manager, or object or personal-content recognition is offered.
- High-yield checks: 1.1.1 text alternatives; 1.2.2 captions; 1.3.1 semantic structure, labels, tables; 1.4.3 contrast 4.5:1 (3:1 for large text, 18 pt or 14 pt bold); 1.4.11 non-text contrast 3:1; 1.4.10 reflow at 320 CSS px; 1.4.12 text spacing; 2.1.1 keyboard and 2.1.2 no trap; 2.4.3 focus order; 2.4.7 focus visible; 3.3.1 and 3.3.3 error identification and suggestion; 4.1.2 name, role, value; 4.1.3 status messages.
- Process: automated scan (axe-core, Lighthouse, Pa11y, WAVE) first, which finds only part of the problems (Deque measured ~57% by issue volume; far fewer criteria are fully automatable); then keyboard-only pass, screen readers (NVDA or JAWS on Windows, VoiceOver, TalkBack), 200% and 400% zoom, forced colours, and sessions with disabled users.
- EU: EN 301 549 V3.2.1 references WCAG 2.1; V4.1.1 (Sep 2026, WCAG 2.2 AA) awaits OJEU citation for EAA presumption of conformity (verify locally).

## Output
Performance: workload model, criteria, per-transaction table (p50, p95, p99, errors, RPS), resource graphs, bottlenecks, actions. Accessibility: scope and page sample, per-criterion pass or fail with location, severity, fix, and a conformance summary.

## Pitfalls
One overloaded generator; no think time; caches warmed by repeated test data; averages; overlay widgets presented as fixes; ARIA misuse (no ARIA beats wrong ARIA).
