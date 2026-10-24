-- One index over a run's commands, in the order the cancel watcher takes them
-- (`repo::run_commands::take_pending`, `WHERE c.run_id = $1 AND c.taken_at IS NULL ORDER BY
-- c.created_at`, every second per running run). It also serves the ON DELETE CASCADE from runs (a
-- graph's runs purged), so the two indexes it replaces go: `run_commands_pending_idx (run_id) WHERE
-- taken_at IS NULL`, which the cascade could not use, and `run_commands_run_id_idx (run_id)`
-- (20261024000400), whose one column leads this one. A run has a handful of commands at most.
SET LOCAL lock_timeout = '5s';

CREATE INDEX IF NOT EXISTS run_commands_run_idx ON run_commands (run_id, created_at);
DROP INDEX IF EXISTS run_commands_pending_idx;
DROP INDEX IF EXISTS run_commands_run_id_idx;
