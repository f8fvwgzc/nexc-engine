-- The Agent Firewall scored and held (AgentSecOps, 2026-10-20).
--
-- Baselines per scope: an agent's runs (native, or reported by an external
-- agent) and, for an external agent, its decision sessions: windows of
-- firewall decisions that are not runs and are not compared with them.
ALTER TABLE agent_baselines
    ADD COLUMN scope text NOT NULL DEFAULT 'runs' CHECK (scope IN ('runs', 'sessions'));
ALTER TABLE agent_baselines DROP CONSTRAINT agent_baselines_pkey;
ALTER TABLE agent_baselines ADD PRIMARY KEY (agent_id, scope);

-- An approval an external agent asked for through the firewall lets one
-- reported call go ahead: the `tool_call` span that used it.
ALTER TABLE approvals ADD COLUMN consumed_span_id uuid;
