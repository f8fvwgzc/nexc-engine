-- no-transaction
-- `issues_project_idx (project_id) WHERE project_id IS NOT NULL` (20261006000005) is the first
-- column of `issues_project_state_idx (project_id, state_id, team_id)` over the same rows
-- (20261025001002), which serves the ON DELETE SET NULL from projects as well. It goes only now,
-- once 20261025001009 has found the wider index valid. Dropped concurrently (docs/DATABASE.md,
-- "Migrations").
DROP INDEX CONCURRENTLY IF EXISTS issues_project_idx;
