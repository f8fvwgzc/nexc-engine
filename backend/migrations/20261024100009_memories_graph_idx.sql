-- no-transaction
-- A graph's memory, newest first. `repo::memories::of_graph` (`WHERE scope = 'graph' AND
-- graph_id = $1 ORDER BY updated_at DESC LIMIT 1000`) runs every time a node's output is
-- consolidated into its graph's memory (`memory::store`). `memories_graph_id_idx (graph_id)`
-- (20261024000400, for the ON DELETE CASCADE from graphs) finds the graph's memories but leaves
-- every one of them to be sorted; this index returns them in order and serves the cascade as
-- well, so 20261024100019 drops the one-column index once this one is known to be valid. Memories
-- of a person, not of a graph, stay out. Built concurrently (docs/DATABASE.md,
-- "Migrations"): memories grow with every node that learns something.
CREATE INDEX CONCURRENTLY IF NOT EXISTS memories_graph_idx ON memories (graph_id, updated_at DESC)
    WHERE graph_id IS NOT NULL;
