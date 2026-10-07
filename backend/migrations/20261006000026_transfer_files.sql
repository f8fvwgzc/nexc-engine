-- A transfer moves everything, files included: the uploaded originals of documents and the
-- artifacts of runs, which this server keeps on disk, travel inside the target database and are
-- put back on disk by the server that receives them. The table is empty on a server that never
-- received a workspace; it is filled only on the target, never here.
CREATE TABLE workspace_files (
    id            uuid PRIMARY KEY,
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    -- `documents/<document id>` or `artifacts/<run id>/<node id>/<path>`: the layout of the
    -- data folder, so the receiving server knows where each file belongs.
    path          text NOT NULL,
    content       bytea NOT NULL,
    -- Set once the receiving server has written the file to its data folder.
    restored_at   timestamptz,
    created_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, path)
);
CREATE INDEX workspace_files_pending_idx ON workspace_files (workspace_id) WHERE restored_at IS NULL;

-- A transfer may also name a Redis the workspace moves to; stored without credentials.
ALTER TABLE workspace_transfers ADD COLUMN redis_target text NOT NULL DEFAULT '';
