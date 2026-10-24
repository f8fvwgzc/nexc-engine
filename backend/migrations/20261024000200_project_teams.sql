-- A project can be owned by a team (Linear's model: one team, many projects). Projects without
-- a team stay the workspace's. Deleting the team keeps its projects, without a team. A project
-- of a private team is seen only by those who see the team.
SET LOCAL lock_timeout = '5s';

ALTER TABLE projects ADD COLUMN team_id uuid REFERENCES teams (id) ON DELETE SET NULL;
CREATE INDEX projects_team_idx ON projects (team_id) WHERE team_id IS NOT NULL;

-- The workspace projection links a project to its team, so a new team re-projects it.
DROP TRIGGER projects_onto_project ON projects;
CREATE TRIGGER projects_onto_project
    AFTER INSERT OR DELETE OR UPDATE OF name, status, lead_id, team_id ON projects
    FOR EACH ROW EXECUTE FUNCTION onto_project_row();

-- As 20261006000102, and a team made private or public restricts, or opens up, its projects
-- as well.
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
                SELECT NEW.workspace_id, 'project', p.id FROM projects p WHERE p.team_id = NEW.id
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
