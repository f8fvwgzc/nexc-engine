-- Cycles: the time boxes a team works in. An issue is in at most one.
CREATE TABLE cycles (
    id          uuid PRIMARY KEY,
    team_id     uuid NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    -- Sequential within the team.
    number      integer NOT NULL,
    name        text NOT NULL DEFAULT '',
    starts_on   date NOT NULL,
    ends_on     date NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (team_id, number),
    CHECK (ends_on >= starts_on)
);
CREATE INDEX cycles_team_idx ON cycles (team_id, starts_on);

ALTER TABLE issues ADD COLUMN cycle_id uuid REFERENCES cycles (id) ON DELETE SET NULL;
CREATE INDEX issues_cycle_idx ON issues (cycle_id) WHERE cycle_id IS NOT NULL;
