-- no-transaction
-- The last sealed run on the AgentSecOps console. `repo::secops::last_seal` (`WHERE
-- r.trace_hash IS NOT NULL ... ORDER BY r.finished_at DESC NULLS LAST, r.id DESC LIMIT 1`)
-- sorted every sealed run of the workspace to keep one. In this order the newest seals come
-- first and the scan stops at the first one the reader may open. `trace_hash` and `finished_at`
-- are written once, when the run ends, so lease renewals stay HOT. Built concurrently
-- (docs/DATABASE.md, "Migrations").
CREATE INDEX CONCURRENTLY IF NOT EXISTS runs_sealed_idx
    ON runs (finished_at DESC NULLS LAST, id DESC) WHERE trace_hash IS NOT NULL;
