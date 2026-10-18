-- Opt-in recording of prompts, completions and tool calls (the guardrails'
-- `recording`): what a span said, kept apart from the span. The span itself
-- keeps carrying digests only. Content is AES-256-GCM under the master key,
-- bound to the span id, after the secret scrubber and the label redaction
-- ran over it; a worker deletes rows past the workspace's retention; a
-- transfer leaves them behind (the key stays here).
CREATE TABLE span_payloads (
    span_id    uuid NOT NULL,
    kind       text NOT NULL CHECK (kind IN ('prompt', 'completion', 'tool_input', 'tool_output')),
    run_id     uuid NOT NULL REFERENCES runs (id) ON DELETE CASCADE,
    content    bytea NOT NULL,
    -- Bytes of the content before encryption, after redaction.
    size       integer NOT NULL CHECK (size >= 0),
    -- Passages and memories replaced by their citation, plus credentials scrubbed.
    redactions integer NOT NULL DEFAULT 0 CHECK (redactions >= 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (span_id, kind)
);
CREATE INDEX span_payloads_run_idx ON span_payloads (run_id);
CREATE INDEX span_payloads_created_idx ON span_payloads (created_at);
