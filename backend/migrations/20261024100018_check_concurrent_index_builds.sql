-- A concurrent build of 20261024100009 to 20261024100017 that failed, or whose server stopped in
-- the middle of it, left its index behind invalid: maintained by every write, used by no query.
-- The migration's IF NOT EXISTS then skipped it when the server started again (docs/DATABASE.md,
-- "Building indexes"). Such an index is not rebuilt here. A REINDEX in this file's transaction
-- would hold the reads and writes of the tables runs work in (runs, node_runs, spans, usage,
-- memories, approvals, artifacts) until the file commits, on every replica still serving, and a
-- startup probe that cut it short would start it over on the next pod. The migration stops
-- instead, naming each such index with the statement that rebuilds it without holding its table;
-- once they are rebuilt, it passes on the next start. On a database where every build went
-- through, this reads the catalogue and does nothing.
DO $$
DECLARE
    rebuild text;
BEGIN
    SELECT string_agg(format('REINDEX INDEX CONCURRENTLY %s;', i.indexrelid::regclass), ' '
                      ORDER BY c.relname)
      INTO rebuild
      FROM pg_index i
      JOIN pg_class c ON c.oid = i.indexrelid
      JOIN pg_namespace n ON n.oid = c.relnamespace
     WHERE n.nspname = current_schema()
       AND NOT (i.indisvalid AND i.indisready AND i.indislive)
       AND c.relname IN ('memories_graph_idx', 'llm_usage_agent_day_idx',
                         'node_runs_node_succeeded_idx', 'node_runs_evidence_finished_idx',
                         'runs_sealed_idx', 'agent_spans_started_brin',
                         'approvals_pending_agent_idx', 'approvals_recent_idx',
                         'artifacts_quarantined_idx');
    IF rebuild IS NOT NULL THEN
        RAISE EXCEPTION 'a concurrent index build did not finish; run each statement on its own, outside a transaction (psql), then start the server again: %', rebuild;
    END IF;
END $$;
