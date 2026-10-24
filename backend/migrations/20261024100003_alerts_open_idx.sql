-- The alerts nobody acknowledged, on the AgentSecOps console (`repo::secops::alerts_open`,
-- `WHERE workspace_id = $1 AND acknowledged_at IS NULL`), which read every alert of the workspace
-- to count the few still open. The partial index holds only those: an acknowledgement takes the
-- row out. Alerts are raised outside any run's lease (engine::defend) and acknowledged by people,
-- so the plain build's short lock holds no run.
SET LOCAL lock_timeout = '5s';

CREATE INDEX IF NOT EXISTS alerts_open_idx ON alerts (workspace_id) WHERE acknowledged_at IS NULL;
