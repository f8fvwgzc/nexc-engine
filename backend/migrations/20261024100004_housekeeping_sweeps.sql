-- The background sweeps every replica runs, each of which read its whole table to find the few
-- rows it acts on (app::workers):
--   * every minute, `repo::tokens::purge_expired`: `DELETE FROM refresh_tokens WHERE expires_at <
--     now() - interval '1 day'` (a row per token refresh);
--   * every minute, `repo::plans::fail_stale`: `WHERE status = 'streaming' AND updated_at < now() -
--     interval '15 minutes'` (only plans a crashed process left streaming match, every plan was
--     read);
--   * every 15 minutes, `repo::issues::remind_due`: `WHERE i.assignee_id IS NOT NULL AND
--     i.due_date <= current_date AND i.due_notified_on IS DISTINCT FROM i.due_date`. The index
--     holds only issues with an assignee whose due date was not reminded of yet; its predicate is
--     spelled as the query is, so the planner matches it.
-- Plain builds: these tables grow with what people do. A lock taken here is held until the file
-- commits, so issues, which a finishing run writes under its lease (engine::issue_report), comes
-- last, and its lock is held only for its own build.
SET LOCAL lock_timeout = '5s';

CREATE INDEX IF NOT EXISTS refresh_tokens_expiry_idx ON refresh_tokens (expires_at);
CREATE INDEX IF NOT EXISTS plans_streaming_idx ON plans (updated_at) WHERE status = 'streaming';
CREATE INDEX IF NOT EXISTS issues_due_reminder_idx ON issues (due_date)
    WHERE assignee_id IS NOT NULL AND due_notified_on IS DISTINCT FROM due_date;
