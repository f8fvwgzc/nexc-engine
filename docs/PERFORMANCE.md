# Performance

What the API costs per endpoint under load, how it is measured, and what the targets are. The
security paths have their own tables (`make prove-agentsecops`: identity, policy, audit, trace,
the gateway and the firewall's live risk, recorded in `CHANGELOG.md`); this is the rest of the
API.

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
5. Drives single sign-on's two navigations against the OpenID Connect provider in process
   (`backend/tests/common/oidc.rs`), each caller a person of their own who signed in once, so the
   provider's discovery document and key set are kept as a running server keeps them:
   `GET /auth/oidc/login` (the kept document read, a row of `oidc_logins` written, the redirect to
   the provider) and `GET /auth/oidc/callback`, a whole sign-in (the state taken, the code exchanged
   at the provider, the ID token's signature verified, the account found and its groups applied, a
   session started), the provider's half done beforehand and untimed for each call. The callback
   row is informational: half of its time is the provider's, here a stub that signs every token
   under one lock, so it measures the stub as much as the server. The generic `GET` sweep leaves
   both routes to this step.

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
| writes: an issue filed, a node added, a run started, single sign-on's login (a sign-in kept)           |  50 ms |
| gRPC calls, the work itself excluded (the stubs do none; the firewall's transport is the health check beside it) |   1 ms |

A missed target fails the test, so the table gates CI: the `api-latency` job runs it on every
push with 8 callers × 4 requests, one round and the targets scaled ten times
(`NEXC_PROVE_API_TARGET_SCALE`) for a shared runner, and writes the table to the job summary. The
full run is a developer's: `make prove-api` on a machine with a local PostgreSQL.

A row whose fixture the harness does not seed (an approval, a memory, an MCP server, an alert
destination, an assistant conversation) answers 404 and is shown but not gated; so are the
informational rows (the 64-caller gRPC figures, the cache-off figures, the one-team write, the
single sign-on callback, which waits on the provider).

## The table

Taken on an Apple M2 Pro (10 cores, 16 GiB, macOS 26.6.2) with the release build (thin LTO,
`cargo test --release`), 64 callers × 8 requests per endpoint, the best of two rounds, a pool of
64 connections, against PostgreSQL 18.3 running natively on the same machine (Homebrew, a C
locale, default settings). PostgreSQL in Docker Desktop, which the test suite uses, answers the
same `SELECT 1` in 0.65 ms at one caller (4.0 ms at 64 callers with 64 connections, 8.8 ms with
20) against 0.05 ms natively (1.3 ms and 1.6 ms): Docker's network puts about 0.6 ms under
every query, which is most of what a two-query read costs here, so the table is taken against
the native server. Other builds were running on the machine during the first rounds of some
rows; the best of two rounds is what the table shows.

The floor rows say what the targets are measured against; the reads marked "(hot-read cache
off)" are the same endpoints with `NEXC_CACHE_TTL_SECS=0`, the gain of the caches being the
difference with the rows above them; the one-team write shows the per-team numbering lock, the
reason issues are filed on a team per caller in the gated row.

The harness's own header: nexc-engine API latency: 64 concurrent callers × 8 requests per endpoint, the best of 2 rounds, through the router in process, a pool of 64 connections; macos aarch64, 10 cores, release build, PostgreSQL 18.3 (Homebrew) on localhost; targets at p99: reads 20 ms (whole-workspace aggregates and the status page 200 ms), writes 50 ms, gRPC 1 ms (scale ×1)

| endpoint | calls | status | p50 ms | p99 ms | max ms | target p99 | result |
| --- | ---: | --- | ---: | ---: | ---: | ---: | --- |
| PostgreSQL round trip, SELECT 1 through the pool (1 caller) | 512 | OK | 0.05 | 0.10 | 0.17 | — | info |
| PostgreSQL round trip, SELECT 1 through the pool (64 callers, 64 connections) | 512 | OK | 1.66 | 2.11 | 2.20 | — | info |
| PostgreSQL round trip, SELECT 1 through the pool (64 callers, 20 connections, the server's default pool) | 512 | OK | 1.53 | 3.67 | 3.71 | — | info |
| gRPC AgentRuntime/Execute round trip, stand-in runtime (1 caller) | 512 | OK | 0.07 | 0.08 | 0.09 | 1 ms | pass |
| gRPC AgentRuntime/Execute round trip, stand-in runtime (64 callers) | 512 | OK | 1.81 | 2.21 | 2.22 | — | info |
| gRPC IntelliService/Ask, stub onto-intelli (1 caller) | 512 | OK | 0.05 | 0.06 | 0.13 | 1 ms | pass |
| gRPC IntelliService/Ask, stub onto-intelli (64 callers) | 512 | OK | 0.89 | 0.91 | 0.91 | — | info |
| gRPC AgentFirewall/AuthorizeAction in process: the work (token lookup, decision, span; 1 caller) | 512 | OK | 0.15 | 0.32 | 0.34 | — | info |
| gRPC AgentFirewall/AuthorizeAction over the wire, whole call (1 caller) | 512 | OK | 0.21 | 0.38 | 0.39 | — | info |
| gRPC AgentFirewall/AuthorizeAction over the wire, whole call (64 callers) | 512 | OK | 5.05 | 30.05 | 43.00 | — | info |
| gRPC grpc.health.v1/Check on the firewall server: the transport, no work (1 caller) | 512 | OK | 0.03 | 0.05 | 0.06 | 1 ms | pass |
| gRPC grpc.health.v1/Check on the firewall server: the transport, no work (64 callers) | 512 | OK | 0.73 | 0.77 | 0.78 | — | info |
| GET /.well-known/agent.json | 512 | 200 | 6.19 | 7.06 | 7.23 | 20 ms | pass |
| GET /a2a/agents/{principal}/card | 512 | 200 | 6.33 | 7.08 | 7.28 | 20 ms | pass |
| GET /a2a/tasks/{tid}/artifacts/{aid} | 512 | 401 | 0.01 | 0.02 | 0.03 | 20 ms | not gated (401) |
| GET /api/v1/admin/events | 512 | 200 | 3.15 | 3.93 | 3.98 | 20 ms | pass |
| GET /api/v1/admin/infrastructure | 512 | 200 | 33.39 | 62.26 | 71.77 | 200 ms | pass |
| GET /api/v1/admin/latency | 512 | 200 | 0.04 | 0.06 | 0.07 | 20 ms | pass |
| GET /api/v1/admin/users | 512 | 200 | 3.14 | 3.80 | 3.92 | 20 ms | pass |
| GET /api/v1/admin/workspaces | 512 | 200 | 3.25 | 3.93 | 4.02 | 20 ms | pass |
| GET /api/v1/admin/workspaces/{wid} | 512 | 200 | 7.56 | 8.52 | 8.79 | 20 ms | pass |
| GET /api/v1/agent-templates | 512 | 200 | 0.05 | 0.07 | 0.09 | 20 ms | pass |
| GET /api/v1/agents | 512 | 200 | 9.36 | 10.57 | 10.84 | 20 ms | pass |
| GET /api/v1/agents/{id} | 512 | 200 | 5.67 | 6.52 | 6.69 | 20 ms | pass |
| GET /api/v1/agents/{id}/a2a/tasks | 512 | 200 | 7.16 | 7.97 | 8.20 | 20 ms | pass |
| GET /api/v1/agents/{id}/activity | 512 | 200 | 6.90 | 7.83 | 8.03 | 20 ms | pass |
| GET /api/v1/agents/{id}/tokens | 512 | 200 | 6.93 | 7.83 | 8.02 | 20 ms | pass |
| GET /api/v1/approvals/{id} | 512 | 404 | 3.15 | 3.76 | 3.82 | 20 ms | not gated (404) |
| GET /api/v1/artifacts/{aid}/download | 512 | 200 | 5.45 | 8.43 | 8.85 | 20 ms | pass |
| GET /api/v1/auth/2fa | 512 | 200 | 5.19 | 6.06 | 6.34 | 20 ms | pass |
| GET /api/v1/auth/activity | 512 | 200 | 3.20 | 3.92 | 4.06 | 20 ms | pass |
| GET /api/v1/auth/me | 512 | 200 | 3.24 | 3.79 | 3.84 | 20 ms | pass |
| GET /api/v1/auth/sessions | 512 | 200 | 3.10 | 3.96 | 4.03 | 20 ms | pass |
| GET /api/v1/graphs | 512 | 200 | 3.63 | 4.62 | 4.68 | 20 ms | pass |
| GET /api/v1/graphs/{gid} | 512 | 200 | 10.03 | 11.40 | 11.85 | 20 ms | pass |
| GET /api/v1/graphs/{gid}/analysis | 512 | 200 | 8.18 | 9.20 | 9.30 | 20 ms | pass |
| GET /api/v1/graphs/{gid}/plans/{pid} | 512 | 200 | 6.22 | 7.18 | 7.42 | 20 ms | pass |
| GET /api/v1/graphs/{gid}/runs | 512 | 200 | 7.04 | 8.09 | 8.25 | 20 ms | pass |
| GET /api/v1/graphs/{gid}/suggestions | 512 | 200 | 8.30 | 9.40 | 9.53 | 20 ms | pass |
| GET /api/v1/healthz | 512 | 200 | 0.01 | 0.02 | 0.03 | 20 ms | pass |
| GET /api/v1/issues/{iid} | 512 | 200 | 3.67 | 4.23 | 4.67 | 20 ms | pass |
| GET /api/v1/issues/{iid}/events | 512 | 200 | 5.85 | 6.90 | 7.18 | 20 ms | pass |
| GET /api/v1/memories | 512 | 200 | 5.51 | 6.06 | 6.20 | 20 ms | pass |
| GET /api/v1/memories/topics | 512 | 200 | 9.05 | 10.05 | 10.39 | 200 ms | pass |
| GET /api/v1/memories/{id} | 512 | 404 | 3.22 | 3.96 | 4.21 | 20 ms | not gated (404) |
| GET /api/v1/orchestrator/status | 512 | 200 | 7.34 | 8.31 | 8.55 | 20 ms | pass |
| GET /api/v1/readyz | 512 | 200 | 2.93 | 3.58 | 3.74 | 20 ms | pass |
| GET /api/v1/runs/{rid} | 512 | 200 | 8.41 | 9.33 | 9.56 | 20 ms | pass |
| GET /api/v1/runs/{rid}/artifacts | 512 | 200 | 5.66 | 6.43 | 6.69 | 20 ms | pass |
| GET /api/v1/runs/{rid}/artifacts.zip | 512 | 200 | 10.91 | 12.60 | 14.78 | 20 ms | pass |
| GET /api/v1/runs/{rid}/trace | 512 | 200 | 7.17 | 8.15 | 8.70 | 20 ms | pass |
| GET /api/v1/runs/{rid}/trace/verify | 512 | 200 | 6.40 | 9.04 | 10.41 | 20 ms | pass |
| GET /api/v1/settings/llm | 512 | 200 | 5.40 | 6.14 | 6.49 | 20 ms | pass |
| GET /api/v1/settings/llm/models | 512 | 200 | 5.10 | 5.98 | 6.24 | 20 ms | pass |
| GET /api/v1/templates | 512 | 200 | 0.02 | 0.03 | 0.05 | 20 ms | pass |
| GET /api/v1/workspaces | 512 | 200 | 3.09 | 3.57 | 3.71 | 20 ms | pass |
| GET /api/v1/workspaces/{wid} | 512 | 200 | 3.19 | 3.93 | 4.08 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/a2a-counterparts | 512 | 200 | 5.08 | 5.93 | 6.04 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/a2a-counterparts/{cid} | 512 | 404 | 5.48 | 6.11 | 6.23 | 20 ms | not gated (404) |
| GET /api/v1/workspaces/{wid}/alert-destinations | 512 | 200 | 5.32 | 6.27 | 6.44 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/alert-destinations/{id} | 512 | 404 | 5.13 | 5.84 | 6.10 | 20 ms | not gated (404) |
| GET /api/v1/workspaces/{wid}/alert-destinations/{id}/deliveries | 512 | 404 | 5.13 | 6.06 | 6.32 | 20 ms | not gated (404) |
| GET /api/v1/workspaces/{wid}/alerts | 512 | 200 | 5.31 | 6.15 | 6.40 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/approvals | 512 | 200 | 5.15 | 5.94 | 6.14 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/assistant/conversations | 512 | 200 | 5.07 | 5.81 | 6.13 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/assistant/conversations/{cid} | 512 | 404 | 5.20 | 6.02 | 6.11 | 20 ms | not gated (404) |
| GET /api/v1/workspaces/{wid}/audit | 512 | 200 | 6.08 | 7.07 | 7.17 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/audit/questions | 512 | 200 | 3.57 | 4.19 | 4.23 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/documents | 512 | 200 | 5.08 | 5.72 | 5.85 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/documents/{did} | 512 | 200 | 5.49 | 6.39 | 6.50 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/guardrails | 512 | 200 | 3.38 | 3.94 | 4.00 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/inbox | 512 | 200 | 5.09 | 5.98 | 6.46 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/invites | 512 | 200 | 5.08 | 5.90 | 6.15 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/issues | 512 | 200 | 6.02 | 7.57 | 7.91 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/knowledge/search | 512 | 200 | 9.04 | 10.48 | 10.90 | 200 ms | pass |
| GET /api/v1/workspaces/{wid}/knowledge/settings | 512 | 200 | 5.35 | 6.09 | 6.18 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/knowledge/topics | 512 | 200 | 5.10 | 5.86 | 6.04 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/labels | 512 | 200 | 5.19 | 6.22 | 6.51 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/llm | 512 | 200 | 3.10 | 3.52 | 3.54 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/map | 512 | 200 | 6.39 | 7.60 | 8.26 | 200 ms | pass |
| GET /api/v1/workspaces/{wid}/map/{kind} | 512 | 200 | 5.18 | 6.19 | 6.44 | 200 ms | pass |
| GET /api/v1/workspaces/{wid}/map/{kind}/{id} | 512 | 200 | 26.01 | 27.58 | 27.96 | 200 ms | pass |
| GET /api/v1/workspaces/{wid}/mcp-servers | 512 | 200 | 5.05 | 5.81 | 6.12 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/mcp-servers/{sid} | 512 | 404 | 5.18 | 5.91 | 6.03 | 20 ms | not gated (404) |
| GET /api/v1/workspaces/{wid}/members | 512 | 200 | 5.49 | 6.22 | 6.50 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/policies | 512 | 200 | 6.18 | 7.17 | 7.52 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/policies/resolve | 512 | 200 | 4.09 | 4.77 | 4.99 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/projects | 512 | 200 | 5.41 | 6.14 | 6.58 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/search | 512 | 200 | 14.99 | 16.18 | 16.72 | 200 ms | pass |
| GET /api/v1/workspaces/{wid}/secops/overview | 512 | 200 | 42.09 | 45.41 | 48.04 | 200 ms | pass |
| GET /api/v1/workspaces/{wid}/secops/spans | 512 | 400 | 0.02 | 0.03 | 0.03 | 20 ms | not gated (400) |
| GET /api/v1/workspaces/{wid}/teams | 512 | 200 | 5.54 | 6.35 | 6.56 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/teams/{tid} | 512 | 200 | 5.49 | 6.38 | 6.60 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/teams/{tid}/cycles | 512 | 200 | 7.13 | 8.25 | 8.55 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/teams/{tid}/members | 512 | 200 | 7.48 | 8.32 | 8.52 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/teams/{tid}/states | 512 | 200 | 7.87 | 8.92 | 9.06 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/timeline | 512 | 200 | 6.81 | 8.10 | 8.27 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/timeline/days | 512 | 200 | 5.31 | 6.10 | 6.36 | 20 ms | pass |
| GET /api/v1/workspaces/{wid}/timeline/summary | 512 | 200 | 5.14 | 6.05 | 6.41 | 200 ms | pass |
| GET /api/v1/workspaces/{wid}/transfers | 512 | 200 | 5.19 | 6.18 | 6.39 | 200 ms | pass |
| GET /api/v1/workspaces/{wid}/usage | 512 | 200 | 15.12 | 16.40 | 17.00 | 200 ms | pass |
| GET /metrics | 512 | 200 | 0.02 | 0.04 | 0.04 | 20 ms | pass |
| GET /.well-known/agent.json (hot-read cache off) | 512 | 200 | 8.59 | 9.57 | 9.77 | — | info |
| GET /api/v1/agents/{id} (hot-read cache off) | 512 | 200 | 14.28 | 15.54 | 15.83 | — | info |
| GET /api/v1/settings/llm (hot-read cache off) | 512 | 200 | 7.26 | 8.07 | 8.38 | — | info |
| GET /api/v1/workspaces/{wid}/guardrails (hot-read cache off) | 512 | 200 | 5.31 | 6.23 | 6.32 | — | info |
| GET /api/v1/workspaces/{wid}/llm (hot-read cache off) | 512 | 200 | 5.12 | 5.74 | 6.09 | — | info |
| GET /api/v1/workspaces/{wid}/policies/resolve (hot-read cache off) | 512 | 200 | 12.53 | 13.63 | 13.83 | — | info |
| POST /api/v1/workspaces/{wid}/teams/{tid}/issues (a team per caller) | 512 | 201 | 15.49 | 16.71 | 17.17 | 50 ms | pass |
| POST /api/v1/workspaces/{wid}/teams/{tid}/issues (64 callers on one team: the numbering lock) | 512 | 201 | 17.80 | 98.18 | 142.54 | — | info |
| POST /api/v1/graphs/{gid}/nodes | 512 | 201 | 13.53 | 15.06 | 16.29 | 50 ms | pass |
| POST /api/v1/graphs/{gid}/runs | 512 | 202 | 35.36 | 37.74 | 39.55 | 50 ms | pass |

96 rows gated, 0 missed, 9 not gated (no fixture or refused: GET /a2a/tasks/{tid}/artifacts/{aid} 401; GET /api/v1/approvals/{id} 404; GET /api/v1/memories/{id} 404; GET /api/v1/workspaces/{wid}/a2a-counterparts/{cid} 404; GET /api/v1/workspaces/{wid}/alert-destinations/{id} 404; GET /api/v1/workspaces/{wid}/alert-destinations/{id}/deliveries 404; GET /api/v1/workspaces/{wid}/assistant/conversations/{cid} 404; GET /api/v1/workspaces/{wid}/mcp-servers/{sid} 404; GET /api/v1/workspaces/{wid}/secops/spans 400)

Single sign-on's rows came later and were taken on the same machine, 64 callers × 8 requests and
the best of two rounds, but against PostgreSQL 17 in Docker Desktop with other builds running, so
every row of that run sits above the native table (eleven of the reads and writes above missed
their target in it by up to 15 ms; their native figures are the ones above). The login is a read
of the kept discovery document and one insert; the callback is informational, the in-process
provider signing every token under one lock:

| endpoint | calls | status | p50 ms | p99 ms | max ms | target p99 | result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| GET /api/v1/auth/providers | 512 | 200 | 0.01 | 0.02 | 0.04 | 20 ms | pass |
| GET /api/v1/auth/identities | 512 | 200 | 8.67 | 12.23 | 12.97 | 20 ms | pass |
| GET /api/v1/admin/users/{uid}/identities | 512 | 200 | 8.67 | 13.53 | 14.27 | 20 ms | pass |
| GET /api/v1/auth/oidc/login (to the provider) | 512 | 303 | 8.51 | 17.18 | 18.40 | 50 ms | pass |
| GET /api/v1/auth/oidc/callback (a whole sign-in, the provider in process) | 512 | 303 /app | 239.54 | 380.60 | 399.57 | — | info |
