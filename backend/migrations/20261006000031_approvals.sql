-- Approvals (AgentSecOps phase 2): a policy that requires an approval pauses
-- the node (`waiting_approval`) while the run goes on; the approvers hear in
-- their inbox; the decision is recorded here, in the audit log and as an
-- `approval` span of the run's trace.
ALTER TABLE nodes DROP CONSTRAINT nodes_status_check;
ALTER TABLE nodes
    ADD CONSTRAINT nodes_status_check
    CHECK (status IN ('idle', 'queued', 'running', 'waiting_approval', 'succeeded', 'failed', 'skipped', 'cancelled'));
ALTER TABLE node_runs DROP CONSTRAINT node_runs_status_check;
ALTER TABLE node_runs
    ADD CONSTRAINT node_runs_status_check
    CHECK (status IN ('idle', 'queued', 'running', 'waiting_approval', 'succeeded', 'failed', 'skipped', 'cancelled'));

CREATE TABLE approvals (
    id               uuid PRIMARY KEY,
    workspace_id     uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    run_id           uuid NOT NULL REFERENCES runs (id) ON DELETE CASCADE,
    node_id          uuid NOT NULL,
    graph_id         uuid NOT NULL,
    agent_id         uuid,
    -- Who asked: the acting principal.
    principal        text NOT NULL,
    -- The policy span that asked.
    span_id          uuid NOT NULL,
    -- What waits: the tool and a digest of its arguments (never the arguments).
    tool             text NOT NULL,
    args_sha256      text,
    rule             text NOT NULL,
    reason           text NOT NULL DEFAULT '',
    -- Who may decide; empty means every admin of the workspace.
    approver_ids     uuid[] NOT NULL DEFAULT '{}',
    status           text NOT NULL DEFAULT 'pending'
                     CHECK (status IN ('pending', 'approved', 'denied', 'expired')),
    requested_at     timestamptz NOT NULL DEFAULT now(),
    expires_at       timestamptz NOT NULL,
    decided_by       uuid REFERENCES users (id) ON DELETE SET NULL,
    decided_by_name  text NOT NULL DEFAULT '',
    decision_reason  text NOT NULL DEFAULT '',
    decided_at       timestamptz,
    -- The `approval` span that recorded the decision.
    decision_span_id uuid,
    policy_versions  jsonb NOT NULL DEFAULT '{}'::jsonb
);
CREATE INDEX approvals_workspace_idx ON approvals (workspace_id, status, requested_at DESC);
CREATE INDEX approvals_run_idx ON approvals (run_id);

-- The inbox also carries approvals to decide; those entries are about no issue.
ALTER TABLE notifications ALTER COLUMN issue_id DROP NOT NULL;
ALTER TABLE notifications
    ADD COLUMN approval_id uuid REFERENCES approvals (id) ON DELETE CASCADE;
ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications
    ADD CONSTRAINT notifications_kind_check
    CHECK (kind IN ('assigned', 'comment', 'state', 'due', 'approval', 'alert'));
ALTER TABLE notifications
    ADD CONSTRAINT notifications_subject_check
    CHECK ((kind IN ('assigned', 'comment', 'state', 'due')) = (issue_id IS NOT NULL));
CREATE INDEX notifications_approval_idx ON notifications (approval_id) WHERE approval_id IS NOT NULL;
