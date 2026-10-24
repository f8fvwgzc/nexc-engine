-- An agent is queued for onto-intelli when anything exported of it changes, its team and its
-- manager included. 20261024000401 split the agents' export trigger in two: `agents_onto_export`
-- for inserts, and `agents_onto_export_update`, which fires only when one of seven columns
-- differs. The export and the projection read three more (`repo::onto_export::agents_by_id`:
-- principal and reports_to; `repo::onto_projection::agents`: team), so a change of team or of
-- manager alone, which an ON DELETE SET NULL makes, was never sent. The update trigger is
-- replaced by one that listens to and compares all ten, under the same name, so an update still
-- queues the agent at most once. The insert trigger stays as it is.
SET LOCAL lock_timeout = '5s';

DROP TRIGGER IF EXISTS agents_onto_export_update ON agents;
CREATE TRIGGER agents_onto_export_update
    AFTER UPDATE OF name, role, model, status, capabilities, policy_version, workspace_id,
        principal_id, reports_to, team_id ON agents
    FOR EACH ROW
    WHEN ((OLD.name, OLD.role, OLD.model, OLD.status, OLD.capabilities, OLD.policy_version,
           OLD.workspace_id, OLD.principal_id, OLD.reports_to, OLD.team_id)
          IS DISTINCT FROM
          (NEW.name, NEW.role, NEW.model, NEW.status, NEW.capabilities, NEW.policy_version,
           NEW.workspace_id, NEW.principal_id, NEW.reports_to, NEW.team_id))
    EXECUTE FUNCTION onto_export_enqueue_agent();
