-- A run that executes an issue reports back on it: the result lands in the
-- issue's timeline as a `run` entry pointing at the run, and the people who
-- care about the issue find it in their inbox.
ALTER TABLE issue_events ADD COLUMN run_id uuid REFERENCES runs (id) ON DELETE SET NULL;
ALTER TABLE issue_events DROP CONSTRAINT issue_events_kind_check;
ALTER TABLE issue_events ADD CONSTRAINT issue_events_kind_check
    CHECK (kind IN ('comment', 'state', 'priority', 'assignee', 'title', 'due', 'run'));
CREATE INDEX issue_events_run_idx ON issue_events (run_id) WHERE run_id IS NOT NULL;

ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check
    CHECK (kind IN ('assigned', 'comment', 'state', 'due', 'approval', 'alert', 'run'));
ALTER TABLE notifications DROP CONSTRAINT notifications_subject_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_subject_check
    CHECK ((kind IN ('assigned', 'comment', 'state', 'due', 'run')) = (issue_id IS NOT NULL));

-- Every finished run asks which issues its graph executes.
CREATE INDEX issues_graph_idx ON issues (graph_id) WHERE graph_id IS NOT NULL;
