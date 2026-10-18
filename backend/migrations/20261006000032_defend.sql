-- Defend (AgentSecOps phase 2): what the DLP classifier, the behaviour
-- baseline and the prompt-injection screen find.

-- An artifact the DLP classifier stopped on the way out: stored, but not
-- downloadable until an admin releases it.
ALTER TABLE artifacts
    ADD COLUMN quarantined_at    timestamptz,
    ADD COLUMN quarantine_reason text NOT NULL DEFAULT '',
    ADD COLUMN released_at       timestamptz,
    ADD COLUMN released_by       uuid REFERENCES users (id) ON DELETE SET NULL;

-- Something about an agent that needs a look: a finding on the way out, a
-- risk score over its threshold, an injection in a tool result. Alerts reach
-- the admins' inbox and the graph's event stream (`alert`).
CREATE TABLE alerts (
    id              uuid PRIMARY KEY,
    workspace_id    uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    agent_id        uuid,
    principal       text NOT NULL,
    run_id          uuid REFERENCES runs (id) ON DELETE CASCADE,
    node_id         uuid,
    graph_id        uuid,
    -- The policy span that recorded the finding, if any.
    span_id         uuid,
    kind            text NOT NULL CHECK (kind IN ('dlp', 'risk', 'injection', 'policy')),
    severity        text NOT NULL CHECK (severity IN ('low', 'medium', 'high', 'critical')),
    -- The rule that raised it (`dlp.secrets`, `alerts.risk_score`, `injection`, ...).
    rule            text NOT NULL,
    title           text NOT NULL,
    detail          text NOT NULL DEFAULT '',
    created_at      timestamptz NOT NULL DEFAULT now(),
    acknowledged_at timestamptz,
    acknowledged_by uuid REFERENCES users (id) ON DELETE SET NULL
);
CREATE INDEX alerts_workspace_idx ON alerts (workspace_id, created_at DESC, id DESC);
CREATE INDEX alerts_agent_idx ON alerts (agent_id, created_at DESC);

ALTER TABLE notifications
    ADD COLUMN alert_id uuid REFERENCES alerts (id) ON DELETE CASCADE;
CREATE INDEX notifications_alert_idx ON notifications (alert_id) WHERE alert_id IS NOT NULL;
