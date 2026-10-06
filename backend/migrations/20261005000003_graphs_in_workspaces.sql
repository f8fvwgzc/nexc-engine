-- Graphs live in a workspace and optionally in one of its teams. `owner_id`
-- stays as the creator. Existing graphs are assigned to their creator's first
-- workspace when the server starts (workspaces are created there, not here).
ALTER TABLE graphs ADD COLUMN workspace_id uuid REFERENCES workspaces (id) ON DELETE CASCADE;
-- A team cannot be deleted while it still holds graphs.
ALTER TABLE graphs ADD COLUMN team_id uuid REFERENCES teams (id) ON DELETE RESTRICT;
CREATE INDEX graphs_workspace_idx ON graphs (workspace_id, updated_at DESC);
CREATE INDEX graphs_team_idx ON graphs (team_id) WHERE team_id IS NOT NULL;
