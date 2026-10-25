-- no-transaction
-- The replies of a comment, for the cascade when it is deleted (docs/DATABASE.md, "Foreign
-- keys"). Rows that answer nothing (every change, run report and top-level comment) stay out.
-- Built concurrently (docs/DATABASE.md, "Building indexes"): a finished run writes its report
-- into issue_events inside its lease, so a plain build would hold those runs while it reads the
-- timeline of every issue.
CREATE INDEX CONCURRENTLY IF NOT EXISTS issue_events_parent_id_idx ON issue_events (parent_id)
    WHERE parent_id IS NOT NULL;
