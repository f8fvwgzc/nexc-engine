-- A concurrent build of 20261025001000 to 20261025001008 that failed, or whose server stopped in
-- the middle of it, left its index behind invalid: maintained by every write, used by no query.
-- The migration's IF NOT EXISTS then skipped it when the server started again (docs/DATABASE.md,
-- "Building indexes"). As in 20261024100018, such an index is not rebuilt here, where a REINDEX
-- would hold the issues, notifications, runs, memories and timeline writes of every replica still
-- serving until the file commits: the migration stops, naming each such index with the statement
-- that rebuilds it without holding its table, and passes on the next start once they are
-- rebuilt. The drops after it (20261025001010 to 12) run only past this check, so a foreign key
-- never loses its index to a wider one that is not valid. On a database where every build went
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
       AND c.relname IN ('issues_team_updated_idx', 'issues_parent_state_idx',
                         'issues_project_state_idx', 'issues_cycle_state_idx',
                         'issues_workspace_created_idx', 'memories_workspace_created_idx',
                         'runs_graph_finished_idx', 'notifications_unread_idx',
                         'issue_events_created_brin');
    IF rebuild IS NOT NULL THEN
        RAISE EXCEPTION 'a concurrent index build did not finish; run each statement on its own, outside a transaction (psql), then start the server again: %', rebuild;
    END IF;
END $$;
