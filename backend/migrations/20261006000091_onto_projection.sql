-- The workspace projected into onto-intelli (docs/ONTOLOGY.md): teams, projects, cycles,
-- issues, agents, graphs, documents, people and runs become entities and links there (ids,
-- names, states and links, never content), next to the audit's facts. onto-intelli's UI
-- renders that map; nexc's own SQL map is gone. What changes goes through the export outbox
-- (onto_export_queue), filled by the tables themselves as before; deletions are queued too
-- and resolved when the row is gone.

ALTER TABLE onto_export_queue DROP CONSTRAINT onto_export_queue_kind_check;
ALTER TABLE onto_export_queue ADD CONSTRAINT onto_export_queue_kind_check CHECK (kind IN (
    'span', 'run', 'agent', 'team', 'project', 'cycle', 'issue', 'graph', 'document', 'person',
    'run_started'));

-- Queues `kind` for `subject` in `workspace`, when the row belongs to one.
CREATE FUNCTION onto_project(workspace uuid, kind text, subject uuid) RETURNS void
LANGUAGE sql AS $$
    INSERT INTO onto_export_queue (workspace_id, kind, subject_id)
    SELECT workspace, kind, subject WHERE workspace IS NOT NULL AND subject IS NOT NULL;
$$;

CREATE FUNCTION onto_project_row() RETURNS trigger
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
        WHEN 'teams' THEN PERFORM onto_project(item.workspace_id, 'team', item.id);
        WHEN 'projects' THEN PERFORM onto_project(item.workspace_id, 'project', item.id);
        WHEN 'issues' THEN PERFORM onto_project(item.workspace_id, 'issue', item.id);
        WHEN 'graphs' THEN PERFORM onto_project(item.workspace_id, 'graph', item.id);
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

CREATE TRIGGER teams_onto_project
    AFTER INSERT OR DELETE OR UPDATE OF name, key, private ON teams
    FOR EACH ROW EXECUTE FUNCTION onto_project_row();
CREATE TRIGGER projects_onto_project
    AFTER INSERT OR DELETE OR UPDATE OF name, status, lead_id ON projects
    FOR EACH ROW EXECUTE FUNCTION onto_project_row();
CREATE TRIGGER cycles_onto_project
    AFTER INSERT OR DELETE OR UPDATE OF name, number, starts_on, ends_on ON cycles
    FOR EACH ROW EXECUTE FUNCTION onto_project_row();
CREATE TRIGGER issues_onto_project
    AFTER INSERT OR DELETE OR UPDATE OF title, number, team_id, state_id, priority, assignee_id,
        agent_id, project_id, graph_id, cycle_id, parent_id ON issues
    FOR EACH ROW EXECUTE FUNCTION onto_project_row();
CREATE TRIGGER graphs_onto_project
    AFTER INSERT OR DELETE OR UPDATE OF name, team_id, workspace_id ON graphs
    FOR EACH ROW EXECUTE FUNCTION onto_project_row();
CREATE TRIGGER documents_onto_project
    AFTER INSERT OR DELETE OR UPDATE OF name, status, label ON documents
    FOR EACH ROW EXECUTE FUNCTION onto_project_row();
CREATE TRIGGER workspace_members_onto_project
    AFTER INSERT OR DELETE OR UPDATE OF role ON workspace_members
    FOR EACH ROW EXECUTE FUNCTION onto_project_row();
CREATE TRIGGER team_members_onto_project
    AFTER INSERT OR DELETE ON team_members
    FOR EACH ROW EXECUTE FUNCTION onto_project_row();

-- A renamed person is projected again in every workspace they are in.
CREATE FUNCTION onto_project_user() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM onto_project(m.workspace_id, 'person', NEW.id)
       FROM workspace_members m WHERE m.user_id = NEW.id;
    RETURN NULL;
END $$;
CREATE TRIGGER users_onto_project
    AFTER UPDATE OF name ON users
    FOR EACH ROW WHEN (OLD.name IS DISTINCT FROM NEW.name)
    EXECUTE FUNCTION onto_project_user();

-- A run is on the map from the moment it is created, linked to its graph and to whoever
-- started it (its finish is queued as before, as 'run').
CREATE FUNCTION onto_project_run() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM onto_project(g.workspace_id, 'run_started', NEW.id)
       FROM graphs g WHERE g.id = NEW.graph_id;
    RETURN NULL;
END $$;
CREATE TRIGGER runs_onto_project
    AFTER INSERT ON runs
    FOR EACH ROW EXECUTE FUNCTION onto_project_run();

-- What the projection put into onto-intelli, per entity: the links it stated from that
-- entity (`rel to`), so a link that no longer holds is taken back and an entity is deleted
-- there only once it was put there.
CREATE TABLE onto_projection (
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    entity        text NOT NULL,
    links         text[] NOT NULL DEFAULT '{}',
    updated_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, entity)
);

-- Set once a workspace was projected whole; until then the export backfills it.
ALTER TABLE onto_export_cursors ADD COLUMN projected_at timestamptz;

-- Each workspace now has a tenant of its own in onto-intelli (`ws_{id}`), so the audit's facts
-- are backfilled into it once more.
UPDATE onto_export_cursors
   SET span_seq = 0, backfill_before = NULL, backfilled_at = NULL, updated_at = now();
