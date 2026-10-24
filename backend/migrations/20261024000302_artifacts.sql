-- Artifacts kept by hand: text files of a workspace (code, HTML, Markdown) with every version,
-- in folders, optionally attached to an issue. The content lives here, at most 1 MiB a version,
-- so nothing lands on disk and a deleted folder, file or workspace takes its versions along.
-- Names are unique among their siblings, ignoring case; the top level is a parent of its own.
SET LOCAL lock_timeout = '5s';

CREATE TABLE artifact_folders (
    id            uuid PRIMARY KEY,
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    parent_id     uuid REFERENCES artifact_folders (id) ON DELETE CASCADE,
    name          text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    created_by    uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX artifact_folders_name_idx ON artifact_folders
    (workspace_id, COALESCE(parent_id, '00000000-0000-0000-0000-000000000000'::uuid), lower(name));
CREATE INDEX artifact_folders_parent_idx ON artifact_folders (parent_id) WHERE parent_id IS NOT NULL;

CREATE TABLE artifact_files (
    id               uuid PRIMARY KEY,
    workspace_id     uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    folder_id        uuid REFERENCES artifact_folders (id) ON DELETE CASCADE,
    issue_id         uuid REFERENCES issues (id) ON DELETE SET NULL,
    name             text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 200),
    current_version  integer NOT NULL CHECK (current_version >= 1),
    size             bigint NOT NULL CHECK (size >= 0),
    sha256           text NOT NULL,
    -- The file this one was copied from, while it exists.
    duplicated_from  uuid REFERENCES artifact_files (id) ON DELETE SET NULL,
    created_by       uuid REFERENCES users (id) ON DELETE SET NULL,
    updated_by       uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX artifact_files_name_idx ON artifact_files
    (workspace_id, COALESCE(folder_id, '00000000-0000-0000-0000-000000000000'::uuid), lower(name));
CREATE INDEX artifact_files_folder_idx ON artifact_files (folder_id) WHERE folder_id IS NOT NULL;
CREATE INDEX artifact_files_issue_idx ON artifact_files (issue_id) WHERE issue_id IS NOT NULL;
CREATE INDEX artifact_files_copies_idx ON artifact_files (duplicated_from)
    WHERE duplicated_from IS NOT NULL;
CREATE INDEX artifact_files_updated_idx ON artifact_files (workspace_id, updated_at DESC);

CREATE TABLE artifact_file_versions (
    file_id     uuid NOT NULL REFERENCES artifact_files (id) ON DELETE CASCADE,
    version     integer NOT NULL CHECK (version >= 1),
    content     text NOT NULL CHECK (octet_length(content) <= 1048576),
    size        bigint NOT NULL CHECK (size >= 0),
    sha256      text NOT NULL,
    created_by  uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (file_id, version)
);

-- A file on an issue is read as the issue is; without one, by every member but guests. So a
-- deleted issue leaves the files of a public team's issue to the workspace (no one new reads
-- them), but takes those of a private team's issue along: they must not become readable by the
-- whole workspace. A team deleted takes its issues along after it is gone, when the issue can no
-- longer tell whether the team was private, so the team takes those files first.
CREATE FUNCTION artifact_files_private_drop() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_TABLE_NAME = 'teams' THEN
        IF OLD.private THEN
            DELETE FROM artifact_files f USING issues i
             WHERE f.issue_id = i.id AND i.team_id = OLD.id;
        END IF;
    ELSE
        DELETE FROM artifact_files f USING teams t
         WHERE f.issue_id = OLD.id AND t.id = OLD.team_id AND t.private;
    END IF;
    RETURN OLD;
END $$;
CREATE TRIGGER teams_artifact_files_private_drop BEFORE DELETE ON teams
    FOR EACH ROW EXECUTE FUNCTION artifact_files_private_drop();
CREATE TRIGGER issues_artifact_files_private_drop BEFORE DELETE ON issues
    FOR EACH ROW EXECUTE FUNCTION artifact_files_private_drop();

-- What happens to an issue's files is an `artifact` entry in its timeline: the file's name, the
-- version it replaced, and what was done (`uploaded`, `updated:v<N>`, `duplicated`, `deleted`).
ALTER TABLE issue_events DROP CONSTRAINT issue_events_kind_check;
ALTER TABLE issue_events ADD CONSTRAINT issue_events_kind_check
    CHECK (kind IN ('comment', 'state', 'priority', 'assignee', 'title', 'due', 'run', 'github',
                    'artifact'));
