-- The concurrent build of 20261025000207, if it failed or its server stopped in the middle of it,
-- left its index behind invalid, and IF NOT EXISTS then skipped it when the server started again.
-- As 20261025000203 does, the migration stops, naming the statement that rebuilds the index
-- without holding the table; once it is rebuilt, it passes on the next start. Where the build
-- went through, this reads the catalogue and does nothing.
DO $$
DECLARE
    rebuild text;
BEGIN
    SELECT string_agg(format('REINDEX INDEX CONCURRENTLY %s;', i.indexrelid::regclass), ' ')
      INTO rebuild
      FROM pg_index i
      JOIN pg_class c ON c.oid = i.indexrelid
      JOIN pg_namespace n ON n.oid = c.relnamespace
     WHERE n.nspname = current_schema()
       AND NOT (i.indisvalid AND i.indisready AND i.indislive)
       AND c.relname = 'notifications_event_id_idx';
    IF rebuild IS NOT NULL THEN
        RAISE EXCEPTION 'a concurrent index build did not finish; run each statement on its own, outside a transaction (psql), then start the server again: %', rebuild;
    END IF;
END $$;
