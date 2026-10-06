-- Issues, their workflow states and projects (Linear's model). A team owns its
-- workflow: the states are rows, only their category is a fixed vocabulary.
CREATE TABLE issue_states (
    id        uuid PRIMARY KEY,
    team_id   uuid NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    name      text NOT NULL,
    category  text NOT NULL CHECK (category IN ('backlog', 'unstarted', 'started', 'completed', 'canceled')),
    color     text NOT NULL,
    position  integer NOT NULL DEFAULT 0,
    UNIQUE (team_id, name)
);
CREATE INDEX issue_states_team_idx ON issue_states (team_id, position);

ALTER TABLE teams ADD COLUMN next_issue_number integer NOT NULL DEFAULT 1;

CREATE TABLE projects (
    id            uuid PRIMARY KEY,
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name          text NOT NULL,
    description   text NOT NULL DEFAULT '',
    status        text NOT NULL DEFAULT 'planned'
                  CHECK (status IN ('planned', 'started', 'paused', 'completed', 'canceled')),
    lead_id       uuid REFERENCES users (id) ON DELETE SET NULL,
    target_date   date,
    created_by    uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX projects_workspace_idx ON projects (workspace_id, created_at);

CREATE TABLE issues (
    id            uuid PRIMARY KEY,
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    team_id       uuid NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    -- Sequential within the team; with the team key it forms the identifier (ENG-12).
    number        integer NOT NULL,
    title         text NOT NULL,
    description   text NOT NULL DEFAULT '',
    state_id      uuid NOT NULL REFERENCES issue_states (id) ON DELETE RESTRICT,
    -- 0 none, 1 urgent, 2 high, 3 medium, 4 low.
    priority      smallint NOT NULL DEFAULT 0 CHECK (priority BETWEEN 0 AND 4),
    assignee_id   uuid REFERENCES users (id) ON DELETE SET NULL,
    -- The agent the issue is delegated to, if any.
    agent_id      uuid REFERENCES agents (id) ON DELETE SET NULL,
    project_id    uuid REFERENCES projects (id) ON DELETE SET NULL,
    -- The graph that plans and executes the issue, if one was created.
    graph_id      uuid REFERENCES graphs (id) ON DELETE SET NULL,
    creator_id    uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    completed_at  timestamptz,
    UNIQUE (team_id, number)
);
CREATE INDEX issues_workspace_idx ON issues (workspace_id, updated_at DESC);
CREATE INDEX issues_team_state_idx ON issues (team_id, state_id);
CREATE INDEX issues_assignee_idx ON issues (assignee_id) WHERE assignee_id IS NOT NULL;
CREATE INDEX issues_project_idx ON issues (project_id) WHERE project_id IS NOT NULL;
