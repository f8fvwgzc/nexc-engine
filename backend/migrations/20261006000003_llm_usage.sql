-- One row per LLM call that spent tokens: who caused it, where, and whose
-- account paid. Reports aggregate this table; rows are never updated.
CREATE TABLE llm_usage (
    id            uuid PRIMARY KEY,
    workspace_id  uuid REFERENCES workspaces (id) ON DELETE CASCADE,
    user_id       uuid REFERENCES users (id) ON DELETE SET NULL,
    graph_id      uuid REFERENCES graphs (id) ON DELETE SET NULL,
    run_id        uuid REFERENCES runs (id) ON DELETE SET NULL,
    purpose       text NOT NULL CHECK (purpose IN ('plan', 'node', 'memory')),
    provider      text NOT NULL,
    model         text NOT NULL,
    -- Whose configuration supplied the credential (see ConfigScope).
    credential    text NOT NULL CHECK (credential IN ('user', 'workspace', 'server')),
    tokens_in     bigint NOT NULL CHECK (tokens_in >= 0),
    tokens_out    bigint NOT NULL CHECK (tokens_out >= 0),
    cost_usd      double precision NOT NULL DEFAULT 0,
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX llm_usage_workspace_idx ON llm_usage (workspace_id, created_at DESC);
CREATE INDEX llm_usage_user_idx ON llm_usage (user_id, created_at DESC);
