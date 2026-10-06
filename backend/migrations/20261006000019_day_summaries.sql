-- A day of a workspace's timeline, told in a few lines by the workspace's model. It is kept, so
-- a day is paid for once; `event_count` is how many entries it was written from, which tells
-- when more has happened since.
CREATE TABLE day_summaries (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    day date NOT NULL,
    headline text NOT NULL,
    highlights text[] NOT NULL DEFAULT '{}',
    attention text[] NOT NULL DEFAULT '{}',
    event_count bigint NOT NULL,
    model text NOT NULL,
    created_by uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, day)
);

-- Writing a summary is spending too: it is booked like every model call.
ALTER TABLE llm_usage DROP CONSTRAINT llm_usage_purpose_check;
ALTER TABLE llm_usage ADD CONSTRAINT llm_usage_purpose_check
    CHECK (purpose IN ('plan', 'node', 'memory', 'assistant', 'embedding', 'summary'));
