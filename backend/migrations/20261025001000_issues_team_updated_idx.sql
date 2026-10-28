-- no-transaction
-- A team's issues, most recently updated first: the board and the issues page with one team
-- picked (`repo::issues::list`, `i.team_id = $3 ... ORDER BY i.updated_at DESC LIMIT $8`). Only
-- `issues_workspace_idx (workspace_id, updated_at DESC)` gave that order, so a team's list walked
-- every issue of the workspace and threw away the other teams'. `issues_team_state_idx (team_id,
-- state_id)` stays: the workflow page counts a team's issues by state. Built concurrently
-- (docs/DATABASE.md, "Migrations"): issues are written all day.
CREATE INDEX CONCURRENTLY IF NOT EXISTS issues_team_updated_idx ON issues (team_id, updated_at DESC);
