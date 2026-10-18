-- Commands an operator sends into a run while it executes: so far, the
-- cancel of one sub-agent. The handler records the command; the instance
-- executing the run takes it (at once when that is this instance, within a
-- second otherwise, from the run's cancel watcher) and sends it down the
-- node attempt's stream, with a `policy` span on the trace.
CREATE TABLE run_commands (
    id           uuid PRIMARY KEY,
    run_id       uuid NOT NULL REFERENCES runs (id) ON DELETE CASCADE,
    node_id      uuid NOT NULL,
    command      text NOT NULL CHECK (command IN ('cancel_agent')),
    -- The `spawn` span of the sub-agent the command is about.
    span_id      uuid NOT NULL,
    -- The sub-agent's principal.
    subject      text NOT NULL DEFAULT '',
    reason       text NOT NULL DEFAULT '',
    requested_by uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    taken_at     timestamptz
);
CREATE INDEX run_commands_pending_idx ON run_commands (run_id) WHERE taken_at IS NULL;
