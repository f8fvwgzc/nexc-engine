# ADR 0001: HTTP/3 at the edge, caching in layers, and an index for every foreign key

- **Status:** accepted, 2026-10-24
- **Scope:** `frontend/nginx*`, `docker-compose.yml`, the Helm chart, `backend/src/ontology/cached.rs`,
  `backend/src/app/caches.rs`, `backend/src/http/middleware/etag.rs`, migrations
  `20261024000400` and `20261024000401`, `backend/tests/schema.rs`

<!-- #region body -->
## Context

The ask was three things: HTTP/3 between the browser and the services, caching that is wise
about what it keeps (including the gRPC calls the services make to each other), and a database
whose keys, indexes and triggers are configured to cost no more than they must.

What was there before this decision:

- **Transport.** nginx (the frontend container) served the console and proxied `/api` on plain
  HTTP/1.1, port 8080; TLS was left to an ingress or load balancer in front of it. Behind nginx,
  the backend is reached over pooled HTTP/1.1 keepalive connections, and the backend talks gRPC
  over HTTP/2 to the agent runtime, onto-intelli and the OTLP collector, each channel a pool of
  connections with keepalive pings (`backend/src/rpc.rs`).
- **Caching.** Already layered and sound: hashed assets cached for a year, the SPA shell always
  revalidated; every JSON read tagged with a strong `ETag` and `Cache-Control: private, no-cache`,
  answered `304` when unchanged (CONTRACT §3); the console keeping bodies per URL (persisted for
  lists) and refetching by resource freshness; in-process hot-read caches (agent cards, resolved
  policies, guardrails, LLM settings, graph vocabularies) invalidated on every replica over
  PostgreSQL `NOTIFY`. Missing: nothing kept at the edge, gzip work redone on every response,
  no cache in front of onto-intelli's gRPC reads, and a bug that silently made the public key set
  uncacheable (below).
- **Database.** 76 tables, every one with a primary key (UUIDv7). But 51 foreign keys had no index
  a lookup by the key could use, one index repeated another, and the triggers that project
  workspaces into onto-intelli fired on updates that changed nothing they project.

## Decision 1: HTTP/3 between the browser and the edge; HTTP/2 between the services

The frontend's nginx gets a second listener on **8443: TLS with HTTP/2 over TCP and HTTP/3 over
QUIC on UDP, same port**. It serves exactly the same site as 8080 (both include
`frontend/nginx/site.conf`), and every HTTPS answer carries `Alt-Svc: h3=":<port>"; ma=86400`,
from which a browser switches to HTTP/3 for the next requests, and back to HTTP/2 by itself
wherever UDP is blocked. The listener exists only when a certificate is mounted at
`/etc/nginx/tls` (`tls.crt`, `tls.key`): `docker-entrypoint.d/41-nexc-tls.sh` writes it from
`nginx/tls-server.conf.template`, logs that HTTPS is off when there is no certificate, and refuses
to start with half of one. Plain HTTP on 8080 stays for the probes, local use and deployments that
terminate TLS in front of nginx.

What HTTP/3 buys here is real: the console holds long-lived SSE streams (graph events, the
audit's answers) beside its ordinary reads. Over HTTP/1.1 every stream holds one of the six
connections a browser allows per origin; over HTTP/2 they share one connection but a lost TCP
packet stalls all of them; over HTTP/3 each stream recovers from loss on its own, a connection
is set up in one round trip, and it survives a change of network (a laptop moving from Wi-Fi).

- **Compose** publishes `${NEXC_HTTPS_PORT:-8443}` on TCP and UDP, mounts `${NEXC_TLS_DIR:-./data/tls}`
  and adds the HTTPS origin to `NEXC_CORS_ORIGINS` (the WebSocket checks its `Origin`).
  `make init` writes a certificate for localhost when there is none (`scripts/tls-cert.sh`:
  signed by mkcert's local CA when mkcert is installed, self-signed otherwise); `make tls-cert`
  writes it again.
- **Kubernetes:** ingress-nginx terminates TLS on TCP only and has no HTTP/3, so the chart's
  `frontend.http3` (off by default) gives the frontend pods the TLS Secret (mode 0440, the pod's
  group) and a `frontend-https` Service of type LoadBalancer carrying the port on TCP and UDP
  (mixed protocols, Kubernetes 1.26+), and opens 8443 in the frontend's NetworkPolicy. The host's
  DNS points at that load balancer instead of the ingress.
- **0-RTT is off** (`ssl_early_data off`): early data can be replayed by anyone who saw it, and
  the API takes writes. **HSTS** is still sent only over HTTPS, and now never for a loopback name
  (`localhost`, `127.0.0.1`, `[::1]`): a browser that trusts a local certificate would otherwise
  pin https for every port of localhost, the plain console on 8080 included.
- WebSockets (`/graphs/{id}/ws`) open over HTTP/1.1 on the same TLS port, as browsers do when
  the server offers no WebSocket over HTTP/2 or HTTP/3.

**Behind nginx, nothing moves to HTTP/3, on purpose.** The gRPC services cannot: tonic (the
backend) has no HTTP/3 transport of its own, only experimental third-party ones, grpcio (the
runtime) has none, and nginx 1.28's proxy speaks HTTP/1.x to its upstreams. Nor would they gain from it: QUIC's advantages are recovery from packet loss
without head-of-line blocking, connection migration and fewer round trips to set up, and the
internal hops are a lossless bridge or cluster network where connections are pooled and kept
alive for the life of the process, so there is no loss to recover from and no setup to save,
while QUIC costs more CPU per byte than the kernel's TCP. Revisit when tonic ships an HTTP/3
transport and a hop crosses a lossy network.

## Decision 2: cache in layers, each keeping only what it can keep correctly

| Layer | What it keeps | How it learns of a change |
| --- | --- | --- |
| Browser | hashed assets for a year; JSON bodies per URL, revalidated by `ETag` (`304`, no body) | the tag changes with the body |
| Edge (nginx) | the build, compressed once at image build time (`gzip -9`, served by `gzip_static`, file descriptors kept by `open_file_cache`); the **public documents** (the key set, agent cards) for their `max-age` | the document's `max-age`; refreshed by `ETag`, one request at a time |
| Backend, in process | hot reads (agent cards, resolved policies, guardrails, LLM settings, vocabularies) | the write drops the entry on every replica (`NOTIFY`) |
| Backend → onto-intelli (gRPC) | `FindEntities`, `Traverse`, `EventService/List`, `SchemaService/Get`, `ListProposals`, per caller and request, for `NEXC_ONTO_READ_CACHE_SECS` (10 s) | a write nexc makes to a tenant drops its reads on every replica; a change made in onto-intelli itself within the lifetime |

**The edge keeps public documents and nothing private.** `nginx/public-cache.conf` caches
`/.well-known/jwks.json` and the agent cards (`/.well-known/agent.json`,
`/a2a/agents/{principal}/card`) in a 16 MB zone on `/tmp`: one request refreshes an expired copy
by `ETag` while the others are answered from it (`proxy_cache_lock`,
`proxy_cache_background_update`), the copy answers while the backend is down
(`proxy_cache_use_stale`), a request carrying `Authorization` bypasses it, and `X-Cache-Status`
says what happened. `/api` and `/a2a` stay unbuffered and uncached: every answer there is private
to its caller, and nginx never stores what is marked `private`, `no-cache` or `no-store` anyway.

**Fixed on the way:** the key set's handler asked for `Cache-Control: public, max-age=300`, but the
conditional-reads middleware overwrote every tagged read's policy with `private, no-cache`, so no
verifier or proxy could keep it. The middleware now keeps a handler's `public` policy (and only a
public one) beside the tag; agent cards say `public, max-age=30`. CONTRACT §3 says so.

**The service-to-service cache** (`backend/src/ontology/cached.rs`) wraps the onto-intelli client
behind the same `OntologyBackend` trait, so no caller changed:

- The key is the method, the **whole principal** and every field of the request, compared
  exactly: onto-intelli filters answers by who asks (the impersonation headers), so one member's
  answer is never another's.
- **Callers that ask the same while a read is on the wire share it** (single flight): a burst of
  identical audit questions costs onto-intelli one call.
- Every write through nexc (ingest, a vocabulary written, a graph opened, a proposal filed) moves
  the tenant to a **new generation** here and, over the realtime fan-out, on every replica; a read
  that started before the write can only fill the old generation, which nothing reads again.
- Errors are never kept; `NEXC_ONTO_READ_CACHE_SECS=0` turns it off. Lookups are counted in
  `nexc_cache_lookups_total{cache="onto_read", result="hit|miss|coalesced"}`.
- The audit's event queries now ask from the start of the minute instead of the millisecond, so
  the same question within a minute is the same read; the answer is still cut at the exact time.

**Deliberately not cached:**

- *The Agent Firewall's token checks.* A revoked token or a quarantined agent must take effect on
  the next call; the firewall reads both from PostgreSQL every time (its resolved policy is
  already a hot read).
- *The runtime's `Execute` stream.* It runs agents, with side effects; a step whose inputs did not
  change is already skipped by the engine.
- *onto-intelli's `Ask`.* onto-intelli keeps its own answer and LLM caches (the `answer_cache` and
  `llm_cache` modes of an answer), and each answer reports what it cost.

## Decision 3: index every foreign key, measured rather than guessed

**Method.** The backend's whole test suite (567 tests at the time) ran against a PostgreSQL 17
started with `enable_seqscan=off` and `auto_explain` logging the plan of every statement, nested
ones included. With sequential scans priced out, a plan that still reads a table sequentially
has no index it could use. Grouped by table and predicate, that left 41 groups, a few of them the
migrations' own one-off updates; the rest of the audit read the catalogue (keys, indexes,
triggers).

**Found and changed** (`20261024000400_foreign_key_indexes.sql`):

- **51 foreign keys without an index.** Deleting the row a key points at (or a `RESTRICT` check)
  looks up the children by the key, under the delete's locks; without an index that is a read of
  the whole child table. Measured examples: deleting an agent read every issue and every agent
  (`issues.agent_id`, `agents.reports_to`), deleting a run every alert, LLM usage row and run
  command, deleting a workspace every A2A task and alert delivery, erasing an account every
  audit and platform event. Each now has an index on the key; a nullable key one `WHERE col IS
  NOT NULL`, as the earlier migrations index theirs: a lookup asks for one value, so the rows
  without one are never read and the index stays small.
- **Predicates no index served:** the export's run list asks, for every run, whether the outbox
  still holds it (`onto_export_queue (subject_id, kind)`; without it, a backlog while onto-intelli
  is down made that list quadratic); the sweep of retired onto-intelli tenants (partial, `WHERE
  cleared_at IS NULL`); and the backfills of memory and passage vectors, which read every row at
  every start to find none missing (partial indexes on the rows still without a vector, created
  with the optional pgvector column in `memory/vectors.rs` and `repo/knowledge_vectors.rs`).
- **One redundant index dropped:** `artifacts (run_id)` is the leading column of the unique
  `(run_id, node_id, path)`, so it only cost every write.

**Triggers** (`20261024000401_projection_trigger_guards.sql`). `UPDATE OF col, ...` fires whenever
a statement names one of the columns, changed or not, and most saves name them all: an issue
saved with a new description set its title again, which queued an export, re-sent the issue to
onto-intelli and dropped the tenant's read cache, for nothing. A trigger that also fires on
`INSERT` or `DELETE` cannot compare `OLD` and `NEW` in its `WHEN`, so the eight projection
triggers (graphs, agents, workspace members, teams, projects, issues, cycles, documents) are each
split into an insert/delete trigger and an update trigger guarded by `WHEN ((OLD.cols) IS DISTINCT
FROM (NEW.cols))`. The functions did not change. The other triggers were already right: the
append-only and hash-chain guards of `audit_log` and `agent_spans`, the run seal, the risk
history and the user rename (already guarded), and the run export (fires once, on finish).

**Looked at and left alone:**

- *Account erasure also matches `audit_log.subject` and `platform_events.subject` by text.* An
  index on a free-text column of the fastest-growing, append-only tables would cost every insert
  to speed up a rare administrative action; erasure reads them, once, by design.
- *The administrators' user search* (`ILIKE '%…%'` on names and e-mails) would need `pg_trgm`; the
  table is small next to everything else, and the extension is not assumed.
- *The graph list without a workspace* (`$2 IS NULL OR workspace_id = $2` beside an `EXISTS` on
  membership) is a query shape no index serves; with a workspace, which the console always sends,
  it uses one.

**Kept so by a test.** `backend/tests/schema.rs` fails when a table has no primary key, when a
foreign key has no index whose leading columns are the key (a partial index counts only as
`WHERE col IS NOT NULL`), when an index repeats another, and when a projection trigger queues an
export for an update that changed nothing projected. Without the two migrations it fails on
exactly the 51 keys, the redundant index and the triggers.

## Consequences

- An install gains 53 small indexes from the migration, and two partial ones with the optional
  vector columns, and loses one; inserts into the indexed tables do slightly more work, and
  deletes and erasures stop reading whole tables. On a large existing database each `CREATE INDEX`
  takes a short write lock on its table (`lock_timeout` 5 s, as every migration). To build them
  without one, run the migration through `psql` with `CONCURRENTLY` first (the file's header has
  the command); the migration then finds them and does nothing.
- A migration that adds a foreign key adds its index in the same file, or the schema test fails.
  Branches written before this one (the expert catalog's skills, for one) are checked the moment
  they merge.
- HTTPS on 8443 is on wherever a certificate is mounted; removing it turns the listener off.
  Browsers on a self-signed certificate ask once before they open the page; browsers may also
  keep to HTTP/2 for a certificate that is not from a public CA, so check HTTP/3 locally with a
  client that speaks it (`curl --http3 -k https://localhost:8443/`, curl built with HTTP/3).
- onto-intelli's reads may be up to 10 s behind a change made in onto-intelli itself (never
  behind one made through nexc); `NEXC_ONTO_READ_CACHE_SECS` sets it, `0` turns it off.

## How it was verified

- Backend: the full suite, 576 tests, passes against PostgreSQL 17 (pgvector image), including
  `tests/schema.rs`, the read cache's unit tests (kept per caller, dropped by a write, errors not
  kept, sixteen concurrent reads sharing one call, the fan-out on the wire) and the conditional
  reads of every documented `GET` (`tests/worst_case.rs`, now expecting the public documents'
  policies).
- Edge: the frontend image built, `nginx -t` passes, and against it: plain HTTP without `Alt-Svc`
  or HSTS; HTTPS on HTTP/2 with `Alt-Svc: h3=":<port>"`; HSTS for a real host name and none for
  loopback; assets served pre-compressed with a `Content-Length`; the key set and an agent card
  `MISS` then `HIT` with the backend asked once, `BYPASS` with an `Authorization` header, the
  private API never cached; and HTTP/3 itself (ALPN `h3`, with an aioquic client) for the SPA,
  an asset, a public document and the proxied API. Without a certificate the HTTPS listener is
  off and 8080 serves; with half of one the container refuses to start and says why.
- Helm: the chart lints and renders with `frontend.http3` off and on (CI renders both).
<!-- #endregion body -->
