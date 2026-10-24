-- no-transaction
-- `memories_graph_id_idx (graph_id) WHERE graph_id IS NOT NULL` (20261024000400) repeats the
-- first column of `memories_graph_idx (graph_id, updated_at DESC)` over the same rows
-- (20261024100009), which serves the ON DELETE CASCADE from graphs as well. It goes only now, once
-- 20261024100018 has found the wider index valid, so the foreign key never lacks one. Dropped
-- concurrently (docs/DATABASE.md, "Migrations"): memories are written after every node.
DROP INDEX CONCURRENTLY IF EXISTS memories_graph_id_idx;
