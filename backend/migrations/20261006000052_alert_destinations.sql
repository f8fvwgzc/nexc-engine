-- Alert routing (AgentSecOps, the Defend function reaching out): where a
-- workspace's alerts and approvals are sent. The URL is kept encrypted (a
-- Slack incoming-webhook URL carries its token); the host alone is in the
-- clear for the console. Every delivery is signed with the destination's
-- secret, generated once, shown once, kept encrypted.
CREATE TABLE alert_destinations (
    id                uuid PRIMARY KEY,
    workspace_id      uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name              text NOT NULL,
    -- `webhook`: a signed JSON POST; `slack`: a Slack incoming webhook (Block Kit).
    kind              text NOT NULL CHECK (kind IN ('webhook', 'slack')),
    -- AES-256-GCM under the master key, bound to the destination's id.
    url               bytea NOT NULL,
    host              text NOT NULL,
    -- The HMAC-SHA256 key of `X-Nexc-Signature`, AES-256-GCM under the master key.
    secret            bytea NOT NULL,
    -- Alert kinds sent; empty means every kind.
    kinds             jsonb NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(kinds) = 'array'),
    min_severity      text NOT NULL DEFAULT 'low'
                      CHECK (min_severity IN ('low', 'medium', 'high', 'critical')),
    include_approvals boolean NOT NULL DEFAULT false,
    enabled           boolean NOT NULL DEFAULT true,
    -- Why the platform switched it off, when it did (50 deliveries in a row failed).
    disabled_reason   text NOT NULL DEFAULT '',
    -- Deliveries that failed in a row; reset by a delivery that went through.
    failures          integer NOT NULL DEFAULT 0 CHECK (failures >= 0),
    last_delivered_at timestamptz,
    last_error        text NOT NULL DEFAULT '',
    created_by        uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, name)
);
