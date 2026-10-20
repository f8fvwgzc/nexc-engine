# The ontology lives in onto-intelli

<!-- #region concept -->
nexc-engine keeps no ontology model of its own. [onto-intelli](https://github.com/f8fvwgzc/onto-intelli)
is the one place where entity kinds, link types and actions are defined, configured and reasoned over,
and nexc uses it for four things:

| What                       | Where it lives in onto-intelli                                                                 | How nexc uses it                                                                                                                                                         |
| -------------------------- | ---------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Graph vocabularies         | `node_type` and `relation_type` entities of the `builtin:nexc-execution` package (`ExecutionService`) | The scheduler, the planner, the editor and the canvas read every graph's node types and relation types from there, through a short-lived cache, and write them back there. |
| The workspace map          | Teams, people, projects, cycles, issues, agents, graphs, documents and runs as entities and links | The backend projects them there (ids, names, states and links, never content); the Map page embeds onto-intelli's own UI, signed in as the member.                          |
| Schema changes             | The live schema, its versions and its proposals (`SchemaService`)                              | The assistant reads the schema, dry-runs a change and files a proposal; people approve it in onto-intelli. Settings → Ontology lists what is open.                         |
| The audit's facts          | Agents, runs, documents, tools, hosts and policies with the events between them                 | "Ask the audit" answers from them (`docs/AGENTSECOPS.md` §4.5).                                                                                                           |

Each nexc workspace is a tenant of its own there, and every call names who makes it: a member with
the roles of their workspace role, the assistant and the run engine as agents, the backend itself
only for what it writes on everyone's behalf. Without onto-intelli (`NEXC_ONTO_MODE=off`) graphs keep
a read-only starter vocabulary and the map, the proposals and the audit's questions are off.
<!-- #endregion concept -->

<!-- #region operate -->
## 1. Modes

`NEXC_ONTO_MODE` picks how the backend reaches onto-intelli:

| Mode       | When                                    | What it does                                                                                                                                                                                                                                          |
| ---------- | --------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `service`  | the default when `NEXC_ONTO_URL` is set | gRPC to `onto serve` at `NEXC_ONTO_URL` (`ExecutionService`, `SchemaService`, `OntologyService`, `EventService`, `IntelliService`), and its HTTP API on the same port for browser sessions (`POST /v1/auth/session`).                            |
| `embedded` | not in this build                       | Refused at start with a pointer to this section: see below.                                                                                                                                                                                           |
| `off`      | the default without `NEXC_ONTO_URL`     | Every graph has the starter vocabulary (topic, research, task, code, document, output; `depends_on` blocking, `relates_to` not), read-only: `PUT /graphs/{gid}/ontology` answers 409 and plans add no types. The Map page and Settings → Ontology say that onto-intelli is not configured; the audit's ask answers 503. |

`service` with no `NEXC_ONTO_URL` is refused at start.

### Embedded mode: why it is not built, and what it needs

Embedding would link onto-intelli into the backend (`Hub::with_postgres` on nexc's own sqlx 0.9 pool,
`hub.tenant("ws_…")`, `Principal::named(…)`, `ws.call(…)`), behind an off-by-default cargo feature
`onto-embedded`, as a third implementation of the `OntologyBackend` trait next to the gRPC client
and the test stub. It cannot be added today without breaking the build of everyone without a sibling
checkout: onto-intelli is published nowhere (not on crates.io, and its repository is not pushed), so
the only dependency that resolves is a path dependency, and an optional path dependency still takes
part in `Cargo.lock` resolution, so `cargo build --locked` fails in CI and for anyone without
`../onto-intelli`.

What it needs, in this order:

1. onto-intelli published: its crates on crates.io, or its repository pushed so a git dependency
   can name it.
2. A dependency pinned to a release or a revision, never a branch:
   `onto-intelli = { git = "https://github.com/f8fvwgzc/onto-intelli", rev = "<commit>", optional = true, default-features = false, features = ["postgres"] }`
   and `[features] onto-embedded = ["dep:onto-intelli"]`, with `Cargo.lock` updated in the same
   commit.
3. `backend/src/ontology/embedded.rs` implementing `OntologyBackend` over `Hub` (each method is one
   `ws.call(&principal, "<action>", json)`: `execution.vocabulary`, `execution.set_vocabulary`,
   `execution.init_graph`, `schema.get`, `schema.plan`, `schema.propose`, `schema.proposals`,
   `graph.traverse` and the rest), and `AppState::new` building it for `NEXC_ONTO_MODE=embedded`.
   Browser sessions still need `onto serve` for the UI, so the embedded mode serves the backend's
   reads and writes, not the map.
4. Memory and start-up: every backend process then holds the indexes of the tenants it opened, about
   2.6 GB and 25 s per million entities (onto-intelli's `docs/BENCHMARK.md`), and tenants stay open
   once used. Size the backend replicas for it, or keep `service`.

## 2. Who a call is made as

The backend has one token toward onto-intelli (`NEXC_ONTO_TOKEN`), configured there with
`impersonate = true`. A call made for someone carries the impersonation headers, so onto-intelli
checks and audits that person, not the backend:

| Header                  | Value                                                      |
| ----------------------- | ---------------------------------------------------------- |
| `x-onto-tenant`         | `ws_<workspace id without dashes>`                         |
| `x-onto-workspace`      | `{NEXC_ONTO_WORKSPACE_PREFIX}{workspace id}` (`nexc-…`)    |
| `x-onto-act-as`         | `user:<user id>`, `assistant:<user id>`, `agent:runtime`   |
| `x-onto-act-roles`      | the roles below, comma-separated                           |
| `x-onto-act-workspaces` | the onto-intelli workspace of the nexc workspace           |
| `x-onto-act-entity`     | `person:<user id>` (members and their assistant)           |

| nexc                                | Calls the backend makes for them      | Their browser session (onto-intelli's UI) | What onto-intelli lets them do (with the governance `deploy/onto` ships)                                                  |
| ----------------------------------- | ------------------------------------- | ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| workspace owner or admin            | `member`, `approver`, `schema_admin`  | `viewer`, `approver`, `schema_admin`      | through nexc: what nexc allowed (a graph's vocabulary, a plan's types); in the UI: read, approve or reject proposals, apply schema changes directly |
| member                              | `member`                              | `viewer`                                  | through nexc: what nexc allowed; in the UI: read, propose schema changes                                                  |
| guest                               | `viewer`                              | `viewer`                                  | read                                                                                                                      |
| the assistant, working for a member | `agent`                               | —                                         | read, plan and propose schema changes; never apply or approve them, whatever else it holds                                |
| the run engine                      | `agent`                               | —                                         | read graph vocabularies (a private team's graph is read as the backend, §4)                                               |
| the backend itself (no `act-as`)    | the token's own principal (`admin`)   | —                                         | the projection, the audit's facts, the first vocabulary of a graph and the migration                                      |

The roles the backend may assert are limited by the token's `impersonate_roles` (`viewer`, `member`,
`agent`, `approver`, `schema_admin`; never `admin`), and `[governance]` gives `schema_admin` the right
to apply and `approver` the right to approve. A guest who works on a team graph therefore reads its
vocabulary but cannot change it (403). The single admin principal is only the backend's own: no
user-facing call is made as it.

**A browser session never holds a write role.** onto-intelli grants `member` the right to write
every entity it can see, and has no per-entity rule that mirrors nexc's own: who may work on which
graph, who is in which team. A session with `member` could rewrite any graph's vocabulary (its
executors, roles, code execution, blocking flags), link itself into `team:admins` or a private team
and so read what is restricted to it, or edit the audit's facts. So what nexc projects is written
only by the backend, after nexc's checks (`PUT /graphs/{gid}/ontology` checks graph access, the
40-type limit and the blocking cycles), and a graph's types are edited in nexc, not in onto-intelli's
UI. Applying an approved schema change still works for owners and admins: onto-intelli runs its data
migration as the system. `team:admins` is itself restricted to its members (§5).

## 3. One tenant per workspace

Each nexc workspace is the tenant `ws_<id>` (PostgreSQL schema `onto_ws_<id>` on onto-intelli's
database), created on its first use: the first graph read or written, the first export of the
workspace, the first session. Inside it, the workspace's entities live in one onto-intelli workspace,
`{NEXC_ONTO_WORKSPACE_PREFIX}{workspace id}`. A workspace's schema, its proposals and its audit trail
are its own; one workspace's members cannot address another's tenant, and a browser session is bound
to one tenant.

## 4. Graph vocabularies

A graph is the entity `execution_graph:<graph id>`; its node types and relation types are
`node_type:<graph id>.<key>` and `relation_type:<graph id>.<key>`, in nexc's own field names (key,
label, description, colour, icon, default role, default executor, stage, `produces_artifact`,
`allow_code_exec`; key, label, description, `blocking`).

- **Reads** go through an in-process cache per graph that lives `NEXC_ONTO_VOCABULARY_TTL_SECS` (30 s;
  0 asks every time). The editor reads as the member who edits, the run engine as `agent:runtime`.
  onto-intelli answers types in stage then key order, so a node without a `kind` gets the first type
  that neither frames the work, delivers it nor runs code (`task` in the starter set).
- **A reader onto-intelli does not show the graph to**: a private team's graph is restricted to the
  team and `team:admins` (§5), which leaves out the run engine, and a member the projection has not
  named in the team yet. onto-intelli answers them "not found", so the read is made again as the
  backend's own principal: nexc checked already that they may work on the graph, and the cache
  serves every reader alike. A write that onto-intelli refuses this way answers 403 (try again once
  the projection caught up).
- **A graph onto-intelli does not know yet** (new, or created while it was unreachable), that is one
  the backend's own principal does not find either, is initialised on its first read: with the
  ontology nexc stored for it before this release, when it was not migrated yet, else with the
  starter set. A graph onto-intelli holds is never initialised again. The projection (§5) states
  every graph as the same entity its types hang from, so a graph it reached first reads as a
  vocabulary without a type: nexc never writes one without types, so that one is initialised too,
  unless the graph is recorded as migrated (then someone emptied it in onto-intelli).
- **Writes**: `PUT /graphs/{gid}/ontology` keeps nexc's own rules (at most 40 types of each sort,
  slugged keys, a type that nodes or edges use cannot be removed: 422, a relation made blocking may
  not close a dependency cycle: 409), then plans the change against what onto-intelli holds and
  applies it there with `SetVocabulary` as the member, in one atomic, audited write, while the graph
  row is locked. The new vocabulary replaces the cache entry here, every other replica drops its
  entry (`PostgreSQL NOTIFY`), and the graph's sockets get `ontology.updated`.
- **A plan's new types** are added when the plan is applied, as the member: to what onto-intelli
  holds once the graph row is locked (read again then, never from the cache), and only them
  (`InitGraph` upserts the types it is given and leaves every other one as it is). A type added,
  changed or removed in onto-intelli or through the editor meanwhile stays as it is.
- **Edits made in onto-intelli itself** (an approved proposal's data migration, an operator's own
  token; browser sessions are read-only, §2) reach nexc within the cache's lifetime. Edges follow their relation's `blocking` flag, unless that would close a
  dependency cycle (the edges then keep theirs and the backend logs the conflict), and open canvases
  get `ontology.updated`.

## 5. The workspace map

The backend projects the workspace into its tenant through the same outbox as the audit's facts
(`onto_export_queue`, filled by the tables' triggers; one worker, `SKIP LOCKED`, at least once). A
workspace is projected whole once (`onto_export_cursors.projected_at`), then by change:

| nexc               | Entity                          | Links stated from it                                                                                                                                       |
| ------------------ | ------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| team               | `team:<id>`                     | —                                                                                                                                                          |
| member             | `person:<user id>` (name, role) | `member_of` each team, and `team:admins` for owners and admins                                                                                             |
| project            | `project:<id>` (name, state)    | `led_by` its lead                                                                                                                                          |
| cycle              | `cycle:<id>` (name, dates)      | `cycle_of` its team                                                                                                                                        |
| issue              | `issue:<id>` (`ENG-12 Title`, state, priority) | `belongs_to` its team and project, `assigned_to` its assignee and its agent, `reported_by` its creator, `part_of` its parent, `in_cycle`, `planned_by` its graph |
| graph              | `execution_graph:<id>` (name)   | `in_team`, `created_by`                                                                                                                                    |
| document           | `document:<id>` (name, status, label) | `uploaded_by`                                                                                                                                         |
| run                | `run:<id>` (status)             | `run_of` its graph, `started_by` who started it                                                                                                            |
| agent              | `agent:<principal>` (the audit's) | `member_of` its team                                                                                                                                     |

Only ids, names, states and links are projected: no description, no document text, no e-mail
address. A private team, its cycles, issues and graphs and the runs of those graphs are
`visibility = "restricted"` with the team and `team:admins` (the workspace's owners and admins) on
their `acl`; making a team private or public projects all of them again, and so does moving a graph
to another team for its runs. `team:admins` is restricted to its own members, so nobody else sees it
or can link themselves to it. A confidential or restricted document is shown by its id, and a
restricted one only to `team:admins`. `onto_projection` remembers what was put there and the links
stated from each entity: a link that no longer holds is taken back, and only what was put there is
deleted when it goes, with what goes along with it in nexc: a deleted team's cycles, a deleted
graph's runs and its node and relation types (they are queued while their parent still says which
workspace they were in). The kinds and relations nexc adds to the built-in enterprise schema are in
`deploy/onto/nexc-workspace.toml`.

**A deleted workspace's tenant** is retired (`onto_retired_tenants`): the export deletes what the
projection put there and the vocabularies of the workspace's graphs, a few hundred entities per pass,
and forgets what was queued for the workspace. onto-intelli has no call that drops a tenant, so the
rest (the audit's facts and their events, the tenant's schema versions and proposals) stays in its
PostgreSQL schema until the operator drops it on onto-intelli's database:
`DROP SCHEMA IF EXISTS "onto_ws_<workspace id without dashes>" CASCADE`. `SELECT workspace_id,
cleared_at FROM onto_retired_tenants` lists the tenants to drop.

**The Map page** (Settings → Workspace map, members and above) embeds onto-intelli's UI at
`NEXC_ONTO_PUBLIC_URL/ui/`. The backend mints the member's own session with
`POST /workspaces/{wid}/ontology/session`: a short-lived token (`NEXC_ONTO_SESSION_TTL_SECS`) for
their principal in the workspace's tenant, read-only there (§2), which cannot impersonate anyone.
The page renews it once three quarters of its lifetime are spent (from its `expires_at`, whatever
the TTL), and at once on return to a tab that was hidden past that point. The page loads the
frame, waits for `{type: "onto:ready"}` from the frame's origin and hands it
`{type: "onto:session", token, tenant, workspace}` with `postMessage` to that origin only. The
backend's own token never reaches a browser, and the session never travels in an address. Issues,
agents and runs have an "On the map" link that opens `#/entities/<id>` in the same frame; Settings →
Ontology opens `#/schema` and `#/proposals/<id>`. onto-intelli must list the console's origin in
`[ui] embed_origins`, and the console must allow onto-intelli's: its Content-Security-Policy has
`frame-src` set to the origin of `NEXC_ONTO_PUBLIC_URL` (the frontend container's nginx writes it at
start, `frontend/docker-entrypoint.d/40-nexc-frame-src.sh`; `'none'` without one), never a wildcard.
The console's build carries no policy of its own: nginx sends it, and `vite preview` sends the same
(the UI suite checks that the frame loads under it).

The day timeline (Settings → Activity) stays in nexc: it is activity, not ontology.

## 6. Proposals and the assistant

When onto-intelli is configured, the assistant is given the workspace's schema (`get_schema`: kinds
with their states, link types, action types) and may answer a request to change it with a proposal.
The backend dry-runs each one (`plan_schema_change`) and files it (`propose_schema_change`) as the
member's assistant (`assistant:<user id>`, role `agent`), with a rationale and `nexc-assistant` as
its source. It never applies or approves: onto-intelli refuses agents that.
The reply links each proposal; Settings → Ontology lists the open proposals of the workspace with a
link to review each in onto-intelli's inbox, where approvers (owners and admins) approve or reject
it. A guest's assistant reads no schema and proposes nothing.

## 7. Migration from the ontology nexc stored

Before this release every graph stored its ontology in `graphs.ontology`. `nexc onto migrate` copies
it into onto-intelli, once per graph, as the backend's own principal (`InitGraph`, then
`SetVocabulary` when onto-intelli held other types), and records each graph in
`onto_vocabulary_migrations`:

- **Idempotent**: a migrated graph is never written again, so what was edited in onto-intelli since
  is kept; a graph onto-intelli already holds is only recorded (a copy that stopped before recording
  it wrote it already).
- **Safe while people work**: every copy, by the pass or by a first read on any replica, holds the
  graph's lock (a transaction-scoped advisory lock) and looks again under it, so a graph opened and
  edited while the pass worked through its batch is left as it is. One replica runs the start-up
  pass at a time (`pg_try_advisory_lock`); the others wait until nothing remains.
- **Resumable**: a graph that fails (onto-intelli unreachable, a type it refused) is recorded with
  its error and tried again on the next pass; the command exits non-zero while any failed.
- **At start**: the backend runs the same pass in the background (`NEXC_ONTO_MIGRATE_ON_START=true`)
  and tries failed graphs every minute until none fails. A graph read before the pass reached it is
  copied on that read.

From then on `graphs.ontology` is neither read nor written: new graphs leave it `{}`. One exception:
a workspace transfer (`POST /workspaces/{wid}/transfers`) reads each graph's vocabulary from
onto-intelli into the copied row's `graphs.ontology`, since the target's onto-intelli knows nothing
of it; there the graph is not migrated yet, so its first read copies that vocabulary in. The
transfer fails, and moves nothing, while onto-intelli does not answer.

An installation that runs with `NEXC_ONTO_MODE=off` does not read the stored ontologies either:
every graph has the starter vocabulary. Nodes and edges of types it does not have are kept and still
run in their order (an edge keeps its `blocking` flag), but such a node runs as an unknown type (role
`engineer` unless it names one, no artifact, no code execution), and new nodes and edges must use
the starter types. Turn onto-intelli on to keep a graph's own types.

The audit's facts move to the workspace's tenant: the migration that brings the projection resets
the export cursors, so each workspace's facts are backfilled into its tenant once more. The facts
already in the default tenant stay there.

### Dropping `graphs.ontology` (one release later)

Not an active migration yet: it ships with the next release, once every installation ran this one.
Before it, check that nothing remains: `SELECT count(*) FROM graphs g WHERE g.workspace_id IS NOT
NULL AND NOT EXISTS (SELECT 1 FROM onto_vocabulary_migrations m WHERE m.graph_id = g.id AND
m.migrated_at IS NOT NULL)` is 0, or `nexc onto migrate` says `0 remaining`.

```sql
-- TODO(next release): backend/migrations/<next>_drop_graph_ontology.sql
-- ALTER TABLE graphs DROP COLUMN ontology;
-- (then remove repo::onto_migrations::legacy_ontology and the legacy branch of
--  ontology::migrate::initial, which read it)
```

## 8. When onto-intelli is not there

| What                                  | Behaviour                                                                                                                                                                       |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A graph read (canvas, editor)          | A vocabulary kept from before is served, however old; without one, 503 `ontology unavailable`.                                                                                 |
| A graph created                       | Created; its vocabulary is initialised on its first read.                                                                                                                       |
| `PUT /graphs/{gid}/ontology`           | 503; nothing changes in nexc either.                                                                                                                                            |
| A run                                 | Queued as usual. Each node uses the vocabulary the run started with, or one kept from before; without one it fails with a retryable `ontology unavailable`, tried again with backoff within `NEXC_MAX_ATTEMPTS`, then fails for good (its descendants are skipped). The attempts spent waiting count: once the vocabulary comes, the node runs on from the attempt it reached. |
| The projection and the audit's facts | Stay queued and go out once it answers (the export retries every 2 s).                                                                                                         |
| The Map page                          | The session cannot be minted: an error with a retry.                                                                                                                            |
| Settings → Ontology                   | Says onto-intelli did not answer; lists nothing.                                                                                                                                |
| The assistant                         | Answers without the schema; a proposal it asked for is reported as not filed.                                                                                                   |
| The migration                         | Records the failure; tried again a minute later, and on the next read of the graph.                                                                                             |

A refusal (403) is not retried: it means the token, the roles or the tenant are not what
onto-intelli expects.

## 9. What was removed, and why

| Removed                                                                       | Why                                                                                                                                                                                                         |
| ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `graphs.ontology` as the source of truth (`repo::graphs::set_ontology`, `ontology()`) | One place for the vocabulary: versioned, audited, permission-checked and on the map. The column stays one release for the migration, then goes (§7).                                                    |
| The SQL workspace map: `GET /workspaces/{wid}/map`, `/map/{kind}`, `/map/{kind}/{id}` (`repo::map`, `WorkspaceMap`, `MapItem`, `MapKind`, `MapNeighbourhood`) | It counted and followed a fixed set of ties in SQL; onto-intelli renders the workspace as entities and links, with search, paths, impact and history, and lets people configure kinds, links and actions. No other part of nexc used the counts. |
| The frontend's map explorer                                                   | Replaced by the embedded UI.                                                                                                                                                                                |

## 10. Running it

- **Compose**: `make demo-onto`, or `docker compose -f docker-compose.yml -f docker-compose.onto.yml
  --profile onto up -d --build`. onto-intelli is built from `../onto-intelli` (or pulled as
  `ONTO_IMAGE`, an image you built and pushed: onto-intelli publishes none yet) and keeps its data on
  a PostgreSQL server of its own, `onto-postgres` (`pgvector/pgvector:pg17`, which it needs, on the
  volume `onto-pgdata`), as the login `onto` (`ONTO_POSTGRES_PASSWORD`). nexc's PostgreSQL, its
  image and its volume stay as they are, so an installation started with `make demo` keeps its data,
  and onto-intelli, which browsers reach, holds no credential of nexc's database. It is published for
  browsers on `ONTO_HTTP_PORT` (8081), and the console's Content-Security-Policy lets it frame that
  origin (`NEXC_ONTO_PUBLIC_URL`, given to the frontend container too). `make init` generates
  `NEXC_ONTO_TOKEN`, `ONTO_SESSION_SECRET` and `ONTO_POSTGRES_PASSWORD`.
- **Helm**: `ontoIntelli.enabled=true` adds a Deployment and a Service (`onto-intelli:50051`). Its
  image must be named (`ontoIntelli.image.repository`): build it from `../onto-intelli` and push it to
  your registry, since onto-intelli publishes none yet; the chart refuses to render without one. Its
  data goes in its own database on the chart's PostgreSQL, owned by its own login `onto`
  (`ONTO_POSTGRES_PASSWORD` in the Secret; an init container creates the role and the database and
  pgvector as the superuser, and takes `CONNECT` on nexc's database from everyone else), so
  `images.postgres` must then have pgvector (on a new installation; an existing Alpine volume moves
  with `pg_dump` and `pg_restore`, never by pointing the new image at it). With
  `postgres.enabled=false`, put `ONTO_DATABASE_URL` in the Secret. Browsers reach it at
  `ontoIntelli.ingress.host` (https when `ingress.tls` lists it), or `backend.ontoPublicUrl`: one of
  the two is required, since a browser cannot reach the Service, and it becomes both
  `NEXC_ONTO_PUBLIC_URL` and the frontend's frame-src. `ontoIntelli.embedOrigins` defaults to the
  console's ingress host. The Secret needs `NEXC_ONTO_TOKEN`, `ONTO_SESSION_SECRET` and
  `ONTO_POSTGRES_PASSWORD`.
- **`make dev`** starts `onto serve` from `../onto-intelli` (or `ONTO_DIR`) on `127.0.0.1:50051` when
  `NEXC_ONTO_MODE=service`, on the dev database (`NEXC_DB_IMAGE` with pgvector), with the dev web
  server's origin allowed to embed its UI; otherwise it says how it runs (`NEXC_ONTO_MODE=off`).
  When the dev database has no pgvector (the default Alpine image) it does not start onto-intelli,
  says how to switch images (a new `NEXC_DB_VOLUME`, data moved with `pg_dump` and `pg_restore`),
  and runs the backend with `NEXC_ONTO_MODE=off` instead of letting the stack go down.
- **Configuration**: `deploy/onto/onto-intelli.toml` (PostgreSQL, multi-tenant, read-your-writes
  across replicas, `builtin:nexc-execution` plus `nexc-audit.toml` and `nexc-workspace.toml`, the
  governance of §2, the impersonating token, `[ui] embed_origins`). Several `onto serve` replicas can
  share the database; set the same `ONTO_SESSION_SECRET` on each.
- **Checking it**: Settings → Infrastructure says whether it answers. `nexc doctor` also mints a
  one-minute session for a throwaway principal with every role nexc asserts, in the default tenant
  (nothing is created): a token without `impersonate = true`, or whose `impersonate_roles` lack one,
  fails it, since every member, assistant and run engine call would be refused. It warns when
  `NEXC_ONTO_PUBLIC_URL` (or `NEXC_ONTO_URL` standing in for it) names a host only the cluster
  resolves, or `/ui/` does not answer there, and says when `NEXC_ONTO_MODE=off` leaves a set
  `NEXC_ONTO_URL` unused.
  `make test-onto-live` builds `../onto-intelli` (a no-op when it is up to date; `ONTO_BIN` names a
  binary to use as it is) and runs the backend's live tests (`backend/tests/onto_live.rs`) against a
  real `onto serve` on a throwaway PostgreSQL with pgvector, or on `ONTO_LIVE_DATABASE_URL` (its
  database `onto` created there when missing); CI runs the same flows against the in-memory
  stub (`backend/src/ontology/stub.rs`, `backend/tests/ontology.rs`), which serves the same gRPC
  services and the session endpoint.
<!-- #endregion operate -->
