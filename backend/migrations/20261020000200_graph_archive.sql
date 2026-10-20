-- Deleting a graph archives it (docs/AGENTSECOPS.md §12): the row stays with who deleted it
-- and when, so its runs, their hash-chained spans, their seals and their payloads' metadata stay
-- too, until the retention purge removes the graph for good. Every reader of graphs that serves
-- people skips archived ones (`graph_access!`); the evidence endpoints do not.
ALTER TABLE graphs
    ADD COLUMN deleted_at timestamptz,
    ADD COLUMN deleted_by uuid REFERENCES users (id) ON DELETE SET NULL;
-- The retention purge looks for archived graphs.
CREATE INDEX graphs_deleted_idx ON graphs (deleted_at) WHERE deleted_at IS NOT NULL;

-- An archived graph leaves onto-intelli's map as a deleted one did: the projector finds no
-- live row and removes the entity.
DROP TRIGGER graphs_onto_project ON graphs;
CREATE TRIGGER graphs_onto_project
    AFTER INSERT OR DELETE OR UPDATE OF name, team_id, workspace_id, deleted_at ON graphs
    FOR EACH ROW EXECUTE FUNCTION onto_project_row();

-- Its runs leave the map with it (the projector no longer finds them live); their evidence stays.
CREATE FUNCTION onto_project_archived_runs() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM onto_project(NEW.workspace_id, 'run_started', r.id)
       FROM runs r WHERE r.graph_id = NEW.id;
    RETURN NULL;
END $$;
CREATE TRIGGER graphs_onto_project_archived_runs
    AFTER UPDATE OF deleted_at ON graphs
    FOR EACH ROW WHEN (OLD.deleted_at IS NULL AND NEW.deleted_at IS NOT NULL)
    EXECUTE FUNCTION onto_project_archived_runs();
