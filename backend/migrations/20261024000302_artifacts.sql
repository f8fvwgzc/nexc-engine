-- Artifacts kept by hand: text files of a workspace (code, HTML, Markdown) with every version,
-- in folders, optionally attached to an issue. The content lives here, at most 1 MiB a version,
-- so nothing lands on disk and a deleted folder, file or workspace takes its versions along.
-- Names are unique among their siblings, ignoring case; the top level is a parent of its own. A
-- file's siblings are the files of its folder on the same issue (or on none), so two issues each
-- keep a notes.md in one folder, and a file someone cannot see never takes a name from them.
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
    (workspace_id, COALESCE(folder_id, '00000000-0000-0000-0000-000000000000'::uuid),
     COALESCE(issue_id, '00000000-0000-0000-0000-000000000000'::uuid), lower(name));
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

-- The name of the `n`th copy of a file, as the server names copies: ` copy`, ` copy 2`… before
-- the extension (a whole name keeps itself as the extension), the stem cut to stay in 200
-- characters. Names stored are allowed ones, so their extension is the text after the last dot.
CREATE FUNCTION artifact_copy_name(name text, n integer) RETURNS text
LANGUAGE plpgsql IMMUTABLE AS $$
DECLARE
    suffix text := CASE WHEN n <= 1 THEN ' copy' ELSE ' copy ' || n END;
    stem text := name;
    extension text := '';
BEGIN
    IF lower(name) IN ('dockerfile', 'makefile', '.gitignore', '.editorconfig', '.env.example') THEN
        extension := '.' || ltrim(lower(name), '.');
    ELSIF length(name) > 12 AND lower(right(name, 12)) = '.env.example' THEN
        stem := left(name, -12);
        extension := right(name, 12);
    ELSIF strpos(name, '.') > 0 THEN
        extension := substring(name FROM '\.[^.]*$');
        stem := left(name, -length(extension));
    END IF;
    stem := left(stem, greatest(200 - length(suffix) - length(extension), 0));
    RETURN rtrim(stem) || suffix || extension;
END $$;

-- A file on an issue is read as the issue is; without one, by every member but guests. So a
-- deleted issue leaves the files of a public team's issue to the workspace (no one new reads
-- them), but takes those of a private team's issue along: they must not become readable by the
-- whole workspace. A team deleted takes its issues along after it is gone, when the issue can no
-- longer tell whether the team was private, so the team takes those files first. A file left to
-- the workspace joins the files of no issue in its folder, as their first copy name when its own
-- is taken there.
CREATE FUNCTION artifact_files_issue_drop() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    kept record;
    free text;
    n integer;
BEGIN
    IF TG_TABLE_NAME = 'teams' THEN
        IF OLD.private THEN
            DELETE FROM artifact_files f USING issues i
             WHERE f.issue_id = i.id AND i.team_id = OLD.id;
        END IF;
        RETURN OLD;
    END IF;
    IF EXISTS (SELECT 1 FROM teams t WHERE t.id = OLD.team_id AND t.private) THEN
        DELETE FROM artifact_files WHERE issue_id = OLD.id;
        RETURN OLD;
    END IF;
    FOR kept IN SELECT id, workspace_id, folder_id, name FROM artifact_files
                 WHERE issue_id = OLD.id ORDER BY lower(name), id LOOP
        free := kept.name;
        n := 0;
        -- Spelled as artifact_files_name_idx is, so each probe is one look in it.
        WHILE EXISTS (SELECT 1 FROM artifact_files g
                       WHERE g.workspace_id = kept.workspace_id
                         AND COALESCE(g.folder_id, '00000000-0000-0000-0000-000000000000'::uuid)
                             = COALESCE(kept.folder_id, '00000000-0000-0000-0000-000000000000'::uuid)
                         AND COALESCE(g.issue_id, '00000000-0000-0000-0000-000000000000'::uuid)
                             = '00000000-0000-0000-0000-000000000000'::uuid
                         AND lower(g.name) = lower(free)) LOOP
            n := n + 1;
            free := artifact_copy_name(kept.name, n);
        END LOOP;
        UPDATE artifact_files SET issue_id = NULL, name = free WHERE id = kept.id;
    END LOOP;
    RETURN OLD;
END $$;
CREATE TRIGGER teams_artifact_files_drop BEFORE DELETE ON teams
    FOR EACH ROW EXECUTE FUNCTION artifact_files_issue_drop();
CREATE TRIGGER issues_artifact_files_drop BEFORE DELETE ON issues
    FOR EACH ROW EXECUTE FUNCTION artifact_files_issue_drop();

-- What happens to an issue's files is an `artifact` entry in its timeline: the file's name, the
-- version it replaced, and what was done (`uploaded`, `updated:v<N>`, `duplicated`, `deleted`).
ALTER TABLE issue_events DROP CONSTRAINT issue_events_kind_check;
ALTER TABLE issue_events ADD CONSTRAINT issue_events_kind_check
    CHECK (kind IN ('comment', 'state', 'priority', 'assignee', 'title', 'due', 'run', 'github',
                    'artifact'));
