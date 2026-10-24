-- Spans are queued for onto-intelli once per write instead of once per span. Spans arrive in
-- batches, one INSERT for many (`repo::spans::insert`, `insert_fenced`), and the row trigger of
-- 20261006000040 ran a PL/pgSQL call and a run-to-graph lookup for each span of a batch. A
-- statement trigger sees the whole batch as a transition table and queues it with one INSERT and
-- one join. What is queued does not change: only spans the statement wrote (not those ON
-- CONFLICT skipped), only those of a run whose graph belongs to a workspace, in the order they
-- were written (`ingested_seq`).
SET LOCAL lock_timeout = '5s';

CREATE OR REPLACE FUNCTION onto_export_enqueue_spans() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO onto_export_queue (workspace_id, kind, subject_id)
    SELECT g.workspace_id, 'span', s.id
      FROM written_spans s
      JOIN runs r ON r.id = s.run_id
      JOIN graphs g ON g.id = r.graph_id
     WHERE g.workspace_id IS NOT NULL
     ORDER BY s.ingested_seq;
    RETURN NULL;
END $$;

DROP TRIGGER IF EXISTS agent_spans_onto_export ON agent_spans;
CREATE TRIGGER agent_spans_onto_export
    AFTER INSERT ON agent_spans
    REFERENCING NEW TABLE AS written_spans
    FOR EACH STATEMENT EXECUTE FUNCTION onto_export_enqueue_spans();

-- The row function had no other caller.
DROP FUNCTION IF EXISTS onto_export_enqueue_span();
