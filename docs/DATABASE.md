# Database

How the PostgreSQL schema stays correct and fast: its keys, the indexes that foreign keys and
polls need, the append-only tables and their triggers, the tables tuned for updates, and the rules
migrations follow.

<!-- #region keys -->
## Keys

**Primary keys are UUID v7**, minted by the backend (`Uuid::now_v7()`), not by the database. A v7
id starts with its creation time in milliseconds, which buys two things:

- New rows land at the right edge of the primary key's btree, as a sequence's would: no random
  page splits, a small hot set of pages.
- Ordering by id is ordering by creation. The sessions list (`repo::sessions::list`) and the trace
  explorer (`repo::secops::search_spans`) page by `id DESC` (`id < $before`) on the primary key,
  and an agent's A2A tasks on `(agent_id, id DESC)`, with no index on a timestamp.

The exceptions are deliberate:

| Key                                    | Tables                                                                                                                                       |
| -------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| An identity `bigint`                   | `onto_export_queue`, `realtime_outbox` (queues: the order of insertion is the order of work)                                                 |
| The columns that make the row          | `workspace_members`, `team_members`, `issue_labels`, `node_runs` (run, node), `span_payloads` (span, kind), `idempotency_keys`, `artifact_file_versions` (file, version), `onto_projection`, `sso_role_grants` |
| The parent's key (one row per parent)  | `llm_settings`, `workspace_llm_settings`, `workspace_guardrails`, `workspace_knowledge_settings`, `agent_baselines`, `agent_session_baselines`, `agent_evaluations`, `onto_export_cursors`, `audit_chain_heads`, `onto_vocabulary_migrations`, `onto_retired_tenants` |
| A digest                               | `realtime_tickets` (ticket), `oidc_logins` (state)                                                                                           |
| A constant                             | `otlp_export_cursor` (one row, `id = 1`)                                                                                                     |

A few ids are random (v4, `gen_random_uuid()`): notifications written in bulk by SQL (approvals,
alerts, run reports, due reminders) and `agent_risk_history`. Do not page those by id.

Every table has a primary key.
<!-- #endregion keys -->

<!-- #region foreign-keys -->
## Foreign keys

**Every foreign key has an index whose leading columns are the key's columns.** An `ON DELETE
CASCADE`, `SET NULL` or `RESTRICT` runs one statement on the child table for every parent row
deleted, `... WHERE <column> = $1`. Without an index each of those reads the whole child table.
Deleting a graph with a hundred runs once meant a hundred and one passes over the whole usage
ledger (`llm_usage.run_id`, `llm_usage.graph_id`). `20261024000400_foreign_key_indexes.sql` gave
the 51 keys that had none an index named `<table>_<column>_idx`, and `tests/schema.rs` fails on a
key without one, so a migration that adds a foreign key adds its index in the same file.

- The index may have more columns after the key's (`memories_graph_idx (graph_id, updated_at
  DESC)`, `run_commands_run_idx (run_id, created_at)`), or be the primary key or a unique key that
  leads with them (`a2a_tasks_run_key`).
- For an optional (nullable) single-column key, a partial index `WHERE <column> IS NOT NULL`
  counts. The delete's `<column> = $1` implies the predicate, so the planner uses it, and the rows
  without a value cost nothing to index. Most keys that name a person are indexed this way.
- The server never deletes an account's row (erasure keeps it, nameless, `engine::account`), and
  those keys are indexed all the same. An operator may remove a user, and erasure itself finds an
  account's rows by those columns (`audit_log.actor_id`, `platform_events.actor_id`,
  `projects.lead_id`).

What a key does on delete: `CASCADE` for what a parent owns (a workspace's teams, a run's spans),
`SET NULL` for references to people and for optional links (an issue's project, cycle, agent),
`RESTRICT` where deleting the parent must be refused while children remain (`graphs.team_id`,
`issues.state_id`).
<!-- #endregion foreign-keys -->

<!-- #region invariants -->
## Invariants

What the code takes for granted about the rows is a constraint, so a race or a bug fails a write
instead of leaving rows the code then reads wrong: an issue number per team
(`UNIQUE (team_id, number)`), a position per audit chain and per span chain
(`audit_log_chain_key`, `UNIQUE (run_id, seq)`), a name per sibling artifact, one identity per
provider and account, one response per idempotency key, and:

| Invariant                                         | Constraint                                                                              | Written by                          |
| ------------------------------------------------- | --------------------------------------------------------------------------------------- | ----------------------------------- |
| One live managed graph per external agent         | `graphs_external_agent_live_key (external_agent_id) WHERE ... AND deleted_at IS NULL`   | `repo::external_runs::ensure_graph` |
| One live managed graph per agent exposed over A2A | `graphs_a2a_agent_live_key (a2a_agent_id) WHERE ... AND deleted_at IS NULL`             | `repo::a2a::ensure_graph`           |
| One A2A task per run                              | `a2a_tasks_run_key (run_id)`                                                            | `repo::a2a::insert_task`            |

A writer that may meet the row it would create inserts with `ON CONFLICT DO NOTHING` and reads
back the row that won (`ensure_graph`, which looks first and creates on first use).

The migrations that add these check the data first. Where the rows already break the invariant,
they leave the constraint out and write a warning to the server's log. To find those rows:

```sql
SELECT external_agent_id, array_agg(id ORDER BY id) FROM graphs
 WHERE external_agent_id IS NOT NULL AND deleted_at IS NULL
 GROUP BY 1 HAVING count(*) > 1;   -- likewise a2a_agent_id, and run_id of a2a_tasks
```

Each of those graphs holds runs and their evidence. Decide which one stays live and archive the
others (set `deleted_at`): an archived graph's runs keep their evidence until the retention purge
removes the graph. Then create the index with the statement of the migration.
<!-- #endregion invariants -->

<!-- #region indexes -->
## Indexes

No two indexes of a table index the same keys (columns or expressions, operator classes,
orderings and partial predicate). An index whose keys are a prefix of another's over the same
rows is dropped unless it backs a constraint. `artifacts_run_idx` went (20261024000400): the
unique key `(run_id, node_id, path)` serves everything it did. The one-column
`run_commands_run_id_idx` and `memories_graph_id_idx` went too, once `run_commands_run_idx` and
`memories_graph_idx` led with the same column over the same rows.

**Queues and polls read an index over the rows they poll for**, partial wherever those are a few
among many, so the index stays the size of the work, not of the history:

| Poll                                                        | Index                                                                                                          |
| ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| Dispatchers claim queued runs, takeovers read running ones  | `runs_active_idx (status, created_at) WHERE status IN ('queued', 'running')`                                   |
| The alert worker takes due deliveries (`SKIP LOCKED`)       | `alert_deliveries_due_idx (next_attempt_at) WHERE status = 'pending'`                                          |
| The knowledge worker takes waiting documents                | `documents_unfinished_idx (status) WHERE status NOT IN ('ready', 'failed')`                                    |
| Overdue firewall approvals are expired                      | `approvals_pending_expiry_idx (expires_at) WHERE status = 'pending'`                                           |
| The firewall counts an agent's pending approvals            | `approvals_pending_agent_idx (agent_id) WHERE status = 'pending'`                                              |
| The cancel watcher takes a run's commands, every second     | `run_commands_run_idx (run_id, created_at)` (a run has a handful)                                              |
| Graph vocabularies still to copy to onto-intelli            | `onto_vocabulary_migrations_pending_idx (graph_id) WHERE migrated_at IS NULL`                                  |
| Plans a crashed process left streaming, every minute        | `plans_streaming_idx (updated_at) WHERE status = 'streaming'`                                                  |
| Due-date reminders, every 15 minutes                        | `issues_due_reminder_idx (due_date) WHERE assignee_id IS NOT NULL AND due_notified_on IS DISTINCT FROM due_date` |
| Accounts whose old tokens may still be alive                | `users_session_epoch_at (session_epoch_at) WHERE session_epoch_at IS NOT NULL`                                 |
| Open alerts and quarantined artifacts on the console        | `alerts_open_idx`, `artifacts_quarantined_idx`                                                                 |

A partial index is only used when the query's `WHERE` implies its predicate, so the predicate is
spelled the way the query spells it.

**A list that ends in `ORDER BY ... LIMIT` has an index in that order**, so the first page is read,
not sorted out of every row: the issue list (`issues_workspace_idx`), the audit log, alerts,
account events, deliveries, an agent's spans, a workspace's approvals (`approvals_recent_idx`).

**BRIN** (a minimum and maximum per block range) suits a column that grows in the order rows are
written, on a table whose old rows mostly stay where they are. PostgreSQL widens a block range's
summary as new rows arrive in it and never narrows it on its own. It is on
`agent_spans.started_at` (`agent_spans_started_brin`), for the console's counts of the last day
and fortnight: spans are never updated and arrive roughly in the order they start. Two things
wear it down over time, and neither makes a count wrong (a BRIN index is always rechecked), only
slower:

- `started_at` is the reporter's clock (an external agent, OTLP), so a span reported late or with
  a skewed clock widens the range it lands in;
- the retention purge deletes the spans of removed graphs. Those are scattered over old pages,
  because runs interleave, and new spans reuse the space, so those old ranges come to cover today.

After a large purge, `REINDEX INDEX CONCURRENTLY agent_spans_started_brin` summarises every range
again, without holding span writes. BRIN is not used where:

- the query pages by id, because the UUID v7 primary key is in time order already;
- the time column follows a leading workspace or user column (`audit_log`, `llm_usage`,
  `account_events`), because the btree on both answers it;
- every old row is purged by age and its space reused (`span_payloads`, `account_events`,
  `idempotency_keys`, `refresh_tokens`): after one retention period every range would match every
  query. Those purges keep their btree on the timestamp.
<!-- #endregion indexes -->

<!-- #region append-only -->
## Append-only tables and their triggers

| Table          | What keeps it                                                                                                                                                                                                                       |
| -------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `agent_spans`  | Each run's spans form a hash chain (`seq`, `prev_hash`, `hash`, with `UNIQUE (run_id, seq)`). `agent_spans_append_only` refuses every update, and every delete outside a transaction that named itself a purge (`nexc.evidence_purge`). A statement trigger refuses `TRUNCATE`. |
| `audit_log`    | `audit_log_chain` (before insert) chains each entry to its workspace's head in `audit_chain_heads`, under a lock on that head, whoever writes it. `audit_log_append_only` allows one update, the erasure of names (`nexc.evidence_redact`), and deletes only in a purge. `TRUNCATE` is refused. |
| `runs`         | `runs_seal_final`: a sealed run's trace hash and signature cannot change.                                                                                                                                                            |
| `llm_usage`    | Written once per model call and never updated, by convention (no trigger).                                                                                                                                                           |

The other triggers feed the export to onto-intelli through `onto_export_queue`, which the export
worker polls (no trigger sends `NOTIFY`): spans as they are written, runs when they finish, agents
when what is exported of them changes, and the workspace projection's teams, projects, cycles,
issues, graphs, documents, members and runs. `agents_risk_history` keeps every risk score.
`teams_artifact_files_drop` and `issues_artifact_files_drop` decide what happens to an issue's
files.

**Triggers stay cheap.**

- `UPDATE OF <columns>` fires whenever a statement *sets* one of the columns, changed or not. A
  trigger that should fire on a change has a `WHEN (OLD.(...) IS DISTINCT FROM NEW.(...))` clause.
  A `WHEN` may compare `OLD` and `NEW` only on an update, so such a trigger is split in two, one for
  inserts (and deletes) and one for updates. 20261024000401 split every projection trigger this
  way (`agents_onto_export` and `agents_onto_export_update`, `issues_onto_project` and
  `issues_onto_project_update`, ...), and `tests/schema.rs` checks that a no-op update queues
  nothing. The update trigger lists every column its export reads: the agents' one also compares
  principal, manager and team (20261024100008).
- A trigger on a table written in batches is a statement trigger over the batch's transition
  table: one `INSERT ... SELECT` per statement instead of one function call and lookup per row
  (`agent_spans_onto_export`, `REFERENCING NEW TABLE AS written_spans`).
- A trigger reads other tables by indexed columns only, and what it inserts is a queue row.
<!-- #endregion append-only -->

<!-- #region hot-updates -->
## Hot updates

An update that changes no indexed column and finds room on the row's page is a HOT (heap-only)
update: no new index entries, and the old version is pruned in place. A table updated all day keeps
its hot columns out of every index (keys, expressions and partial predicates alike) and leaves
room on its pages:

| Table    | Fillfactor | Updated all day, never indexed                                                                                    |
| -------- | ---------- | ----------------------------------------------------------------------------------------------------------------- |
| `runs`   | 90         | `heartbeat_at`, `lease_expires_at`, `lease_secs` (each renewal), `tokens_in`, `tokens_out`, `cost_usd`, `cancel_requested`, `policy_mismatches`, `policy_versions` |
| `agents` | 90         | `heartbeat_at`, `spent_tokens`, `status`, `updated_at`, `risk_score`, `risk_reasons`, `risk_updated_at`            |
| `nodes`  | 90         | `status`, `output`, `updated_at`, `x`, `y`                                                                         |

`runs_lease_idx (lease_expires_at)` was dropped for this. Every lease renewal wrote an entry into
each index of `runs`, and the takeover poll read every running run anyway (it also takes runs
with no lease). Before indexing any of the columns above, weigh the index against every update
that stops being HOT.

A lower fillfactor does nothing where the updates change an indexed column anyway: `node_runs`
(its `status` is in partial predicates), `issues` (`updated_at` is the issue list's index),
`alert_deliveries` (`status`, `next_attempt_at`), `documents` while they are processed. A
fillfactor applies to pages written after it is set. A `VACUUM FULL` in a maintenance window
applies it to the rest. `pg_stat_user_tables.n_tup_hot_upd` against `n_tup_upd` shows how many
updates were HOT.
<!-- #endregion hot-updates -->

<!-- #region notify -->
## NOTIFY

The realtime fan-out between replicas uses `LISTEN`/`NOTIFY` on the channel `nexc_events`
(`repo::outbox`, `realtime::fanout`):

- One notification per hub event: node status, every streamed output delta and token count,
  every trace span, plan progress. Cache invalidations go together, up to 64 keys per
  notification.
- A payload is at most 7,500 bytes inline (PostgreSQL's limit is 8,000). A larger event is parked
  in `realtime_outbox` and its id sent instead. Parked rows are deleted after five minutes.
- Each notification is a transaction of its own, with a transaction id and a commit record, and
  wakes every listening replica. The rate follows the hub's: a model streaming its output sends
  a notification per chunk, whether or not anyone watches the graph on another replica.
<!-- #endregion notify -->

<!-- #region migrations -->
## Migrations

The schema is built by the migrations in `backend/migrations/`, which the server applies when it
starts. `nexc migrate` applies them and exits.

- **A migration that shipped is never edited.** sqlx stores each file's checksum, and a server
  whose file differs refuses to start (`VersionMismatch`). A fix is a new migration.
- Files are named `<timestamp>_<what>.sql` and applied once each, in order, under an advisory lock,
  so replicas starting together apply them once (`repo::migrate`).
- One concern per file. A new migration keeps every statement safe to run again: `IF NOT EXISTS`,
  `IF EXISTS`, `CREATE OR REPLACE FUNCTION`, `DROP TRIGGER IF EXISTS` before `CREATE TRIGGER`.
  Most earlier files predate the rule and are not re-runnable, `20261024000401` among them (a
  bare `DROP TRIGGER`, and its `*_update` triggers created with no `DROP TRIGGER IF EXISTS`).
- Migrations are additive: replicas of the previous version keep working against the new schema
  during a rolling update ([HA](HA.md)).
- A migration runs in one transaction by default and starts with `SET LOCAL lock_timeout = '5s'`
  when it touches tables in use. A lock it cannot get within five seconds fails the migration (the
  server retries on its next start) instead of queueing every reader and writer behind it.
- A unique constraint on data that already exists is created only where the data allows it, in a
  `DO` block that checks first and raises a warning otherwise. An installation never fails to
  start over rows it already has (`one_managed_graph_per_agent`, `one_a2a_task_per_run`).

### Building indexes

`CREATE INDEX` holds a lock that blocks writes to its table while it reads the table, and in a
migration's transaction it keeps that lock until the whole file commits (`lock_timeout` bounds
only the wait for it). On a table that grows with what people do (issues, projects, alerts,
settings) the build takes milliseconds to a second. It is done the plain way, in the migration's
transaction: atomic with the rest of the file and safe to retry. A file of plain builds touches
at most one table that a run writes under its lease, and builds it last, so the lock outlives
that build by nothing (`housekeeping_sweeps` ends with `issues`, which a finishing run reports
to).

On a table that grows with what agents do, a plain build would hold span ingest, lease renewals,
usage and audit writes for as long as it reads millions of rows, on the replicas still serving
during a rolling update. A write stalled inside a run's lease transaction holds the run row, its
renewal times out, and the replica gives up every run it holds. Those indexes are built with
`CREATE INDEX CONCURRENTLY`, and dropped with `DROP INDEX CONCURRENTLY`:

- The tables are `agent_spans`, `llm_usage`, `audit_log`, `node_runs`, `runs`, `agents`,
  `notifications` and `memories`, and `approvals` and `artifacts`, which a node writes inside its
  run's lease (`engine::approvals`, `engine::artifacts`).
- `20261024000400_foreign_key_indexes.sql` came before this rule and builds its indexes the plain
  way, in one transaction. On a large installation, build them concurrently before upgrading with
  the `psql` command in its header; the migration then finds them and does nothing.
- PostgreSQL refuses `CONCURRENTLY` inside a transaction, including the implicit one around a
  multi-statement query. Each such migration is a file of its own whose first line is
  `-- no-transaction` and which holds that one statement.
- A concurrent build reads the table twice and waits for every transaction whose snapshot is
  older than its own. A session blocked on a lock keeps its snapshot, so a server waiting for
  another's migrations must not block on the migration lock: `repo::migrate` asks for it again
  every half second instead, and takes sqlx's own lock only once it holds that one. Otherwise two
  servers starting together deadlock, and PostgreSQL ends the build.
- A build that fails, or a server stopped in the middle of one, leaves an **invalid** index
  behind. It is maintained by every write, used by no query, and skipped by `IF NOT EXISTS` when
  the migration runs again. A batch of concurrent builds therefore ends with a migration that
  refuses to go on while any of its indexes is invalid (`check_concurrent_index_builds`): the
  server does not start, and its error names each such index with the
  `REINDEX INDEX CONCURRENTLY` that rebuilds it. Run those with `psql`, one at a time, then start
  the server again. A plain rebuild inside the migration would hold the reads and writes of those
  tables until it committed, and a startup probe that cut it short would start it over. On a
  server, find invalid indexes with the query below.
- The startup probe of the Helm chart and of the manifests gives migrations 150 seconds. On a
  large installation, run `nexc migrate` with the new image, the old replicas still serving,
  before rolling the deployment, so no build is cut short by a restart.

```sql
SELECT indexrelid::regclass FROM pg_index WHERE NOT indisvalid;
```

### Checks

`backend/tests/schema.rs` reads the catalogue of a database every migration ran on, and fails
naming the offenders: a table without a primary key, a foreign key without an index whose leading
columns are the key's (a partial index counts only as `WHERE <column> IS NOT NULL`), an index that
repeats another (a plain b-tree whose columns lead another b-tree, or two indexes with the same
definition, partial, expression and unique ones included), and a projection trigger that queues an
export for an update that changed nothing it projects.
<!-- #endregion migrations -->

<!-- #region added -->
## Indexes added on 2026-10-24

Migrations `20261024100000` to `20261024100019`. They come after `20261024000400` (an index for
every foreign key, `artifacts_run_idx` dropped) and `20261024000401` (projection triggers split
and guarded), and build on both.

| Migration | Index or change | Serves |
| --------- | --------------- | ------ |
| `100000` | drop `runs_lease_idx` (concurrently) | lease renewals become HOT updates |
| `100001` | fillfactor 90 on `runs`, `agents`, `nodes` | HOT updates (above) |
| `100002` | `run_commands_run_idx (run_id, created_at)` in place of `run_commands_pending_idx` and `run_commands_run_id_idx` | the cancel watcher's poll and the cascade from runs |
| `100003` | `alerts_open_idx` | the console's open alerts |
| `100004` | `refresh_tokens_expiry_idx`, `plans_streaming_idx`, `issues_due_reminder_idx` | the sweeps every minute and every quarter hour |
| `100005` | unique `graphs_external_agent_live_key`, `graphs_a2a_agent_live_key` | one live managed graph per agent (`ensure_graph`) |
| `100006` | unique `a2a_tasks_run_key` in place of `a2a_tasks_run_idx` | one A2A task per run |
| `100007` | `agent_spans_onto_export` as a statement trigger | one queue insert per span batch |
| `100008` | `agents_onto_export_update` also compares principal, manager and team | a change of team or manager alone reaches onto-intelli |
| `100009` | `memories_graph_idx (graph_id, updated_at DESC)` (concurrently) | `memories::of_graph` after every node that learns, and the cascade from graphs |
| `100010` | `llm_usage_agent_day_idx (agent_id, created_at)` (concurrently) | an agent's spending today, checked before each of its nodes |
| `100011` | `node_runs_node_succeeded_idx` (concurrently) | upstream data labels, read as every node starts |
| `100012` | `node_runs_evidence_finished_idx` (concurrently) | the console's outputs by verdict, by day |
| `100013` | `runs_sealed_idx (finished_at DESC NULLS LAST, id DESC)` (concurrently) | the console's last seal |
| `100014` | `agent_spans_started_brin` (BRIN, concurrently) | the console's span and denial counts by window |
| `100015` | `approvals_pending_agent_idx` (concurrently) | the firewall's count of an agent's pending approvals |
| `100016` | `approvals_recent_idx (workspace_id, requested_at DESC, id DESC)` (concurrently) | the approvals list without a status filter |
| `100017` | `artifacts_quarantined_idx` (concurrently) | the console's quarantined artifacts |
| `100018` | stops, naming the `REINDEX INDEX CONCURRENTLY` to run, while an index of `100009` to `100017` is invalid | a concurrent build that did not finish |
| `100019` | drop `memories_graph_id_idx` (concurrently) | `memories_graph_idx` leads with the same column |

### Looked at and left

- `repo::usage::tokens_this_month` sums a workspace's month of usage at every admission when the
  workspace has a monthly budget. The index is right, the rows are many. A running total would
  serve it better than a wider index.
- The workspace timeline (`repo::insight`) bounds issues, graphs, runs and memories by when they
  happened, which their indexes do not lead with. A day of a large workspace reads its whole
  history of those. Their v7 ids are in creation order and could bound the scan.
- `memories` updates its access counts on every recall, and those updates seldom find room for a
  HOT version. Its rows are wide (embedding, text, search vector), and a fillfactor that leaves
  room for one would grow the table by about a third. Measure `n_tup_hot_upd` first.
- `node_runs_evidence_idx ((evidence->>'verdict'), finished_at)` serves no query on
  PostgreSQL 17, which cannot skip from one verdict to the next. It stays until a query or
  PostgreSQL 18's skip scan uses it, or until it is dropped on purpose.
- `realtime_outbox` is purged by `created_at` with no index. It holds five minutes of oversized
  events, a few pages.
- `audit_chain_heads` takes an update per audit entry, but holds a row per workspace on a few
  pages that pruning keeps room on. It stays at the default fillfactor.
- Two firewall calls asking for the same approval at once can each open one
  (`repo::approvals::find_pending_like`, then `insert`). A unique index over pending requests would
  turn the second into an error, and an expired request still marked pending would refuse the
  next one until the sweep. The code's dedupe stays as it is.
<!-- #endregion added -->
