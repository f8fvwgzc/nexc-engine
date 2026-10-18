-- The AgentSecOps console (phase 5): every risk score an agent was given,
-- kept so the console can draw the last fourteen days. The engine writes
-- `agents.risk_score`; a trigger keeps the history, so scoring does not have
-- to know about it.
CREATE TABLE agent_risk_history (
    id           bigserial PRIMARY KEY,
    agent_id     uuid NOT NULL REFERENCES agents (id) ON DELETE CASCADE,
    workspace_id uuid,
    score        double precision NOT NULL CHECK (score >= 0),
    reasons      jsonb NOT NULL DEFAULT '[]'::jsonb,
    scored_at    timestamptz NOT NULL
);
CREATE INDEX agent_risk_history_workspace_idx ON agent_risk_history (workspace_id, scored_at DESC);
CREATE INDEX agent_risk_history_agent_idx ON agent_risk_history (agent_id, scored_at DESC);

CREATE FUNCTION agents_record_risk() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO agent_risk_history (agent_id, workspace_id, score, reasons, scored_at)
    VALUES (NEW.id, NEW.workspace_id, NEW.risk_score, NEW.risk_reasons,
            COALESCE(NEW.risk_updated_at, now()));
    RETURN NULL;
END $$;
CREATE TRIGGER agents_risk_history
    AFTER UPDATE OF risk_updated_at ON agents
    FOR EACH ROW
    WHEN (NEW.risk_updated_at IS DISTINCT FROM OLD.risk_updated_at)
    EXECUTE FUNCTION agents_record_risk();
