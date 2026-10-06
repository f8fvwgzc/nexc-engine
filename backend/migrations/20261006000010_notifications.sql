-- A member's inbox: what happened to the issues they created or are assigned.
CREATE TABLE notifications (
    id            uuid PRIMARY KEY,
    user_id       uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    issue_id      uuid NOT NULL REFERENCES issues (id) ON DELETE CASCADE,
    actor_id      uuid REFERENCES users (id) ON DELETE SET NULL,
    kind          text NOT NULL CHECK (kind IN ('assigned', 'comment', 'state')),
    created_at    timestamptz NOT NULL DEFAULT now(),
    read_at       timestamptz
);
CREATE INDEX notifications_user_idx ON notifications (user_id, workspace_id, created_at DESC);
