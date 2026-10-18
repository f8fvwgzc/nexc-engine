-- Sessions: one run seen as a tree of nodes, agents and sub-agents, with
-- tokens and cost rolled up per subtree. The tree is computed from
-- agent_spans, node_runs and llm_usage; the list of a workspace's sessions
-- is narrowed by the agent that worked in a run, which this index serves.
CREATE INDEX node_runs_agent_idx ON node_runs (agent_id, run_id) WHERE agent_id IS NOT NULL;
