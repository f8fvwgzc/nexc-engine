# Infrastructure

What nexc-engine runs on, what can be changed, and how.

## One database: PostgreSQL

Everything is stored in one PostgreSQL 17 database. There is no SQLite mode and none is planned:
the data layer uses PostgreSQL's full-text search (`tsvector`, GIN), `jsonb`, row claiming with
`FOR UPDATE SKIP LOCKED`, array `UNNEST` writes and, when present, the `pgvector` extension.

The connection is `NEXC_DATABASE_URL`, read when the server starts. It is deliberately not
editable from the browser: the settings that would say where the database is live in that
database, and a web form that repoints a server at another database is a way to take it over.

### Moving to another PostgreSQL (hosted, local, another container)

1. Settings -> Infrastructure -> **Check a connection**: paste the new URL. The server reports
   whether it can reach it, its version, whether pgvector is available, and whether it already
   holds nexc tables. The URL is used once; it is neither stored nor logged.
2. Copy the data: `pg_dump -Fc` from the old database, `pg_restore --no-owner` into the new one.
   Never point a different PostgreSQL image at an existing data volume (text indexes built under
   musl are not valid under glibc and the other way round).
3. Set `NEXC_DATABASE_URL` and restart. An empty database is set up on start: the server applies
   its migrations (`backend/migrations/`, versioned by timestamp, applied once each in order).

For vector search use an image with pgvector: `deploy/postgres/pgvector.Dockerfile` builds one;
`NEXC_DB_IMAGE` and `NEXC_DB_VOLUME` in `.env` select it for `make dev`. `make db-up` builds
`nexc-postgres-pgvector:<tag>` from that Dockerfile when the machine does not have it (it is on
no registry) and pulls any other image it lacks.

## Transferring one workspace to its owner's database

An organisation that wants to hold its own data does not need the whole installation moved:
Settings -> **Data transfer** (the workspace's owner only) copies one workspace to a PostgreSQL
the owner names, and then offers to remove it from this server.

1. The owner pastes `postgres://user:password@host:5432/database` of an empty database and,
   optionally, the `redis://` of the Redis that server will use. **Test connection** reaches both
   without moving anything. The strings are used for that transfer only; what is recorded is the
   target without credentials.
2. The server applies its migrations there and copies the workspace's rows table by table,
   parents before children, in one transaction on the target (`engine/transfer.rs`): the copy
   is there completely or not at all. Per-table counts are kept in `workspace_transfers`.
3. Files kept on disk travel inside the same database: uploaded originals
   (`documents/<document id>`) and run artifacts (`artifacts/<run id>/<node id>/<path>`) are
   written to `workspace_files`, one row per file, with the path they had in the data folder
   (`NEXC_DATA_DIR`). The nexc server that owns the target database writes them back out to its
   own data folder when it next starts (`transfer::restore_files`) and clears the rows' content.
   Nothing to unpack by hand.
4. With a Redis given, the handover is recorded there under `nexc:workspace:<wid>` (the
   target's location and the time), so the receiving side can see that the workspace arrived.
   nexc keeps no state in Redis otherwise — queues are rows and travel with the data.
5. After checking the copy, the owner removes the workspace here. Removal deletes every row of
   the workspace, the uploaded files of its documents and the artifact files of its runs.

Not copied, on purpose: password hashes (accounts are created on the target with a hash nothing
matches), stored API keys (sealed with this server's master key), and sessions.
In production a target that resolves to a loopback, private or link-local address is refused, so
a workspace owner cannot make the server connect into its own network.

### What this does and does not give you for GDPR or HIPAA

It gives a workspace **portability** (a complete copy in the standard schema) and **erasure from
the live database**. It is not, by itself, compliance. Outside what it reaches: this server's
database backups and logs, and whatever was sent to LLM and embedding
providers while the workspace ran here. Encryption in transit to the target is the connection
string's `sslmode`; encryption at rest is the target's. Agreements such as a BAA are between the
organisations, not something the software provides.

What the software does provide, for whoever is building a compliance case on it:

| Requirement                     | Where it is                                                                                                                                                              |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Access control                  | Workspace roles, team visibility, platform administrators walled off from content on the server                                                                          |
| Strong authentication           | Argon2 passwords, short-lived tokens, lockout after failed attempts, two-factor sign-in with recovery codes                                                              |
| Accountability                  | Per-workspace audit log, platform activity log, each account's own security activity (sign-ins with address, password and factor changes, what administrators did to it) |
| Right of access and portability | A person downloads a copy of their data; a workspace moves to its own database                                                                                           |
| Right of erasure                | Account deletion that empties the account and replaces its name in the logs; workspace deletion that removes files too                                                   |
| Session control                 | Suspension and sign-out-everywhere end live tokens, open streams and sockets at once                                                                                     |
| Backups                         | `make backup`, `make backup-check`, `make restore`                                                                                                                       |
| Encryption at rest              | Stored AI keys and two-factor secrets are sealed with `NEXC_MASTER_KEY`; everything else relies on disk or database encryption of the host                               |

What it does not provide: a log of who _read_ what (only changes are logged), encryption of the
database itself, and anything about the LLM provider's handling of what was sent to it.

## Secrets

| Secret                                | How it is kept                                                                                                             |
| ------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| Passwords                             | Argon2id hashes (`security/password.rs`); never recoverable                                                                |
| Access tokens                         | Signed JWTs, 15 minutes; refresh tokens stored as digests and rotated on use                                               |
| LLM and embedding API keys            | AES-256-GCM (`security/secret_box.rs`) with the master key, bound to their owner; only a 4-character hint is ever returned |
| Master key, JWT secret, runtime token | Environment only (`NEXC_MASTER_KEY`, …), generated by `make init`                                                          |

The master key is not changeable from the browser either. Rotating it means decrypting every
stored key with the old one and encrypting it with the new one; a server started with a different
master key cannot read the stored API keys (they must be entered again) but nothing else is lost.
Moving an installation therefore means moving the database **and** the same `.env` secrets.

## Queues and caches

No other service is needed.

- **Background work** (runs, document ingestion) is queued in PostgreSQL: workers claim rows with
  `SKIP LOCKED`, so several server instances share the work and a crashed worker's rows return to
  the queue.
- **Caches** are in process. The hot reads (agent cards, an agent's resolved policy per policy
  version, a workspace's guardrails and LLM settings) are kept per instance until they are
  written: the write drops the entry locally and tells every other replica over PostgreSQL
  `NOTIFY`, the same channel the realtime events travel on; `NEXC_CACHE_TTL_SECS` (60 s) only
  bounds a missed notification, and `0` turns the caches off. The smaller ones (a workspace's
  memory set for 30 seconds, the list of common words for 10 minutes) expire on their own. Every
  JSON read also carries an `ETag`, so a console that asks again gets a `304` with no body
  (CONTRACT §3).

Redis or an external queue would be additions, not requirements. The Infrastructure page can
check that a Redis is reachable (`redis://…`), which is as far as the server uses one today.

## What the Infrastructure page shows

Platform administrators (the `admin` role of the installation, not workspace admins) see: the
database location without credentials, its version and size, the pgvector version, applied versus
known migrations, whether the agent runtime answers, the server's embedding model, and where
queues and caches live. API: `GET /api/v1/admin/infrastructure`,
`POST /api/v1/admin/infrastructure/check`.

## gRPC channels, keepalive and the LLM circuit breaker

The backend opens one channel to each gRPC service it talks to (the agent runtime, onto-intelli,
the OTLP collector) when it starts and keeps it for the life of the process: `NEXC_GRPC_POOL_SIZE`
connections (2), each with HTTP/2 keepalive (`NEXC_GRPC_KEEPALIVE_SECS` 30, dropped after
`NEXC_GRPC_KEEPALIVE_TIMEOUT_SECS` 10 without an answer), requests and streams spread over them.
The Agent Firewall server pings its clients the same way. `docker-compose.yml` and the Helm chart
(`backend.grpc`) carry the three settings with the reason to shorten the interval behind a NAT or
a mesh. The `make prove-api` table (`docs/PERFORMANCE.md`) measures the round trips.

Calls to a network LLM provider (Anthropic, an OpenAI-compatible server) go through a circuit
breaker per endpoint: after `NEXC_LLM_BREAKER_FAILURES` (5) consecutive failures that say the
provider is in trouble (429, 5xx, no connection) the circuit opens and every call to that endpoint
fails at once with a retryable error for `NEXC_LLM_BREAKER_OPEN_SECS` (30), instead of each
waiting on its own retries and timeouts; then one probe goes out and its outcome closes the
circuit or opens it again. `nexc_llm_breaker_total` counts the events; `0` failures turns it off.

## onto-intelli (optional): the audit's query engine

"Ask the audit" needs an [onto-intelli](https://github.com/f8fvwgzc/onto-intelli) server: the
backend exports the trace to it as ontology facts (ids, names, hosts, counts and decisions,
never content) and answers audit questions from them. Set `NEXC_ONTO_URL` (its gRPC endpoint)
and `NEXC_ONTO_TOKEN` (the bearer token its `onto-intelli.toml` knows the backend by); with
`NEXC_ONTO_URL` unset the feature is off, nothing is exported and the ask routes answer 503.

- In Compose: `make demo-onto`, or `docker compose -f docker-compose.yml -f docker-compose.onto.yml
  --profile onto up -d --build`, builds it from a checkout next to this repository and wires the
  backend to it; `deploy/onto/onto-intelli.toml` is its configuration (token auth, the audit's
  schema in `deploy/onto/nexc-audit.toml`, the mock model unless `ANTHROPIC_API_KEY` is set).
- Elsewhere: run `onto serve` with a configuration like `deploy/onto/onto-intelli.toml`.
- `nexc doctor` and Settings → Infrastructure report whether it answers, its version and the
  model it explains with.

What goes out and how far it got: the tables queue every span written, run finished and agent
changed in `onto_export_queue`; the worker claims rows with `SKIP LOCKED`, streams them and
deletes them once onto-intelli acknowledged, so a span reaches it at least once and instances
share the work; `onto_export_cursors` holds each workspace's backfill (done once, the first time
the feature sees the workspace) and its last error. Moving to another onto-intelli is a restart
with a new `NEXC_ONTO_URL` and `DELETE FROM onto_export_cursors`, which backfills everything there.
