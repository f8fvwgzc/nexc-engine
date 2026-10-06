-- The timeline of an issue: comments people write, and the changes to its
-- state, priority, assignee and title, in one ordered stream.
CREATE TABLE issue_events (
    id          uuid PRIMARY KEY,
    issue_id    uuid NOT NULL REFERENCES issues (id) ON DELETE CASCADE,
    actor_id    uuid REFERENCES users (id) ON DELETE SET NULL,
    kind        text NOT NULL CHECK (kind IN ('comment', 'state', 'priority', 'assignee', 'title')),
    -- The text of a comment; empty for changes.
    body        text NOT NULL DEFAULT '',
    -- What a change replaced and what it set, as shown at the time.
    from_value  text,
    to_value    text,
    created_at  timestamptz NOT NULL DEFAULT now(),
    edited_at   timestamptz
);
CREATE INDEX issue_events_issue_idx ON issue_events (issue_id, created_at, id);
