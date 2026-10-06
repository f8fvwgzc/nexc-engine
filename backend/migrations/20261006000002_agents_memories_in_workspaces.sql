-- Agents and memories belong to a workspace. `owner_id` stays as the creator
-- (agents) or the author (memories). Existing rows are assigned at startup.
ALTER TABLE agents ADD COLUMN workspace_id uuid REFERENCES workspaces (id) ON DELETE CASCADE;
ALTER TABLE agents DROP CONSTRAINT agents_owner_id_name_key;
CREATE UNIQUE INDEX agents_workspace_name_key ON agents (workspace_id, name)
    WHERE workspace_id IS NOT NULL;
CREATE INDEX agents_workspace_role_idx ON agents (workspace_id, role);

ALTER TABLE memories ADD COLUMN workspace_id uuid REFERENCES workspaces (id) ON DELETE CASCADE;
CREATE INDEX memories_workspace_idx ON memories (workspace_id, updated_at DESC);
