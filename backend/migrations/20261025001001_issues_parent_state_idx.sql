-- no-transaction
-- The sub-issues of an issue with their states. Every issue a list shows counts its sub-issues
-- and how many of them are closed (`issue_summary_for_user!`, `WHERE c.parent_id = i.id`), and
-- the issue page lists them (`parent_id = $10`). With the state in the index the counts read the
-- index alone. It serves the ON DELETE SET NULL from issues as well, so 20261025001010 drops
-- `issues_parent_idx (parent_id)` once this one is known to be valid. Built concurrently
-- (docs/DATABASE.md, "Migrations").
CREATE INDEX CONCURRENTLY IF NOT EXISTS issues_parent_state_idx ON issues (parent_id, state_id)
    WHERE parent_id IS NOT NULL;
