-- no-transaction
-- The issues of a cycle with their states. The cycles of a team are listed with how many issues
-- each holds and how many of them are closed (`repo::cycles`, `WHERE i.cycle_id = c.id`), on the
-- issue page and the issues page's filter; with the state in the index the counts read the index
-- alone. It serves the cycle filter and the ON DELETE SET NULL from cycles as well, so
-- 20261025001012 drops `issues_cycle_idx (cycle_id)` once this one is known to be valid. Built
-- concurrently (docs/DATABASE.md, "Migrations").
CREATE INDEX CONCURRENTLY IF NOT EXISTS issues_cycle_state_idx ON issues (cycle_id, state_id)
    WHERE cycle_id IS NOT NULL;
