-- no-transaction
-- Memories learned in a time window: the timeline's "learned" entries (`repo::insight`,
-- `m.workspace_id = $1 AND m.created_at >= $2 AND m.created_at < $3`), which only had
-- `memories_workspace_idx (workspace_id, updated_at DESC)` to go on. Built concurrently
-- (docs/DATABASE.md, "Migrations"): memories are written after every node.
CREATE INDEX CONCURRENTLY IF NOT EXISTS memories_workspace_created_idx ON memories (workspace_id, created_at);
