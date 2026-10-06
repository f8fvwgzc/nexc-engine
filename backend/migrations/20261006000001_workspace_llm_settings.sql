-- A workspace can hold one LLM credential that its admins manage. It is used
-- for a member's work when that member has not connected an account of their own.
CREATE TABLE workspace_llm_settings (
    workspace_id  uuid PRIMARY KEY REFERENCES workspaces (id) ON DELETE CASCADE,
    provider      text NOT NULL CHECK (provider IN ('anthropic', 'openai_compatible', 'demo', 'claude_code')),
    model         text NOT NULL,
    base_url      text,
    api_key_enc   bytea,
    key_hint      text,
    updated_by    uuid REFERENCES users (id) ON DELETE SET NULL,
    updated_at    timestamptz NOT NULL DEFAULT now()
);
