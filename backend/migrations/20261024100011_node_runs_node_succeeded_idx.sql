-- no-transaction
-- The classifications upstream outputs were derived from. `repo::runs::context_labels_of`
-- (`WHERE node_id = ANY($1) AND status = 'succeeded'`) runs as every node starts, for its
-- upstream nodes, and read the whole table: the key of node_runs leads with the run. Only
-- successful results are looked for, so only they are indexed. `status` is in the predicate of
-- `node_runs_hash_idx` already, so a status change was never a HOT update anyway. Built
-- concurrently (docs/DATABASE.md, "Migrations"): node_runs takes a row per node of every run.
CREATE INDEX CONCURRENTLY IF NOT EXISTS node_runs_node_succeeded_idx ON node_runs (node_id)
    WHERE status = 'succeeded';
