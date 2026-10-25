-- A comment that mentions an agent can hand it the issue: an `agent` entry in the issue's timeline
-- says so (`to_value` is the agent's name, `body` what was asked: `delegated`, `asked to plan`).
-- A finished run writes its report into issue_events inside its lease, so the check comes back NOT
-- VALID, like 20261025000100's, and 20261025000102 checks the rows already there.
SET LOCAL lock_timeout = '5s';

ALTER TABLE issue_events DROP CONSTRAINT IF EXISTS issue_events_kind_check;
ALTER TABLE issue_events ADD CONSTRAINT issue_events_kind_check
    CHECK (kind IN ('comment', 'state', 'priority', 'assignee', 'title', 'due', 'run', 'github',
                    'artifact', 'agent'))
    NOT VALID;
