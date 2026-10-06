-- The day an issue is due, if it has one, and a timeline entry for when that changes.
ALTER TABLE issues ADD COLUMN due_date date;

ALTER TABLE issue_events DROP CONSTRAINT issue_events_kind_check;
ALTER TABLE issue_events ADD CONSTRAINT issue_events_kind_check
    CHECK (kind IN ('comment', 'state', 'priority', 'assignee', 'title', 'due'));

-- "Created by me" reads a person's issues without walking the workspace's.
CREATE INDEX issues_creator_idx ON issues (creator_id) WHERE creator_id IS NOT NULL;
