-- The projection into onto-intelli is told about a row only when what it projects changed.
-- `UPDATE OF col, ...` fires whenever a statement names one of the columns, changed or not, and
-- most updates name them all (an issue saved with a new description sets its title again), so
-- every such save queued an export, re-sent the entity to onto-intelli and dropped its read
-- cache for nothing. A trigger that fires on INSERT or DELETE as well cannot look at OLD and NEW
-- in its WHEN, so each one is split: the insert and the delete as before, the update only when
-- a projected column is distinct from what it was (docs/adr/0001-http3-edge-caching-and-indexes.md).
-- The functions are the same; users_onto_project and runs_onto_export were already guarded.
SET LOCAL lock_timeout = '5s';

DROP TRIGGER graphs_onto_project ON graphs;
CREATE TRIGGER graphs_onto_project
    AFTER INSERT OR DELETE ON graphs
    FOR EACH ROW EXECUTE FUNCTION onto_project_row();
CREATE TRIGGER graphs_onto_project_update
    AFTER UPDATE OF name, team_id, workspace_id, deleted_at ON graphs
    FOR EACH ROW
    WHEN ((OLD.name, OLD.team_id, OLD.workspace_id, OLD.deleted_at)
          IS DISTINCT FROM (NEW.name, NEW.team_id, NEW.workspace_id, NEW.deleted_at))
    EXECUTE FUNCTION onto_project_row();

DROP TRIGGER agents_onto_export ON agents;
CREATE TRIGGER agents_onto_export
    AFTER INSERT ON agents
    FOR EACH ROW EXECUTE FUNCTION onto_export_enqueue_agent();
CREATE TRIGGER agents_onto_export_update
    AFTER UPDATE OF name, role, model, status, capabilities, policy_version, workspace_id ON agents
    FOR EACH ROW
    WHEN ((OLD.name, OLD.role, OLD.model, OLD.status, OLD.capabilities, OLD.policy_version,
           OLD.workspace_id)
          IS DISTINCT FROM (NEW.name, NEW.role, NEW.model, NEW.status, NEW.capabilities,
                            NEW.policy_version, NEW.workspace_id))
    EXECUTE FUNCTION onto_export_enqueue_agent();

DROP TRIGGER workspace_members_onto_project ON workspace_members;
CREATE TRIGGER workspace_members_onto_project
    AFTER INSERT OR DELETE ON workspace_members
    FOR EACH ROW EXECUTE FUNCTION onto_project_row();
CREATE TRIGGER workspace_members_onto_project_update
    AFTER UPDATE OF role ON workspace_members
    FOR EACH ROW WHEN (OLD.role IS DISTINCT FROM NEW.role)
    EXECUTE FUNCTION onto_project_row();

DROP TRIGGER teams_onto_project ON teams;
CREATE TRIGGER teams_onto_project
    AFTER INSERT OR DELETE ON teams
    FOR EACH ROW EXECUTE FUNCTION onto_project_row();
CREATE TRIGGER teams_onto_project_update
    AFTER UPDATE OF name, key, private ON teams
    FOR EACH ROW
    WHEN ((OLD.name, OLD.key, OLD.private) IS DISTINCT FROM (NEW.name, NEW.key, NEW.private))
    EXECUTE FUNCTION onto_project_row();

DROP TRIGGER projects_onto_project ON projects;
CREATE TRIGGER projects_onto_project
    AFTER INSERT OR DELETE ON projects
    FOR EACH ROW EXECUTE FUNCTION onto_project_row();
CREATE TRIGGER projects_onto_project_update
    AFTER UPDATE OF name, status, lead_id, team_id ON projects
    FOR EACH ROW
    WHEN ((OLD.name, OLD.status, OLD.lead_id, OLD.team_id)
          IS DISTINCT FROM (NEW.name, NEW.status, NEW.lead_id, NEW.team_id))
    EXECUTE FUNCTION onto_project_row();

DROP TRIGGER issues_onto_project ON issues;
CREATE TRIGGER issues_onto_project
    AFTER INSERT OR DELETE ON issues
    FOR EACH ROW EXECUTE FUNCTION onto_project_row();
CREATE TRIGGER issues_onto_project_update
    AFTER UPDATE OF title, number, team_id, state_id, priority, assignee_id, agent_id, project_id,
                    graph_id, cycle_id, parent_id ON issues
    FOR EACH ROW
    WHEN ((OLD.title, OLD.number, OLD.team_id, OLD.state_id, OLD.priority, OLD.assignee_id,
           OLD.agent_id, OLD.project_id, OLD.graph_id, OLD.cycle_id, OLD.parent_id)
          IS DISTINCT FROM (NEW.title, NEW.number, NEW.team_id, NEW.state_id, NEW.priority,
                            NEW.assignee_id, NEW.agent_id, NEW.project_id, NEW.graph_id,
                            NEW.cycle_id, NEW.parent_id))
    EXECUTE FUNCTION onto_project_row();

DROP TRIGGER cycles_onto_project ON cycles;
CREATE TRIGGER cycles_onto_project
    AFTER INSERT OR DELETE ON cycles
    FOR EACH ROW EXECUTE FUNCTION onto_project_row();
CREATE TRIGGER cycles_onto_project_update
    AFTER UPDATE OF name, number, starts_on, ends_on ON cycles
    FOR EACH ROW
    WHEN ((OLD.name, OLD.number, OLD.starts_on, OLD.ends_on)
          IS DISTINCT FROM (NEW.name, NEW.number, NEW.starts_on, NEW.ends_on))
    EXECUTE FUNCTION onto_project_row();

DROP TRIGGER documents_onto_project ON documents;
CREATE TRIGGER documents_onto_project
    AFTER INSERT OR DELETE ON documents
    FOR EACH ROW EXECUTE FUNCTION onto_project_row();
CREATE TRIGGER documents_onto_project_update
    AFTER UPDATE OF name, status, label ON documents
    FOR EACH ROW
    WHEN ((OLD.name, OLD.status, OLD.label) IS DISTINCT FROM (NEW.name, NEW.status, NEW.label))
    EXECUTE FUNCTION onto_project_row();
