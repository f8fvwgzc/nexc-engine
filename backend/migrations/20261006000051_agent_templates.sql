-- Role templates for agents (AgentSecOps §4.6): the competency evaluation an
-- agent made from a template passed once, with the workspace's model, is the
-- score shown on its card.
CREATE TABLE agent_evaluations (
    agent_id     uuid PRIMARY KEY REFERENCES agents (id) ON DELETE CASCADE,
    template     text NOT NULL,
    score        integer NOT NULL CHECK (score >= 0),
    total        integer NOT NULL CHECK (total >= score),
    model        text NOT NULL,
    -- `[{question, expected, passed, answer}]`: the answer is a short excerpt.
    questions    jsonb NOT NULL DEFAULT '[]'::jsonb,
    evaluated_at timestamptz NOT NULL DEFAULT now()
);

-- The evaluation's calls are usage like any other.
ALTER TABLE llm_usage DROP CONSTRAINT llm_usage_purpose_check;
ALTER TABLE llm_usage ADD CONSTRAINT llm_usage_purpose_check
    CHECK (purpose IN ('plan', 'node', 'memory', 'assistant', 'embedding', 'summary', 'evaluation'));
