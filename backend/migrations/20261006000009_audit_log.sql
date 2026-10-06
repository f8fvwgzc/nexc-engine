-- Who changed what in a workspace: membership, teams, credentials, guardrails.
-- Names are copied in, so an entry still reads after the account or team is gone.
CREATE TABLE audit_log (
    id            uuid PRIMARY KEY,
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    actor_id      uuid REFERENCES users (id) ON DELETE SET NULL,
    actor_name    text NOT NULL,
    action        text NOT NULL,
    -- What the action was about: a person, a team, a label.
    subject       text NOT NULL DEFAULT '',
    -- What changed, in words ("member -> admin").
    detail        text NOT NULL DEFAULT '',
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX audit_log_workspace_idx ON audit_log (workspace_id, created_at DESC, id DESC);
