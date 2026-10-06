-- Transfers of a workspace's data to a PostgreSQL the workspace's owner names.
-- The connection string itself is never stored: only where it pointed.
CREATE TABLE workspace_transfers (
    id            uuid PRIMARY KEY,
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    started_by    uuid REFERENCES users (id) ON DELETE SET NULL,
    -- The target without credentials: postgres://host:5432/db
    target        text NOT NULL,
    status        text NOT NULL DEFAULT 'running' CHECK (status IN ('running', 'done', 'failed')),
    -- Per table: rows read here and rows written there.
    report        jsonb NOT NULL DEFAULT '[]',
    error         text NOT NULL DEFAULT '',
    created_at    timestamptz NOT NULL DEFAULT now(),
    finished_at   timestamptz
);
CREATE INDEX workspace_transfers_workspace_idx ON workspace_transfers (workspace_id, created_at DESC);
