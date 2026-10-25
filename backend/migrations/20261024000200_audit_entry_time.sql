-- Each audit entry records when it was written, not when its transaction began: a change that
-- writes many entries at once (the expert catalog seed writes about 65) gives them distinct times,
-- so that paging the log by time (`before=`) does not skip the ones that shared an instant.
ALTER TABLE audit_log ALTER COLUMN created_at SET DEFAULT clock_timestamp();
