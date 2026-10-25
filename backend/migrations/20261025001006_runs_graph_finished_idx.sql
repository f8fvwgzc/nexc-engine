-- no-transaction
-- A graph's runs by when they ended (or were queued, while they run). The timeline places a run
-- at `COALESCE(r.finished_at, r.created_at)` and reads a window of it per graph of the workspace
-- (`repo::insight`), which `runs_graph_idx (graph_id, created_at DESC)` cannot bound. Built
-- concurrently (docs/DATABASE.md, "Migrations"): runs change state all the time.
CREATE INDEX CONCURRENTLY IF NOT EXISTS runs_graph_finished_idx ON runs
    (graph_id, (COALESCE(finished_at, created_at)));
