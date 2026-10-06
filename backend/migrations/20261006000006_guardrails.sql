-- Policy a workspace puts on its use of LLMs: budgets, which providers may be
-- used, whether agents may run code, whether secrets are scrubbed from prompts.
CREATE TABLE workspace_guardrails (
    workspace_id  uuid PRIMARY KEY REFERENCES workspaces (id) ON DELETE CASCADE,
    config        jsonb NOT NULL CHECK (jsonb_typeof(config) = 'object'),
    updated_by    uuid REFERENCES users (id) ON DELETE SET NULL,
    updated_at    timestamptz NOT NULL DEFAULT now()
);

-- The workspace assistant spends tokens too.
ALTER TABLE llm_usage DROP CONSTRAINT llm_usage_purpose_check;
ALTER TABLE llm_usage ADD CONSTRAINT llm_usage_purpose_check
    CHECK (purpose IN ('plan', 'node', 'memory', 'assistant'));
