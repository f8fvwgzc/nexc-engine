-- The outbox of alert routing: one row per event per destination, filled
-- when an alert is raised or an approval is requested or decided, taken by
-- a worker with SKIP LOCKED, retried with exponential backoff for a day,
-- and kept as the delivery log the console shows.
CREATE TABLE alert_deliveries (
    id              uuid PRIMARY KEY,
    destination_id  uuid NOT NULL REFERENCES alert_destinations (id) ON DELETE CASCADE,
    workspace_id    uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    event           text NOT NULL
                    CHECK (event IN ('alert.raised', 'approval.requested', 'approval.decided', 'test')),
    alert_id        uuid REFERENCES alerts (id) ON DELETE CASCADE,
    approval_id     uuid REFERENCES approvals (id) ON DELETE CASCADE,
    -- The body as sent (the idempotency id is its `id`); a Slack message is rendered from it.
    payload         jsonb NOT NULL,
    status          text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'delivered', 'failed')),
    attempts        integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    next_attempt_at timestamptz NOT NULL DEFAULT now(),
    last_attempt_at timestamptz,
    -- The HTTP status of the last attempt, when one came back.
    last_status     integer,
    last_error      text NOT NULL DEFAULT '',
    created_at      timestamptz NOT NULL DEFAULT now(),
    delivered_at    timestamptz
);
CREATE INDEX alert_deliveries_due_idx ON alert_deliveries (next_attempt_at) WHERE status = 'pending';
CREATE INDEX alert_deliveries_destination_idx ON alert_deliveries (destination_id, created_at DESC, id DESC);
CREATE INDEX alert_deliveries_alert_idx ON alert_deliveries (alert_id) WHERE alert_id IS NOT NULL;
