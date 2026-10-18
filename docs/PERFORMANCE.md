# Performance

What the API costs per endpoint under load, how it is measured, and what the targets are. The
security paths have their own tables (`make prove-agentsecops`: identity, policy, audit, trace
and the gateway, recorded in `CHANGELOG.md`); this is the rest of the API.

## `make prove-api`

`backend/tests/api_latency.rs`, a measurement test run in release (`--ignored`, like the other
prove targets), does the following on an isolated test database:

1. Seeds a workspace with one of everything on the demo provider: a team, a project, a label, an
   issue with a comment, a graph with two nodes and an edge, a plan, a run that finished, a
   document ingested, an agent published over A2A, an external agent with an API token, a
   workspace policy.
2. Drives every documented `GET` (the streams and the SIEM exports excluded; the exports are rate
   limited to a handful per minute by design) with 64 concurrent callers for 8 requests each,
   through the real router in process: there is no socket between the caller and axum, so the
   figures are the server's own, PostgreSQL included. Then the main writes: an issue filed (a team
   per caller, since issues are numbered per team under a lock; the same callers on one team are
   shown as an informational row), a node added (a graph per caller) and a run started.
3. Measures the three gRPC services over loopback: the runtime's `AgentRuntime/Execute` stream
   against the in-process stand-in runtime (the request down, every event up to the result),
   onto-intelli's `IntelliService/Ask` against the stub server, and the Agent Firewall's
   `AuthorizeAction` in process (the work: the token looked up in PostgreSQL, the decision, the
   span) and over the wire, with `grpc.health.v1/Check` served beside it on the same server as the
   transport alone. Each with one caller in series (the per-call cost, gated) and with 64 callers
   (informational).
4. Measures the cached reads again with the hot-read caches off (`NEXC_CACHE_TTL_SECS=0`), to show
   the gain.

Every row is the best of two rounds (`NEXC_PROVE_API_ROUNDS`), so a stall of the machine during
one round, another build or a backup, does not stand for the endpoint. The first rows are the
floor under every read: PostgreSQL's own round trip (`SELECT 1` through the pool) at one caller,
at 64 callers with the harness's pool (as many connections as callers, as a deployment serving
that many concurrent requests would set `NEXC_DB_MAX_CONNECTIONS`; `NEXC_PROVE_API_DB_CONNECTIONS`
sizes it otherwise) and at 64 callers with the server's default pool of 20, which shows what the
pool's queue costs at that concurrency.

### Targets (p99)

| Rows                                                                                                   | Target |
| ------------------------------------------------------------------------------------------------------ | -----: |
| reads: every documented `GET`                                                                          |  20 ms |
| reads that aggregate a whole workspace in one answer (the SecOps overview, search, the map, usage, the day summary, transfers, knowledge search, memory topics) and the server's status page, which probes what it runs on | 200 ms |
| writes: an issue filed, a node added, a run started                                                    |  50 ms |
| gRPC calls, the work itself excluded (the stubs do none; the firewall's transport is the health check beside it) |   1 ms |

A missed target fails the test, so the table gates CI: the `api-latency` job runs it on every
push with 8 callers × 4 requests, one round and the targets scaled ten times
(`NEXC_PROVE_API_TARGET_SCALE`) for a shared runner, and writes the table to the job summary. The
full run is a developer's: `make prove-api` on a machine with a local PostgreSQL.

A row whose fixture the harness does not seed (an approval, a memory, an MCP server, an alert
destination, an assistant conversation) answers 404 and is shown but not gated; so are the
informational rows (the 64-caller gRPC figures, the cache-off figures, the one-team write).
