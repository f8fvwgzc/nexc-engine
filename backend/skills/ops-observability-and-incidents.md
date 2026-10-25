---
id: ops-observability-and-incidents
name: Observability and incidents
category: devops
description: Instruments services with OpenTelemetry metrics, logs and traces, builds golden-signal, RED and USE dashboards and SLO alerts, and runs incident command and blameless postmortems.
keywords: observability, monitoring, opentelemetry, otel, metrics, logs, traces, golden signals, red method, use method, alerting, incident management, incident commander, on-call, postmortem, mttr
source: OpenTelemetry - Status | https://opentelemetry.io/status/
source: OpenTelemetry - Sampling | https://opentelemetry.io/docs/concepts/sampling/
source: Google SRE Book - Monitoring distributed systems | https://sre.google/sre-book/monitoring-distributed-systems/
source: Google SRE Workbook - Incident response | https://sre.google/workbook/incident-response/
source: Google SRE Book - Postmortem culture | https://sre.google/sre-book/postmortem-culture/
---
## When to use
Instrumenting a service, designing dashboards and alerts, running or reviewing an incident, writing a postmortem.

## Telemetry with OpenTelemetry
- Signals (October 2026): traces and metrics are stable in most language SDKs; logs are stable in C++, .NET, Java, PHP and Rust, a release candidate in Go and in development in Python and JavaScript; profiles are still in development. Check the status page before promising a signal. Send OTLP to an OpenTelemetry Collector (receivers, processors, exporters) so backends stay swappable.
- Follow semantic conventions (service.name, deployment.environment, http.*, db.*); propagate W3C Trace Context through services and queues; write trace_id into structured JSON logs.
- Sampling: head sampling (by trace ID ratio) is cheap but may drop error traces; tail sampling in the Collector keeps errors and slow traces but needs stateful, well-resourced, load-balanced collectors; combine them at high volume. Consider sampling from about 1,000 traces per second; at tens per second keep everything.
- Latency as histograms (percentiles, never averages); bound label cardinality (no user IDs or raw URLs as labels).

## What to measure
- Golden signals for user-facing systems: latency (success and failure tracked apart), traffic, errors, saturation.
- RED per service or endpoint: rate, errors, duration. USE per resource (CPU, memory, disk, NIC, pools, queues): utilisation, saturation, errors; list unmeasurable ones as known gaps.
- Page on user symptoms tied to SLO burn; causes belong on dashboards and tickets. Every page must be urgent, actionable and need human judgement; each alert links a runbook.

## Incident response
1. Declare early against agreed criteria (customer impact, fast SLO burn, security or data exposure); set severity from impact (e.g. SEV1 broad outage or data exposure, SEV2 major feature or many users degraded, SEV3 limited impact with workaround, SEV4 minor; define yours in writing).
2. Roles: Incident Commander (coordinates, delegates, decides, holds any unfilled role), Operations Lead (hands-on mitigation), Communications Lead (stakeholder and status updates on a fixed cadence, e.g. every 30 minutes for SEV1), scribe keeping the timeline. The three Cs: coordinate, communicate, control.
3. Mitigate before diagnosing: roll back, fail over, shed load, turn off a flag.
4. Hand over command explicitly; rotate people in long incidents.
5. Close when impact ends and signals are green; log detection, mitigation and resolution times (MTTD, MTTR).

## Blameless postmortem
Triggers: user impact past a threshold, any data loss, on-call intervention (rollback, rerouting), resolution time past a threshold, or a monitoring miss (humans found it). Contents: summary; impact (users, duration, budget spent); timeline; root and contributing causes; what went well, what went badly, where we were lucky; actions with owner, priority and due date. Fix systems and processes, not people; review, publish, track actions to closure.

## Output
Instrumentation plan, dashboard and alert list with thresholds and runbook links, incident timeline or postmortem in the structure above.

## Pitfalls
Dashboards nobody reads; alerts without runbooks; cardinality blow-ups; logs without correlation IDs; diagnosing before mitigating; postmortem actions never tracked.
