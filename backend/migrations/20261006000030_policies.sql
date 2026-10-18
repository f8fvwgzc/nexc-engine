-- Versioned policies (AgentSecOps phase 2): rules as data, one append-only
-- history per scope. Resolution walks workspace -> team -> agent and only
-- narrows; the resolved result is compiled into the capabilities snapshot of
-- the per-run token, and the versions that governed a node are recorded on
-- the run, the node run and every policy span.
CREATE TABLE policies (
    id            uuid PRIMARY KEY,
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    scope         text NOT NULL CHECK (scope IN ('workspace', 'team', 'agent')),
    -- The team or agent the policy is for; NULL for the workspace level.
    scope_id      uuid,
    version       integer NOT NULL CHECK (version >= 1),
    -- `domain::policy_document::PolicyDocument`.
    document      jsonb NOT NULL,
    author_id     uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT policies_scope_id_check CHECK ((scope = 'workspace') = (scope_id IS NULL))
);
-- One version number per scope (the workspace level has no scope id).
CREATE UNIQUE INDEX policies_version_idx
    ON policies (workspace_id, scope, COALESCE(scope_id, '00000000-0000-0000-0000-000000000000'::uuid), version);
CREATE INDEX policies_workspace_idx ON policies (workspace_id, created_at DESC);

-- The policy versions that governed a run (`{"workspace": 3}`) and each of
-- its nodes (`{"workspace": 3, "team": 1, "agent": 2, "capabilities": 4}`).
ALTER TABLE runs ADD COLUMN policy_versions jsonb NOT NULL DEFAULT '{}'::jsonb;
ALTER TABLE node_runs ADD COLUMN policy_versions jsonb NOT NULL DEFAULT '{}'::jsonb;
-- The classifications of the data a node was given (its own context and its
-- upstream nodes'): what it produces is derived from them.
ALTER TABLE node_runs ADD COLUMN context_labels text[] NOT NULL DEFAULT '{}';
