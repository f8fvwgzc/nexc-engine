-- The trace spine: every action an agent takes during a run, as a span.
-- One row per LLM call, tool call, sub-agent spawn, data read, egress,
-- policy decision or approval. Rows are never updated. Each run's spans
-- form a hash chain in order of receipt (`seq`):
--   hash = SHA-256(prev_hash || canonical JSON of the span)
-- with 32 zero bytes before the first span. The chain's final hash is
-- written to the run and to the audit log (`trace_sealed`) when the run
-- ends, so a row changed afterwards is found by `GET /runs/{id}/trace/verify`.
CREATE TABLE agent_spans (
    id            uuid PRIMARY KEY,
    run_id        uuid NOT NULL REFERENCES runs (id) ON DELETE CASCADE,
    node_id       uuid NOT NULL,
    -- The workspace agent assigned to the node; NULL for a node without one.
    agent_id      uuid,
    -- Who acted: the agent's name as the runtime reports it (sub-agents
    -- are named `parent/role-n`), or `system` for the backend's own spans.
    principal     text NOT NULL,
    -- The turn a tool call belongs to, the spawn a sub-agent's turns belong to.
    parent_id     uuid,
    kind          text NOT NULL
                  CHECK (kind IN ('llm_call', 'tool_call', 'spawn', 'data_read', 'egress', 'policy', 'approval')),
    name          text NOT NULL,
    started_at    timestamptz NOT NULL,
    ended_at      timestamptz,
    status        text NOT NULL CHECK (status IN ('ok', 'error')),
    -- Digests, sizes, counts and ids: never tool arguments, never secrets.
    attributes    jsonb NOT NULL DEFAULT '{}'::jsonb,
    -- Position in the run's chain, in order of receipt by the backend.
    seq           integer NOT NULL CHECK (seq >= 0),
    prev_hash     bytea NOT NULL CHECK (octet_length(prev_hash) = 32),
    hash          bytea NOT NULL CHECK (octet_length(hash) = 32),
    UNIQUE (run_id, seq)
);
CREATE INDEX agent_spans_run_idx ON agent_spans (run_id, id);
CREATE INDEX agent_spans_agent_idx ON agent_spans (agent_id, started_at DESC);

-- Usage joins the trace: a call is attributable to the agent, the node and
-- (for the built-in executor's single call) the span that spent the tokens.
ALTER TABLE llm_usage
    ADD COLUMN agent_id uuid,
    ADD COLUMN node_id  uuid,
    ADD COLUMN span_id  uuid;

-- The final hash of the run's chain, set when the run ends (NULL while it
-- runs, and for a run that produced no spans).
ALTER TABLE runs
    ADD COLUMN trace_hash bytea CHECK (trace_hash IS NULL OR octet_length(trace_hash) = 32);
