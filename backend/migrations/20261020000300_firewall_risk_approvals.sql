-- The Agent Firewall scored and held (AgentSecOps, 2026-10-20). Additive
-- only: replicas still on the previous version keep working during a
-- rolling deploy.
SET LOCAL lock_timeout = '5s';

-- The decision sessions of an external agent (windows of firewall
-- decisions, not runs) have a baseline of their own, never compared with
-- its runs' (`agent_baselines`).
CREATE TABLE agent_session_baselines (
    agent_id    uuid PRIMARY KEY REFERENCES agents (id) ON DELETE CASCADE,
    runs        integer NOT NULL DEFAULT 0 CHECK (runs >= 0),
    features    jsonb NOT NULL DEFAULT '{}'::jsonb,
    signatures  jsonb NOT NULL DEFAULT '[]'::jsonb,
    updated_at  timestamptz NOT NULL DEFAULT now()
);

-- An approval an external agent asked for through the firewall lets one
-- reported call go ahead: the `tool_call` span that used it.
ALTER TABLE approvals ADD COLUMN consumed_span_id uuid;

-- Pending approvals past their time are swept (the firewall's, whose
-- watching instance may be gone).
CREATE INDEX approvals_pending_expiry_idx ON approvals (expires_at) WHERE status = 'pending';
