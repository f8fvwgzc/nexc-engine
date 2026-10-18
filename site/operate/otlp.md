# OpenTelemetry export

This page sends the trace to an OpenTelemetry collector and says exactly what leaves.

<!--@include: ../../docs/CONTRACT.md#otlp-wire-->

## Setting it up

| Where               | Setting                                                                                              |
| ------------------- | ---------------------------------------------------------------------------------------------------- |
| `.env`              | `NEXC_OTLP_ENDPOINT=http://otel-collector:4317` (OTLP over gRPC). Unset keeps the export off.         |
| Helm chart          | `backend.otlpEndpoint`. See [Helm chart](/operate/helm).                                             |
| Checking it         | `nexc doctor` and the platform console's Infrastructure page report whether the collector answers, how many spans went and how far the cursor is behind. |

The run is the trace, so a tracing backend shows one trace per run with the workspace as the resource and the agent, node and principal as attributes of every span. What a span holds is on [Trace](/secops/trace).
