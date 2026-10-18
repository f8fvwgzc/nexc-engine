# Trace

This page describes the trace spine: every action of an agent as a span on a hash chain, how spans are written and fanned out, and the views and exports built on them.

<!--@include: ../../docs/AGENTSECOPS.md#trace-->

## Related pages

| Topic                                   | Page                                                                                   |
| --------------------------------------- | -------------------------------------------------------------------------------------- |
| Spans on the wire, the re-check         | [Runtime protocol](/reference/runtime)                                                 |
| Verifying a run's chain                 | `GET /runs/{rid}/trace/verify` in the [endpoints](/reference/endpoints)                |
| Sending spans to a collector            | [OpenTelemetry export](/operate/otlp)                                                  |
| The trace explorer                      | [The console](/secops/console)                                                         |
