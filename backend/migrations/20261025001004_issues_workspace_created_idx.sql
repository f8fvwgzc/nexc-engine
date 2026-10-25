-- no-transaction
-- Issues filed in a time window. The workspace timeline (`repo::insight`, `i.workspace_id = $1
-- AND i.created_at >= $2 AND i.created_at < $3`, also for its days and its count) had only
-- `issues_workspace_idx (workspace_id, updated_at DESC)`, so a day of it read every issue of the
-- workspace. Built concurrently (docs/DATABASE.md, "Migrations").
CREATE INDEX CONCURRENTLY IF NOT EXISTS issues_workspace_created_idx ON issues (workspace_id, created_at);
