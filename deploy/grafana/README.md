# Grafana dashboard

`nexc-engine.json` is a dashboard over the metrics the backend serves at `GET /metrics`
(Prometheus text; admin bearer or `NEXC_METRICS_TOKEN`; never exposed through the public edge).

Import it in Grafana (Dashboards → New → Import, paste the file) and pick a Prometheus data
source; the `instance` variable narrows the panels to one backend replica or shows them all.

| Row                        | Panels and the metrics behind them                                                                                                                                                                                                                                    |
| -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| HTTP                       | requests/s, 5xx/s and the share of GETs answered `304` (`nexc_http_requests_total{method, route, status}`); p50/p99 over every route (`nexc_http_request_duration_seconds`); the ten busiest routes. The per-route table of the last five minutes is on the console's Infrastructure page (`GET /admin/latency`). |
| Service to service (gRPC)  | p99 and calls/s by service and method of the calls the backend makes (`nexc_grpc_client_latency_seconds{service, method}`: the runtime's `Execute`, onto-intelli, the OTLP export); Agent Firewall decisions (`nexc_firewall_decisions_total`); gateway calls (`nexc_gateway_calls_total`) |
| Runs, nodes and the trace  | runs by status and their duration (`nexc_runs_total`, `nexc_run_duration_seconds`); node executions (`nexc_node_runs_total`); spans by kind and the fan-out latency (`nexc_trace_spans_total`, `nexc_trace_span_fanout_seconds`); policy mismatches, quarantined artifacts, alerts |
| LLM and caches             | tokens/s by direction (`nexc_llm_tokens_total`); the circuit breaker's events (`nexc_llm_breaker_total{provider, event}`); hit ratios of the LLM response cache, the hot-read caches (`nexc_cache_lookups_total{cache, result}`) and the node result cache; audit asks by mode |
| Alert routing and A2A      | deliveries by destination kind and outcome (`nexc_alert_deliveries_total`); A2A tasks by direction and outcome (`nexc_a2a_tasks_total`)                                                                                                                                 |

Scraping: point Prometheus at `http://backend:8080/metrics` on the internal network with
`authorization: Bearer <NEXC_METRICS_TOKEN>` (`bearer_token` in the scrape config). CI checks that
the file is valid JSON; it carries no secrets.
