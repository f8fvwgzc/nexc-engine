-- A re-plan the change-impact engine called for (an assessment recommending `partial_replan` or
-- `full_replan`) and nexc's planner drafts: a plan of the re-plan root's graph (an issue's; the
-- issue gets one when it has none), started after the change commits and once no further change
-- to the same root came for NEXC_REPLAN_DEBOUNCE_SECS, so a burst of edits starts one LLM run.
-- Nothing is applied on its own: the plan waits on the graph until a person applies it.
--
-- At most one open proposal (pending, starting or started) per root: a newer assessment that asks
-- for a re-plan of the same root extends a pending one (its seed becomes the newer assessment) and
-- supersedes one already started.
--
-- `root_id` and `assessment_id` are not keys: the proposal outlives a deleted root (it is skipped),
-- and the workspace transfer copies change_assessments after this table's referrers.
SET LOCAL lock_timeout = '5s';

CREATE TABLE IF NOT EXISTS replan_proposals (
    id            uuid PRIMARY KEY,
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    -- The root's team (who may read the proposal); NULL for a project of the whole workspace.
    team_id       uuid REFERENCES teams (id) ON DELETE CASCADE,
    root_kind     text NOT NULL CHECK (root_kind IN ('issue', 'project')),
    root_id       uuid NOT NULL,
    -- The latest assessment that asked for it: what the planner is told.
    assessment_id uuid NOT NULL,
    -- Whom the planner runs as (their LLM key, their guardrails): who made the change.
    owner_id      uuid REFERENCES users (id) ON DELETE SET NULL,
    -- pending: waiting out the debounce (or a plan still streaming on the graph);
    -- starting: claimed by a backend that is starting the planner;
    -- started: the plan is drafted in `plan_id` (its own status says ready, failed or applied);
    -- superseded: a newer assessment of the same root replaced it;
    -- skipped: no plan could be drafted, `error` says why.
    status        text NOT NULL
                  CHECK (status IN ('pending', 'starting', 'started', 'superseded', 'skipped')),
    graph_id      uuid REFERENCES graphs (id) ON DELETE SET NULL,
    plan_id       uuid REFERENCES plans (id) ON DELETE SET NULL,
    error         text,
    -- When the planner may start: each further change to the root pushes it back, up to ten
    -- debounce periods after the proposal was made.
    not_before    timestamptz NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS replan_proposals_open_idx
    ON replan_proposals (root_id) WHERE status IN ('pending', 'starting', 'started');
CREATE INDEX IF NOT EXISTS replan_proposals_due_idx
    ON replan_proposals (not_before) WHERE status = 'pending';
CREATE INDEX IF NOT EXISTS replan_proposals_workspace_id_idx ON replan_proposals (workspace_id);
CREATE INDEX IF NOT EXISTS replan_proposals_team_id_idx
    ON replan_proposals (team_id) WHERE team_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS replan_proposals_owner_id_idx
    ON replan_proposals (owner_id) WHERE owner_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS replan_proposals_graph_id_idx
    ON replan_proposals (graph_id) WHERE graph_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS replan_proposals_plan_id_idx
    ON replan_proposals (plan_id) WHERE plan_id IS NOT NULL;

-- Every assessment that asked for a re-plan points at the proposal that took it up: the ones of a
-- burst share one.
ALTER TABLE change_assessments ADD COLUMN IF NOT EXISTS replan_id uuid;

-- The assessment an `impact` entry is about: the inbox reads its re-plan proposal through it.
ALTER TABLE notifications ADD COLUMN IF NOT EXISTS assessment_id uuid;
