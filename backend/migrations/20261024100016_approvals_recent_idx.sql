-- no-transaction
-- A workspace's approvals, newest first, whatever their status. The console polls
-- `repo::approvals::list` every 15 s (`WHERE workspace_id = $1 AND ($2::text IS NULL OR status =
-- $2) ORDER BY requested_at DESC, id DESC LIMIT $3`). Without a status `approvals_workspace_idx
-- (workspace_id, status, requested_at DESC)` gives no order, so every approval of the workspace
-- was read and sorted to return the first page. This index returns the page in order, and with a
-- status the scan stops once the page is full; the status-led index stays for the pending ones.
-- Built concurrently, as 20261024100015 is (docs/DATABASE.md, "Migrations").
CREATE INDEX CONCURRENTLY IF NOT EXISTS approvals_recent_idx
    ON approvals (workspace_id, requested_at DESC, id DESC);
