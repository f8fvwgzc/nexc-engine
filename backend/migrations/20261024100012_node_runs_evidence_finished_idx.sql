-- no-transaction
-- The day's outputs by verdict on the AgentSecOps console. `repo::secops::evidence_counts`
-- (`WHERE nr.evidence IS NOT NULL AND nr.finished_at >= $3`) bounds the outputs by when they
-- finished, but `node_runs_evidence_idx ((evidence->>'verdict'), finished_at)` leads with the
-- verdict, so it was read whole: PostgreSQL 17 does not skip from one leading value to the next.
-- This one starts from the window. Built concurrently (docs/DATABASE.md, "Migrations").
CREATE INDEX CONCURRENTLY IF NOT EXISTS node_runs_evidence_finished_idx ON node_runs (finished_at)
    WHERE evidence IS NOT NULL;
