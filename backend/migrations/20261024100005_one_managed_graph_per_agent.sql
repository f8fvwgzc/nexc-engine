-- An external agent owns one managed graph, and so does an agent exposed over A2A: every run it
-- reports, every task it receives, is a node of that graph (repo::external_runs, repo::a2a). The
-- graph is created on first use by looking for it and inserting it when it is not there, so two
-- first uses at once could each create one, and the agent's runs would be split between them
-- from then on. These unique indexes hold one live graph per agent; the insert now gives way to
-- the graph created first and reads it back (`ensure_graph`, ON CONFLICT DO NOTHING). Archived
-- graphs are left out: they keep their runs' evidence, whichever agent they were for.
--
-- An installation where the race already happened keeps working as before: an index is created
-- only when no agent has two live managed graphs, and a warning says so otherwise
-- (docs/DATABASE.md, "Invariants").
SET LOCAL lock_timeout = '5s';

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM graphs
                    WHERE external_agent_id IS NOT NULL AND deleted_at IS NULL
                    GROUP BY external_agent_id HAVING count(*) > 1) THEN
        CREATE UNIQUE INDEX IF NOT EXISTS graphs_external_agent_live_key
            ON graphs (external_agent_id) WHERE external_agent_id IS NOT NULL AND deleted_at IS NULL;
    ELSE
        RAISE WARNING 'graphs_external_agent_live_key not created: two live graphs of one agent';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM graphs
                    WHERE a2a_agent_id IS NOT NULL AND deleted_at IS NULL
                    GROUP BY a2a_agent_id HAVING count(*) > 1) THEN
        CREATE UNIQUE INDEX IF NOT EXISTS graphs_a2a_agent_live_key ON graphs (a2a_agent_id)
            WHERE a2a_agent_id IS NOT NULL AND deleted_at IS NULL;
    ELSE
        RAISE WARNING 'graphs_a2a_agent_live_key not created: two live graphs of one agent';
    END IF;
END $$;
