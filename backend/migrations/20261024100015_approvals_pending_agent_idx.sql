-- no-transaction
-- An agent's pending approvals. Every approval the Agent Firewall opens first counts the ones
-- the agent is still waiting on (`repo::approvals::pending_of_agent`, `WHERE agent_id = $1 AND
-- status = 'pending'`, against PENDING_MAX), and read the whole table: no index named the agent.
-- Pending approvals are few and leave the index when decided or expired; `status` is indexed by
-- `approvals_workspace_idx` already, so no update that was HOT stops being so. Built concurrently
-- (docs/DATABASE.md, "Migrations"): approvals grow with what agents ask for, and a node asks
-- inside its run's lease (engine::approvals), so a plain build would hold the run while it reads.
CREATE INDEX CONCURRENTLY IF NOT EXISTS approvals_pending_agent_idx ON approvals (agent_id)
    WHERE status = 'pending';
