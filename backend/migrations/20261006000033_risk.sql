-- Behaviour baseline and risk score (AgentSecOps phase 2): what each agent
-- usually does per run, learned from its spans as runs finish, and how far
-- its latest run strayed from it.
CREATE TABLE agent_baselines (
    agent_id    uuid PRIMARY KEY REFERENCES agents (id) ON DELETE CASCADE,
    -- Runs the baseline was learned from.
    runs        integer NOT NULL DEFAULT 0 CHECK (runs >= 0),
    -- Per feature, the running mean and the sum of squared deviations (Welford):
    -- `{"tool_calls_per_turn": {"mean": 2.1, "m2": 4.2}, ...}`.
    features    jsonb NOT NULL DEFAULT '{}'::jsonb,
    -- MinHash signatures of the tool sequences of the last runs, newest last
    -- (`[[u64 as text, ...], ...]`): "unlike any earlier run" is a low best match.
    signatures  jsonb NOT NULL DEFAULT '[]'::jsonb,
    updated_at  timestamptz NOT NULL DEFAULT now()
);

ALTER TABLE agents
    ADD COLUMN risk_score    double precision NOT NULL DEFAULT 0 CHECK (risk_score >= 0),
    -- Why, in words: `["3.2x its usual tool calls per turn", "2 hosts it never called"]`.
    ADD COLUMN risk_reasons  jsonb NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN risk_updated_at timestamptz;
