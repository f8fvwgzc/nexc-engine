# ADR 0004: Indexes for the polls, HOT updates on the busiest tables, and index builds that hold no writes

- **Status:** accepted, 2026-10-24. **Extends ADR 0001** (Decision 3) and replaces nothing in it.
- **Not built or run yet:** implemented without building or running anything (no compiler,
  migration, test or `EXPLAIN` has seen it); "How to verify" lists what has to pass before
  relying on it.
- **Scope:** migrations `20261024100000` to `20261024100019`, `backend/src/repo/mod.rs`
  (`migrate`), `ensure_graph` in `repo::external_runs` and `repo::a2a`, `backend/tests/schema.rs`
  (`no_index_repeats_another`), `docs/DATABASE.md`

<!-- #region body -->
## Context

[ADR 0001](0001-http3-edge-caching-and-indexes.md) gave every foreign key an index
(`20261024000400`), split and guarded the projection triggers (`20261024000401`), and made
`backend/tests/schema.rs` keep both. The rule that every foreign key has an index whose leading
columns are the key is ADR 0001's and is not restated here. What it left:

- **Polls and lists that read whole tables** for a few rows: an agent's pending approvals before
  every approval the Agent Firewall opens, what an agent spent today before each of its nodes,
  the upstream labels as every node starts, a graph's memories after every node that learns, the
  approvals list polled every 15 s without a status, the console's open alerts, quarantined
  artifacts, outputs by verdict over a day, last seal and span counts over a day and a fortnight,
  and the sweeps of refresh tokens, stale plans and due reminders.
- **Lease renewals were never HOT updates.** Each renewal rewrites `runs.lease_expires_at`, which
  `runs_lease_idx` indexed, so it put a new entry into every index of `runs`; the takeover poll
  also takes runs with no lease, so it read every running run whatever that index held.
- **Two indexes repeated what a wider one could serve:** `run_commands_pending_idx` and
  `run_commands_run_id_idx` (the cancel watcher's poll needs `created_at` order),
  `memories_graph_id_idx` (`of_graph` sorts by `updated_at`), and `a2a_tasks_run_idx` beside an
  invariant the code assumes (one task per run).
- **Managed graphs were created look-then-insert,** so two first uses at once could give one
  external or A2A agent two live graphs and split its runs between them.
- **Two export triggers cost more or saw less than they should:** the span export ran a PL/pgSQL
  call and a run-to-graph lookup per span of a batch, and `agents_onto_export_update` (ADR 0001)
  compared seven columns but not the principal, manager or team the export reads.
- **Index builds held writes.** A plain `CREATE INDEX` in a migration holds writes to its table
  until the file commits; on the tables agents write all day that stalls span ingest and lease
  renewals on every replica still serving, and a write stalled inside a run's lease transaction
  can cost the replica its leases. sqlx's migration lock blocks (`pg_advisory_lock`), and a
  session blocked on a lock keeps a snapshot that a `CREATE INDEX CONCURRENTLY` waits for.
- **`no_index_repeats_another` looked only at plain b-trees** without a predicate or expressions,
  so two copies of one partial, expression or unique index passed it.

## Decision

1. **A poll or a list reads an index over the rows it wants**, partial where those are a few
   among many and in the list's `ORDER BY` where it ends in `LIMIT`, with a partial predicate
   spelled as the query spells it: `alerts_open_idx`, `refresh_tokens_expiry_idx`,
   `plans_streaming_idx`, `issues_due_reminder_idx` (`100003`, `100004`),
   `memories_graph_idx (graph_id, updated_at DESC)`, `llm_usage_agent_day_idx`,
   `node_runs_node_succeeded_idx`, `node_runs_evidence_finished_idx`, `runs_sealed_idx`,
   `approvals_pending_agent_idx`, `approvals_recent_idx (workspace_id, requested_at DESC, id DESC)`,
   `artifacts_quarantined_idx` (`100009` to `100017`).
2. **BRIN where a column grows in write order:** `agent_spans_started_brin` on
   `agent_spans.started_at` (`autosummarize = on`, `100014`), for the console's counts by window.
   Spans are never updated and arrive roughly in the order they start, so a few kilobytes of
   index find the window's pages at almost no cost per insert. A btree stays where the query
   pages by id, where the time column follows a leading workspace or user column, and where old
   rows are purged by age and their space reused.
3. **HOT updates on the busiest tables:** `runs_lease_idx` is dropped (`100000`, concurrently; the
   takeover poll uses `runs_active_idx`) and `runs`, `agents` and `nodes` get `fillfactor = 90`
   (`100001`), so lease renewals, heartbeats, token totals and node progress can stay on their
   page with no new index entries.
4. **A wider index replaces a narrower one, the foreign key rule kept:**
   `run_commands_run_idx (run_id, created_at)` replaces both `run_commands` indexes (`100002`);
   `memories_graph_idx` serves the cascade from graphs, and `100019` drops
   `memories_graph_id_idx` once `100018` has found the wider index valid; the unique
   `a2a_tasks_run_key` replaces `a2a_tasks_run_idx` (`100006`).
5. **Invariants the code assumes are unique indexes:** one live managed graph per external agent
   and per agent exposed over A2A (`graphs_external_agent_live_key`, `graphs_a2a_agent_live_key`,
   partial on `deleted_at IS NULL`, `100005`) and one A2A task per run. `ensure_graph` inserts with
   `ON CONFLICT DO NOTHING RETURNING id` and reads back the graph that won. Each migration checks
   the data first and, where existing rows already break the invariant, leaves the index out and
   raises a warning, so no installation fails to start over rows it has (`docs/DATABASE.md`,
   "Invariants", has the query to find them).
6. **Cheaper and complete export triggers:** `agent_spans_onto_export` becomes a statement
   trigger over the batch's transition table, one `INSERT ... SELECT` per write queueing the same
   spans in `ingested_seq` order (`100007`), and `agents_onto_export_update` compares ten columns,
   the principal, the manager and the team added (`100008`).
7. **Index builds on the tables agents write are concurrent.** `agent_spans`, `llm_usage`,
   `audit_log`, `node_runs`, `runs`, `agents`, `notifications` and `memories`, and `approvals` and
   `artifacts`, which a node writes inside its run's lease, get `CREATE INDEX CONCURRENTLY` and
   `DROP INDEX CONCURRENTLY`, one statement per `-- no-transaction` file. Tables that grow with
   what people do get plain builds in the migration's transaction, a file touching at most one
   table a run writes under its lease, and building it last.
8. **The migration lock is polled.** `repo::migrate` asks for
   `pg_try_advisory_lock(hashtext('nexc.migrate'))` every 500 ms on one connection and only then
   applies the migrations there (`MIGRATOR.run_direct`), taking sqlx's own lock at once; if they
   fail, the connection is closed so no lock outlives them. Two servers starting together no
   longer deadlock a concurrent build.
9. **A concurrent build that did not finish stops the start.** It leaves an invalid index that
   every write maintains, no query uses and `IF NOT EXISTS` skips the next time. `100018` fails
   while any index of `100009` to `100017` is invalid, its error naming the
   `REINDEX INDEX CONCURRENTLY` statement for each; the operator runs them with `psql` and starts
   the server again. Rebuilding inside the migration was rejected: a `REINDEX` there holds the
   reads and writes of those tables until the file commits, and a startup probe that cut it
   short would start it over on the next pod, again and again.
10. **One schema test catches every repeat.** `no_index_repeats_another` in
    `backend/tests/schema.rs` also fails on two indexes with the same definition (access method,
    keys or expressions, operator classes, collations, orderings and partial predicate), partial,
    expression and unique ones included, naming the plain one of a pair, else the later.

## Consequences

- Every new index costs a btree insert per row it covers, and the new partial predicates
  (`plans.status`, `alerts.acknowledged_at`, the artifacts' quarantine and release, an issue's
  due reminder, an approval's status) make the few updates that change them non-HOT.
- The fillfactor applies to pages written after `100001`; a `VACUUM FULL` in a maintenance window
  applies it to the rest. The takeover poll sorts the running runs by lease itself.
- A failed concurrent build keeps the new release from starting until someone runs the
  statements its error names; the replicas of the previous release keep serving meanwhile. On a
  large installation, run `nexc migrate` with the new image before rolling the deployment, since
  the startup probe of the chart and the manifests allows migrations 150 s.
- Whatever applies migrations with sqlx's blocking lock alone (the `sqlx migrate run` CLI, a
  replica of a previous release restarting during the rollout) can still deadlock a concurrent
  build; `100018` then says which index to rebuild.
- The agents' export also fires when only the principal, the manager or the team changes, an
  `ON DELETE SET NULL` included, so onto-intelli receives changes it never did before.
- `DROP TRIGGER` in `100007` and `100008` takes a brief exclusive lock on `agent_spans` and
  `agents`, bounded by the 5 s lock timeout.

## How to verify

Nothing in this record has been built, migrated or run. Before relying on it:

1. `make db-up`, then `make test-backend`: every test applies all the migrations, and
   `tests/schema.rs` checks the result (keys, foreign-key indexes, repeated indexes, the
   projection triggers).
2. Two servers starting together on an empty database (`NEXC_BACKEND_REPLICAS=2` in Compose):
   both start, the migrations run once, and no index is invalid:

   ```sql
   SELECT indexrelid::regclass FROM pg_index WHERE NOT indisvalid;              -- no rows
   SELECT relname, n_tup_upd, n_tup_hot_upd FROM pg_stat_user_tables
    WHERE relname IN ('runs', 'agents', 'nodes');                              -- HOT share rising
   ```

3. The stop: on a test database, mark one of the new indexes invalid
   (`UPDATE pg_index SET indisvalid = false WHERE indexrelid = 'runs_sealed_idx'::regclass`, as a
   superuser), delete the row of `20261024100018` from `_sqlx_migrations`, and start the server:
   it refuses, naming `REINDEX INDEX CONCURRENTLY runs_sealed_idx;`; after that statement it
   starts.
4. `EXPLAIN` the queries each migration's header names (`repo::approvals::pending_of_agent`,
   `repo::usage::agent_today`, `repo::runs::context_labels_of`, `repo::approvals::list`,
   `repo::secops::span_count`) on a database with realistic volumes: each uses its index.
5. `make prove-api` (CI's `api-latency` job runs it with scaled targets): the console's reads,
   the approvals list and the issue, node and run writes.
<!-- #endregion body -->
