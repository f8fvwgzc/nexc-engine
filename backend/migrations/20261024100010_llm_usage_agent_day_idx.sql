-- no-transaction
-- What an agent spent today. `repo::usage::agent_today` (`WHERE agent_id = $1 AND created_at >=
-- <start of the UTC day>`) runs in the scheduler's gate before every node of an agent whose policy
-- has a daily token or dollar limit, and read the whole usage ledger: no index named the agent.
-- Calls of no agent stay out. Built concurrently (docs/DATABASE.md, "Migrations").
CREATE INDEX CONCURRENTLY IF NOT EXISTS llm_usage_agent_day_idx ON llm_usage (agent_id, created_at)
    WHERE agent_id IS NOT NULL;
