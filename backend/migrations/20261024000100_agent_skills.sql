-- Skills: the ids of the embedded skill library (backend/skills/*.md) an agent brings to every
-- step, and the role template an agent was made from, so that its competency questions can be
-- asked again (docs/AGENT-CATALOG.md).
ALTER TABLE agents ADD COLUMN skills text[] NOT NULL DEFAULT '{}';
ALTER TABLE agents ADD COLUMN template_id text;
ALTER TABLE agents ADD CONSTRAINT agents_skills_count_check CHECK (cardinality(skills) <= 12);

-- Agents made from a template before this release: their evaluation names the template.
UPDATE agents a SET template_id = e.template
FROM agent_evaluations e
WHERE e.agent_id = a.id AND a.template_id IS NULL;

-- Catalog seeds a workspace has received: each runs once per workspace, so an agent a workspace
-- deletes afterwards is not brought back on the next start.
CREATE TABLE workspace_catalog_seeds (
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    seed text NOT NULL,
    seeded_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, seed)
);
