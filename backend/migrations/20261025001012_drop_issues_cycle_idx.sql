-- no-transaction
-- `issues_cycle_idx (cycle_id) WHERE cycle_id IS NOT NULL` (20261006000012) is the first column
-- of `issues_cycle_state_idx (cycle_id, state_id)` over the same rows (20261025001003), which
-- serves the ON DELETE SET NULL from cycles as well. It goes only now, once 20261025001009 has
-- found the wider index valid. Dropped concurrently (docs/DATABASE.md, "Migrations").
DROP INDEX CONCURRENTLY IF EXISTS issues_cycle_idx;
