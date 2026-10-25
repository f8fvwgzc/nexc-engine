-- A reply answers a top-level comment of the same issue: `parent_id` names it, and deleting the
-- comment deletes its replies. The handler checks the rest (a comment, of the same issue, not a
-- reply itself).
--
-- A finished run writes its report into issue_events inside its lease, so the key comes back NOT
-- VALID, like 20261025000101's check, and 20261025000205 checks the rows already there (all of
-- them without a parent). It is DEFERRABLE, INITIALLY IMMEDIATE: the server's own writes are
-- checked at once, as with any other key, while a workspace transfer, which copies issue_events
-- by id in batches, defers it to its commit, so a reply whose id sorts before its comment's
-- (another server's clock) still arrives (engine::transfer). Its index is built concurrently by
-- 20261025000202.
SET LOCAL lock_timeout = '5s';

ALTER TABLE issue_events ADD COLUMN IF NOT EXISTS parent_id uuid;
ALTER TABLE issue_events DROP CONSTRAINT IF EXISTS issue_events_parent_id_fkey;
ALTER TABLE issue_events ADD CONSTRAINT issue_events_parent_id_fkey
    FOREIGN KEY (parent_id) REFERENCES issue_events (id) ON DELETE CASCADE
    DEFERRABLE INITIALLY IMMEDIATE NOT VALID;
