-- The workspace projection (docs/ONTOLOGY.md §5) follows what changes indirectly:
--
-- * a team made private or public restricts, or opens up, its issues, cycles and graphs and
--   the runs of those graphs, not only the team itself; a graph moved to another team does
--   the same for its runs;
-- * rows removed with their parent are queued while the parent is still there to say which
--   workspace they were in: a deleted team's cycles and members, a deleted graph's runs (the
--   graph itself takes its vocabulary along, see ontology::projection);
-- * a deleted workspace's tenant is retired: what the projection put there and the graphs
--   whose vocabularies live there are kept in onto_retired_tenants, the export worker deletes
--   them from onto-intelli, and the operator drops the tenant's schema (onto-intelli has no
--   call for that).

CREATE OR REPLACE FUNCTION onto_project_row() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    item record;
BEGIN
    IF TG_OP = 'DELETE' THEN
        item := OLD;
    ELSE
        item := NEW;
    END IF;
    CASE TG_TABLE_NAME
        WHEN 'teams' THEN
            PERFORM onto_project(item.workspace_id, 'team', item.id);
            IF TG_OP = 'UPDATE' AND OLD.private IS DISTINCT FROM NEW.private THEN
                INSERT INTO onto_export_queue (workspace_id, kind, subject_id)
                SELECT NEW.workspace_id, 'issue', i.id FROM issues i WHERE i.team_id = NEW.id
                UNION ALL
                SELECT NEW.workspace_id, 'cycle', c.id FROM cycles c WHERE c.team_id = NEW.id
                UNION ALL
                SELECT NEW.workspace_id, 'graph', g.id FROM graphs g WHERE g.team_id = NEW.id
                UNION ALL
                SELECT NEW.workspace_id, 'run_started', r.id
                  FROM runs r JOIN graphs g ON g.id = r.graph_id WHERE g.team_id = NEW.id;
            END IF;
        WHEN 'projects' THEN PERFORM onto_project(item.workspace_id, 'project', item.id);
        WHEN 'issues' THEN PERFORM onto_project(item.workspace_id, 'issue', item.id);
        WHEN 'graphs' THEN
            PERFORM onto_project(item.workspace_id, 'graph', item.id);
            IF TG_OP = 'UPDATE' AND OLD.team_id IS DISTINCT FROM NEW.team_id
               AND NEW.workspace_id IS NOT NULL THEN
                INSERT INTO onto_export_queue (workspace_id, kind, subject_id)
                SELECT NEW.workspace_id, 'run_started', r.id FROM runs r WHERE r.graph_id = NEW.id;
            END IF;
        WHEN 'documents' THEN PERFORM onto_project(item.workspace_id, 'document', item.id);
        WHEN 'cycles' THEN
            PERFORM onto_project(t.workspace_id, 'cycle', item.id)
               FROM teams t WHERE t.id = item.team_id;
        WHEN 'workspace_members' THEN PERFORM onto_project(item.workspace_id, 'person', item.user_id);
        WHEN 'team_members' THEN
            PERFORM onto_project(t.workspace_id, 'person', item.user_id)
               FROM teams t WHERE t.id = item.team_id;
    END CASE;
    RETURN NULL;
END $$;

-- What goes with a team or a graph, queued while the row still says where it was: the
-- projection then finds it gone and deletes it in onto-intelli (cycles, runs), or states the
-- links that still hold (people).
CREATE FUNCTION onto_project_before_delete() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    CASE TG_TABLE_NAME
        WHEN 'teams' THEN
            INSERT INTO onto_export_queue (workspace_id, kind, subject_id)
            SELECT OLD.workspace_id, 'cycle', c.id FROM cycles c WHERE c.team_id = OLD.id
            UNION ALL
            SELECT OLD.workspace_id, 'person', m.user_id FROM team_members m WHERE m.team_id = OLD.id;
        WHEN 'graphs' THEN
            IF OLD.workspace_id IS NOT NULL THEN
                INSERT INTO onto_export_queue (workspace_id, kind, subject_id)
                SELECT OLD.workspace_id, 'run_started', r.id FROM runs r WHERE r.graph_id = OLD.id;
            END IF;
    END CASE;
    RETURN OLD;
END $$;
CREATE TRIGGER teams_onto_project_before_delete
    BEFORE DELETE ON teams
    FOR EACH ROW EXECUTE FUNCTION onto_project_before_delete();
CREATE TRIGGER graphs_onto_project_before_delete
    BEFORE DELETE ON graphs
    FOR EACH ROW EXECUTE FUNCTION onto_project_before_delete();

-- A run removed on its own (its graph stays) goes from the map too.
CREATE FUNCTION onto_project_run_gone() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM onto_project(g.workspace_id, 'run_started', OLD.id)
       FROM graphs g WHERE g.id = OLD.graph_id;
    RETURN NULL;
END $$;
CREATE TRIGGER runs_onto_project_gone
    AFTER DELETE ON runs
    FOR EACH ROW EXECUTE FUNCTION onto_project_run_gone();

-- A deleted workspace's tenant in onto-intelli (`ws_<id>`), to clear: the entities the
-- projection put there (`entities`, as onto_projection had them) and the graphs whose
-- vocabularies live there (`graphs`). The export worker deletes them and sets `cleared_at`;
-- what remains (the audit's facts and events, the tenant's schema and proposals) goes when the
-- operator drops the schema `onto_ws_<id>`. No foreign key: the workspace is gone.
CREATE TABLE onto_retired_tenants (
    workspace_id  uuid PRIMARY KEY,
    entities      text[] NOT NULL DEFAULT '{}',
    graphs        uuid[] NOT NULL DEFAULT '{}',
    retired_at    timestamptz NOT NULL DEFAULT now(),
    cleared_at    timestamptz,
    last_error    text
);

CREATE FUNCTION onto_retire_tenant() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO onto_retired_tenants (workspace_id, entities, graphs)
    VALUES (
        OLD.id,
        ARRAY(SELECT p.entity FROM onto_projection p WHERE p.workspace_id = OLD.id ORDER BY p.entity),
        ARRAY(SELECT g.id FROM graphs g WHERE g.workspace_id = OLD.id ORDER BY g.id))
    ON CONFLICT (workspace_id) DO NOTHING;
    RETURN OLD;
END $$;
CREATE TRIGGER workspaces_onto_retire
    BEFORE DELETE ON workspaces
    FOR EACH ROW EXECUTE FUNCTION onto_retire_tenant();
