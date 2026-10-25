-- no-transaction
-- `issues_parent_idx (parent_id) WHERE parent_id IS NOT NULL` (20261006000011) is the first
-- column of `issues_parent_state_idx (parent_id, state_id)` over the same rows (20261025001001),
-- which serves the ON DELETE SET NULL from issues as well. It goes only now, once
-- 20261025001009 has found the wider index valid, so the foreign key never lacks one. Dropped
-- concurrently (docs/DATABASE.md, "Migrations"): issues are written all day.
DROP INDEX CONCURRENTLY IF EXISTS issues_parent_idx;
