-- Organisations: a workspace is the top of the hierarchy; users join it with
-- a role and work in teams. Roles follow Linear: owner, admin, member, guest.
CREATE TABLE workspaces (
    id          uuid PRIMARY KEY,
    name        text NOT NULL,
    slug        text NOT NULL CHECK (slug ~ '^[a-z0-9][a-z0-9-]{1,47}$'),
    created_by  uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX workspaces_slug_key ON workspaces (slug);

CREATE TABLE workspace_members (
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    user_id       uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role          text NOT NULL CHECK (role IN ('owner', 'admin', 'member', 'guest')),
    created_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, user_id)
);
CREATE INDEX workspace_members_user_idx ON workspace_members (user_id);

-- People invited by e-mail who have no account yet; they join on sign-up.
CREATE TABLE workspace_invites (
    id            uuid PRIMARY KEY,
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    email         text NOT NULL,
    role          text NOT NULL CHECK (role IN ('admin', 'member', 'guest')),
    invited_by    uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX workspace_invites_key ON workspace_invites (workspace_id, lower(email));
CREATE INDEX workspace_invites_email_idx ON workspace_invites (lower(email));

CREATE TABLE teams (
    id            uuid PRIMARY KEY,
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name          text NOT NULL,
    -- Short identifier that prefixes the team's issues, e.g. ENG.
    key           text NOT NULL CHECK (key ~ '^[A-Z][A-Z0-9]{0,6}$'),
    description   text NOT NULL DEFAULT '',
    -- Private teams are visible to their members only.
    private       boolean NOT NULL DEFAULT false,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, key)
);

CREATE TABLE team_members (
    team_id     uuid NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    user_id     uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role        text NOT NULL CHECK (role IN ('owner', 'member')),
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (team_id, user_id)
);
CREATE INDEX team_members_user_idx ON team_members (user_id);
