-- Labels of a workspace, and which issues carry them.
CREATE TABLE labels (
    id            uuid PRIMARY KEY,
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name          text NOT NULL,
    color         text NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX labels_workspace_name_idx ON labels (workspace_id, lower(name));

CREATE TABLE issue_labels (
    issue_id  uuid NOT NULL REFERENCES issues (id) ON DELETE CASCADE,
    label_id  uuid NOT NULL REFERENCES labels (id) ON DELETE CASCADE,
    PRIMARY KEY (issue_id, label_id)
);
CREATE INDEX issue_labels_label_idx ON issue_labels (label_id);
