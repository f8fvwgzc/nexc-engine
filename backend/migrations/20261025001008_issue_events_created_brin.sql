-- no-transaction
-- Issue timeline entries in a time window: the workspace timeline (`repo::insight`,
-- `e.created_at >= $2 AND e.created_at < $3`, for its days and its count too) had nothing to
-- bound the window with, and read the events of every issue of the workspace. Entries arrive in
-- the order they are made and `created_at` never changes, so a BRIN index (a min and max per 128
-- pages) finds the window's pages for a few kilobytes and almost no cost per insert, where a
-- btree would cost every comment and change an entry. An edited comment can land in another
-- page and widen its range a little; the window is always rechecked. Ranges are summarised by
-- autovacuum as they fill (`autosummarize`). Built concurrently (docs/DATABASE.md,
-- "Migrations").
CREATE INDEX CONCURRENTLY IF NOT EXISTS issue_events_created_brin ON issue_events
    USING brin (created_at) WITH (autosummarize = on);
