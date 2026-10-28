-- no-transaction
-- The issues of a project with their states and teams. Every project the projects page, the
-- issue page's picker and search show counts the issues of it the reader may see, and how many
-- are closed (`project_for_user!`): with the state and the team in the index those counts read
-- the index alone, never the issues. It serves the project page's list (`project_id = $5`) and
-- the ON DELETE SET NULL from projects as well, so 20261025001011 drops `issues_project_idx
-- (project_id)` once this one is known to be valid. Built concurrently (docs/DATABASE.md,
-- "Migrations").
CREATE INDEX CONCURRENTLY IF NOT EXISTS issues_project_state_idx ON issues (project_id, state_id, team_id)
    WHERE project_id IS NOT NULL;
