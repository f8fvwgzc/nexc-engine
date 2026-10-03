# nexc-engine backend

The backend of [nexc-engine](../README.md): one Rust binary, `nexc`, that serves the REST, SSE
and WebSocket API, plans graphs with an LLM, schedules and executes them, and keeps long-term
memory. It is an [axum](https://github.com/tokio-rs/axum) monolith on PostgreSQL 17 with a few
small C kernels for the hot numeric paths.

The API is specified in [`docs/CONTRACT.md`](../docs/CONTRACT.md) and published as OpenAPI 3.1
at `GET /api/openapi.json` (interactive reference at `/api/docs`).

```text
 browser ──REST/SSE/WS──▶ axum router ──▶ handlers ──▶ engine ──▶ executors ──▶ LLM providers
                              │                        │  │          │            (Anthropic,
                              │                        │  │          │             OpenAI-compatible,
                              ▼                        │  │          ├──▶ Python agent runtime
                         realtime hub ◀── events ──────┘  │          └──▶ texc-symphony
                   (LISTEN/NOTIFY across replicas)        ▼
                                                    PostgreSQL 17
```

## Quick start

```bash
make db-up                       # PostgreSQL 17 in docker (from the repository root)
cargo run -- init --path ../.env # writes ../.env with fresh secrets (skip if `make init` did it)
cargo run -- serve               # http://localhost:8080, docs at /api/docs
```

No API key? Set `NEXC_LLM_PROVIDER=demo` (or pick **demo** in Settings): the deterministic offline
provider plans and runs graphs without network access and labels its output `[demo output]`.

Only a Rust 1.99 toolchain and a C11 compiler are needed to build (`cargo build --release`);
there are no other system dependencies.

## Commands

| Command | What it does |
|---|---|
| `nexc serve` | API server, run dispatcher, realtime fan-out, heartbeats and housekeeping. Graceful shutdown on Ctrl-C / SIGTERM. |
| `nexc migrate` | Applies the embedded SQL migrations and exits. `serve` also migrates on start. |
| `nexc init [--path ../.env] [--force]` | Writes a `.env` with freshly generated `NEXC_JWT_SECRET`, `NEXC_MASTER_KEY`, `NEXC_RUNTIME_TOKEN`, `POSTGRES_PASSWORD` and a matching `NEXC_DATABASE_URL` (mode 0600). Refuses to overwrite without `--force`. |
| `nexc user create --email E --name N [--admin]` | Creates a user (and its default agent organisation). The password comes from `NEXC_PASSWORD` or an interactive prompt. |
| `nexc doctor` | Checks configuration, database connectivity and server version (warns below 17), pending migrations, the agent runtime, texc-symphony and the LLM key. Exit code 1 if any check fails. |
| `nexc config show` | Prints the effective configuration with secrets redacted and validates it. |
| `nexc openapi` | Prints the OpenAPI document to stdout (`npm run gen:api` in the frontend uses it). |

`.env` is read from the working directory or its parent; variables that are already set win, and
empty values count as unset.

## Configuration

| Variable | Default | Notes |
|---|---|---|
| `NEXC_ENV` | `development` | `production` forces `Secure` cookies and HSTS, refuses weak secrets and remote plain-http CORS origins, and switches logs to JSON. |
| `NEXC_HOST` / `NEXC_PORT` | `0.0.0.0` / `8080` | |
| `NEXC_DATABASE_URL` | built from `POSTGRES_*` | `postgres://user:password@host:5432/db`; add `sslmode=require` outside development. |
| `POSTGRES_USER` / `POSTGRES_PASSWORD` / `POSTGRES_DB` | `nexc` / – / `nexc` | Used when `NEXC_DATABASE_URL` is unset. |
| `NEXC_DB_MAX_CONNECTIONS` | `20` | Pool size. |
| `NEXC_DATA_DIR` | `./data` | Artifacts live in `<data>/artifacts/<run_id>/<node_id>/`. |
| `NEXC_JWT_SECRET` | **required** | ≥ 32 bytes. |
| `NEXC_MASTER_KEY` | **required** | Base64 of 32 bytes; AES-256-GCM key for stored API keys. |
| `NEXC_RUNTIME_TOKEN` | **required** | ≥ 32 characters; bearer secret shared with the agent runtime. |
| `NEXC_CORS_ORIGINS` | `http://localhost:5173` | Comma separated, explicit origins only. Also checked against the WebSocket `Origin`. |
| `NEXC_COOKIE_SECURE` | `false` | Always on in production. |
| `NEXC_ALLOW_SIGNUP` | `true` | |
| `NEXC_ADMIN_EMAIL` / `NEXC_ADMIN_PASSWORD` | – | Bootstrap admin created on start when both are set. |
| `NEXC_ACCESS_TTL_SECS` / `NEXC_REFRESH_TTL_SECS` | `900` / `1209600` | |
| `NEXC_LLM_PROVIDER` | `anthropic` | `anthropic`, `openai_compatible` or `demo`. |
| `NEXC_LLM_MODEL` | `claude-opus-5` | |
| `NEXC_LLM_BASE_URL` | provider default | Anthropic or OpenAI-compatible base URL (Ollama: `http://localhost:11434/v1`). |
| `NEXC_LLM_FALLBACKS` | `true` | Anthropic server-side refusal fallbacks (first-party API only). |
| `ANTHROPIC_API_KEY` | – | Server-wide key; a user's own key (Settings) wins. |
| `NEXC_RUNTIME_URL` | `http://localhost:8090` | Python agent runtime. |
| `NEXC_SYMPHONY_ENABLED` / `NEXC_SYMPHONY_URL` / `NEXC_SYMPHONY_WORKFLOW` | `false` / `http://localhost:4000` / `./data/symphony/WORKFLOW.md` | texc-symphony bridge. |
| `NEXC_MAX_CONCURRENCY` / `NEXC_MAX_ATTEMPTS` / `NEXC_NODE_TIMEOUT_SECS` | `4` / `3` / `600` | Scheduler defaults. |
| `NEXC_TRUST_PROXY` | `false` | Use the last `X-Forwarded-For` hop as client IP (only behind your own proxy). |
| `NEXC_METRICS_TOKEN` | – | Bearer token for `GET /metrics` (admins can always read it). |
| `NEXC_LOG` | `info` | `tracing` filter, `RUST_LOG` syntax. |

## Source layout

```text
backend/
  build.rs              compiles csrc/ with the `cc` crate
  csrc/                 C kernels (Linux kernel style, see .clang-format)
  migrations/           PostgreSQL schema (embedded with sqlx::migrate!)
  templates/            built-in starter graphs (embedded with include_str!)
  src/
    main.rs             thin entry point: parse the CLI and dispatch
    cli/                clap commands: serve, migrate, init, user, doctor, config, openapi
    config/             typed Settings from the environment, validation, Secret<T>
    app/                AppState, router + middleware stack, workers, server lifecycle
    http/               handlers per resource, extractors, problem+json, middleware, OpenAPI
    domain/             pure types and rules (no IO): users, graphs, plans, runs, agents, memory
    repo/               sqlx repositories (runtime queries, bound parameters only)
    dsa/                data structures and algorithms (below)
    kernel/             safe wrappers over the C kernels + a Rust reference implementation
    security/           argon2id, JWT, refresh sessions, AES-GCM secret box, randomness
    llm/                provider trait, Anthropic, OpenAI-compatible, demo, router, cache
    engine/             editor, dependency detection, analysis, planner, scheduler, executors
    orchestrator/       agent organisation, assignment, heartbeats, status + health cache
    memory/             extraction, consolidation and hybrid retrieval
    realtime/           per-graph hub, SSE stream, WebSocket session, LISTEN/NOTIFY fan-out
    observability/      tracing setup and Prometheus metrics
  tests/                integration tests against PostgreSQL (#[sqlx::test])
```

`unsafe` is forbidden everywhere except `src/kernel/`, where every block carries a `SAFETY`
comment and `unsafe_op_in_unsafe_fn` is denied.

## How it works

**Graphs.** Nodes are notes or tasks (`topic`, `task`, `research`, `code`, `document`, `output`);
`depends_on` edges mean "source finishes before target". Adding a `depends_on` edge that would
close a cycle is a `409` (the graph row is locked during the check). Every mutation bumps the
graph version, which also drives the strong `ETag` of `GET /graphs/{id}`.

**Dependency detection.** After each node change (debounced per graph):
`[[Title]]` wikilinks become `auto` edges from the linked node to the linking one (stale ones are
removed), and similar unconnected nodes are suggested: score `0.6·cosine + 0.4·BM25`, with
near-duplicates (MinHash Jaccard ≥ 0.8) skipped. The more general or earlier node becomes the
source. Results are pushed over the WebSocket.

**Planning.** `POST /graphs/{gid}/plan` asks the model for a strict JSON-schema proposal
(`summary`, `nodes`, `edges`). Nodes are published as `plan.node` events the moment each JSON
object is complete in the stream; the proposal is then sanitised (unique refs, real
`existing_id`s, no dangling or cycle-closing edges). Applying it upserts nodes, replaces the
graph's plan edges and lays new nodes out in topological columns, all in one transaction.

**Execution.** A run is inserted `queued`; any replica's dispatcher claims it with
`FOR UPDATE SKIP LOCKED`. The DAG is executed with a priority queue (longest remaining path
first), a semaphore for `max_concurrency`, a per-attempt timeout, exponential backoff with full
jitter for retryable errors, skipping of descendants of failed nodes and cancellation (polled
from the database, so it works across replicas). Each node's content hash (SHA-256 from the C
kernel over its text, kind, executor, model, goal and upstream outputs) lets unchanged nodes be
served from earlier results (`cached: true`, artifacts copied) unless `force` is set. Instances
heartbeat their runs; runs of a dead instance are failed after 60 s.

**Executors.** `llm` streams one completion (documents and outputs are also saved as Markdown
artifacts); `agent` calls the Python runtime's NDJSON API and stores the artifacts it returns
under a sanitised path (no `..`, no absolute paths, ≤ 20 MiB); `symphony` writes the node as an
issue into a managed `tracker.kind: memory` `WORKFLOW.md` (atomic rename), triggers a refresh and
follows the run history until the issue's run ends.

**LLM providers.** Anthropic is called over raw HTTP (`/v1/messages`, streaming, adaptive
thinking, `output_config.format` for structured output, `effort: low` for memory extraction,
server-side refusal fallbacks). `stop_reason: refusal` is a non-retryable error and
`max_tokens` marks the output truncated. 429/5xx/529 and connection errors are retried with
backoff that honours `retry-after`. Cacheable requests are served from an LRU keyed by the
SHA-256 of the request. Token usage is priced per model for `cost_usd`.

**Memory.** After a node succeeds, salient facts are extracted and consolidated against the
graph's memories: cosine ≥ 0.92 reinforces, 0.75–0.92 replaces, otherwise adds. Retrieval ranks
full-text (GIN `tsvector`) and recent candidates by `0.5·cosine + 0.3·BM25 + 0.2·recency×importance`
and feeds the top memories into node prompts and the planner.

**Agents.** Every user starts with an organisation (planner → researcher, writer, engineer,
reviewer). Nodes are assigned by `agent_role` (or by kind); spending is tracked per agent and an
agent over its token budget is flagged `over_budget` and refuses work.

**Realtime.** Events go to a per-graph tokio broadcast hub (local fast path) and to the other
replicas via PostgreSQL `NOTIFY` (payloads over 7.5 KB are parked in `realtime_outbox`). SSE
streams start with `retry: 3000`, carry increasing ids and a `heartbeat` every 15 s. Both SSE and
WebSocket authenticate with single-use, graph-bound tickets stored as SHA-256 digests.

## C kernels (`csrc/`)

| File | Function | Purpose |
|---|---|---|
| `hash.c` | `nexc_fnv1a64`, `nexc_mix64`, `nexc_hash64` | FNV-1a 64 with a splitmix64 finalizer; feature hashing, MinHash, ETags. |
| `embed.c` | `nexc_embed`, `nexc_dot`, `nexc_cosine` | 256-dimensional feature-hashing embedding of unigrams (±1) and word bigrams (±0.5), L2-normalised. |
| `minhash.c` | `nexc_minhash`, `nexc_jaccard_estimate` | 64-slot MinHash over word-bigram shingles for near-duplicate detection. |
| `sha256.c` | `nexc_sha256` | FIPS 180-4 SHA-256 for result memoization. |

The tokenizer lowercases ASCII and keeps UTF-8 words intact. All functions are pure, validate
their arguments and never allocate. `src/kernel/reference.rs` re-implements them in Rust and the
tests cross-check both (SHA-256 is also checked against the `sha2` crate).

## Data structures and algorithms (`src/dsa/`)

| Module | Structure / algorithm | Complexity |
|---|---|---|
| `graph.rs` | cycle check on edge insert (iterative DFS reachability) | O(V + E) |
| | Kahn topological sort | O(V + E) |
| | Tarjan strongly connected components (iterative, reports cycles) | O(V + E) |
| | parallel levels and critical path (longest path, DP over topological order) | O(V + E) |
| | remaining depth per node (scheduler priority) | O(V + E) |
| | weakly connected components (via union–find) | O(E·α(V)) |
| `union_find.rs` | disjoint sets with path compression and union by rank | O(α(n)) amortised |
| `lru.rs` | LRU cache with TTL: `HashMap` + index-linked list in a `Vec` arena, no `unsafe` | O(1) get / put |
| `bm25.rs` | inverted index with Okapi BM25 ranking | build O(tokens), query O(Σ postings) |
| `token_bucket.rs` | token-bucket rate limiter, lazily refilled | O(1) |
| `priority.rs` | ready queue: binary heap by critical-path length, FIFO ties | O(log n) |

## Security

* Passwords: argon2id (19 MiB, t = 2, p = 1), 12–128 characters, verified on the blocking pool;
  unknown e-mails pay for a dummy verification; identical error messages; lockout with backoff
  after 5 consecutive failures; 5 failed logins per minute per IP, then `429` with `Retry-After`.
* Tokens: HS256 JWT (15 min, `iss`/`aud`/`exp`/`nbf`/`jti` validated) in the `Authorization`
  header; refresh token only in an `HttpOnly; SameSite=Strict` cookie scoped to `/api/v1/auth`,
  rotated on every use, stored as SHA-256, with family revocation on reuse and on logout.
  Cookie endpoints require `X-Requested-With: nexc`.
* User API keys are sealed with AES-256-GCM (random 96-bit nonce, owner id as associated data);
  only the last four characters are ever shown. In production `base_url` must be public https.
* Every query on user data is scoped by owner; other users' resources are `404`.
* Strict request parsing (`deny_unknown_fields`, typed ids), 1 MiB body limit, field validation
  with RFC 7807 problem+json errors, no internal details in `500`s.
* Headers on every response: CSP (`default-src 'none'`; a narrow policy for `/api/docs`),
  `nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`, `Permissions-Policy`,
  COOP/CORP, `Cache-Control: no-store`, HSTS in production. CORS is an explicit allow-list.
* Artifact downloads are path-checked (canonicalised under the data directory) and always sent
  as `Content-Disposition: attachment` with `nosniff`.
* Rate limits per IP on auth endpoints, per user on the API, per socket on WebSocket frames.
* Secrets are wrapped in `Secret<T>` whose `Debug`/`Display` print `[redacted]`.

## Observability

Logs use `tracing` (pretty in development, JSON lines in production) with a request id
(`x-request-id`) on every response. `GET /metrics` exposes Prometheus counters and histograms:
HTTP requests by method/route/status and latency, LLM tokens in/out, LLM and node cache hits,
node runs by status, runs by status and run duration.

## Tests

Unit tests live next to the code; integration tests in `tests/` drive the real router with
`tower::ServiceExt::oneshot` (and a real socket for the WebSocket test) against PostgreSQL, using
a fake LLM provider so that nothing touches the network.

```bash
make db-up                                                    # from the repository root
export DATABASE_URL=postgres://nexc:<POSTGRES_PASSWORD>@localhost:5432/nexc
cargo test                                                    # each #[sqlx::test] gets its own database
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

`DATABASE_URL` must point at a role that may create databases (the default `nexc` role of the
container can). Unit tests alone (`cargo test --lib`) need no database.

## Notes for API consumers

* Health probes: `GET /api/v1/healthz` (liveness) and `GET /api/v1/readyz` (checks the
  database); `/healthz` and `/readyz` are aliases.
* `PATCH /graphs/{gid}/nodes/{nid}` accepts `title`, `content`, `kind`, `tags`, `x`, `y`,
  `status`, `agent_role`, `executor` and `output`; `agent_role: null` / `output: null` clear the
  field, and unknown fields are rejected.
* `node.tokens` events carry cumulative totals for the node in the current attempt.
* texc-symphony issue identifiers are `NEXC-` plus the **last** eight hex digits of the node id.
  UUID v7 ids begin with a timestamp, so nodes created together (for example by applying a plan)
  share their first eight digits; the tail keeps identifiers unique.
