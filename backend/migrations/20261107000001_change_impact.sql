-- Every issue and project carries a semantic version, which the change-impact engine
-- (src/engine/impact.rs, vendor/assessment-core) moves on each change: MAJOR when what others
-- planned against it broke, MINOR for compatible additions, PATCH for a cosmetic edit. Rows start
-- at 1.0.0.
--
-- An issue also pins the version of what it was planned against: its parent issue, or its
-- project when it has none (`basis_id`, `basis_version`). A MAJOR bump of that makes its plan
-- incompatible, like a dependency in a lockfile. NULL pins 1.0.0 of whatever it is part of now
-- (where every row starts); a pin on something it is no longer part of pins nothing.
--
-- Issues and projects are written by every edit, comment and finished run, so the checks come
-- back NOT VALID (every new row is checked) and 20261107000002 checks the rows already there.
SET LOCAL lock_timeout = '5s';

ALTER TABLE issues ADD COLUMN IF NOT EXISTS version text NOT NULL DEFAULT '1.0.0';
ALTER TABLE issues ADD COLUMN IF NOT EXISTS basis_id uuid;
ALTER TABLE issues ADD COLUMN IF NOT EXISTS basis_version text;
ALTER TABLE projects ADD COLUMN IF NOT EXISTS version text NOT NULL DEFAULT '1.0.0';

ALTER TABLE issues DROP CONSTRAINT IF EXISTS issues_version_check;
ALTER TABLE issues ADD CONSTRAINT issues_version_check
    CHECK (version ~ '^[0-9]{1,20}\.[0-9]{1,20}\.[0-9]{1,20}$'
           AND (basis_version IS NULL OR basis_version ~ '^[0-9]{1,20}\.[0-9]{1,20}\.[0-9]{1,20}$')
           AND (basis_id IS NULL) = (basis_version IS NULL))
    NOT VALID;
ALTER TABLE projects DROP CONSTRAINT IF EXISTS projects_version_check;
ALTER TABLE projects ADD CONSTRAINT projects_version_check
    CHECK (version ~ '^[0-9]{1,20}\.[0-9]{1,20}\.[0-9]{1,20}$')
    NOT VALID;

-- One assessed change to an issue or a project: how big it was, the version it moved to, what
-- else it affects (ranked, with an action each), the plan conflicts it introduced and the
-- engine's recommendation. A comment is assessed too (it moves no version).
CREATE TABLE IF NOT EXISTS change_assessments (
    id               uuid PRIMARY KEY,
    workspace_id     uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    -- The changed item's team (a project's owning team; NULL for a project of the whole
    -- workspace): who may read the assessment, also after the item is deleted.
    team_id          uuid REFERENCES teams (id) ON DELETE CASCADE,
    entity_kind      text NOT NULL CHECK (entity_kind IN ('issue', 'project')),
    -- Not a key: the assessment of a deletion outlives what it assessed.
    entity_id        uuid NOT NULL,
    actor_id         uuid REFERENCES users (id) ON DELETE SET NULL,
    cause            text NOT NULL CHECK (cause IN ('created', 'updated', 'deleted', 'comment')),
    bump             text NOT NULL
                     CHECK (bump IN ('none', 'patch', 'minor', 'major', 'initial')),
    previous_version text,
    next_version     text,
    -- The content score that drove the bump; earlier ones are the engine's change history.
    raw_score        double precision NOT NULL CHECK (raw_score BETWEEN 0 AND 1),
    score            double precision NOT NULL CHECK (score BETWEEN 0 AND 1),
    level            text NOT NULL CHECK (level IN ('none', 'low', 'medium', 'high', 'critical')),
    recommendation   text NOT NULL
                     CHECK (recommendation IN ('no_action', 'notify', 'review', 'partial_replan',
                                               'full_replan')),
    confidence       double precision NOT NULL CHECK (confidence BETWEEN 0 AND 1),
    -- The smallest subtree to re-plan; set when the recommendation is a re-plan.
    replan_root      uuid,
    summary          text NOT NULL,
    -- The other issues and projects the change asks something of, for "what changed under this
    -- issue"; `impacts` holds every affected item, ranked, with its action and reasons.
    impacted         uuid[] NOT NULL DEFAULT '{}',
    impacts          jsonb NOT NULL DEFAULT '[]',
    violations       jsonb NOT NULL DEFAULT '[]',
    created_at       timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS change_assessments_entity_idx
    ON change_assessments (entity_id, created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS change_assessments_impacted_idx
    ON change_assessments USING gin (impacted);
CREATE INDEX IF NOT EXISTS change_assessments_workspace_id_idx
    ON change_assessments (workspace_id);
CREATE INDEX IF NOT EXISTS change_assessments_team_id_idx
    ON change_assessments (team_id) WHERE team_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS change_assessments_actor_id_idx
    ON change_assessments (actor_id) WHERE actor_id IS NOT NULL;

-- The owners of an item a change affects hear of it when the engine asks for a review or a
-- re-plan: an `impact` entry in their inbox, about the affected issue.
ALTER TABLE notifications DROP CONSTRAINT IF EXISTS notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check
    CHECK (kind IN ('assigned', 'comment', 'state', 'due', 'approval', 'alert', 'run', 'mention',
                    'reaction', 'impact'))
    NOT VALID;
ALTER TABLE notifications DROP CONSTRAINT IF EXISTS notifications_subject_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_subject_check
    CHECK ((kind IN ('assigned', 'comment', 'state', 'due', 'run', 'mention', 'reaction',
                     'impact'))
           = (issue_id IS NOT NULL))
    NOT VALID;
