-- An issue can link to the GitHub issue or pull request it is about. Only the link: nothing is
-- synced. The backend stores the canonical form (domain::issue::github_url), which the check
-- holds it to. A change is a `github` entry in the issue's timeline.
SET LOCAL lock_timeout = '5s';

ALTER TABLE issues ADD COLUMN github_url text
    CHECK (github_url ~ '^https://github\.com/[A-Za-z0-9-]+/[A-Za-z0-9._-]+/(issues|pull)/[0-9]+$');

ALTER TABLE issue_events DROP CONSTRAINT issue_events_kind_check;
ALTER TABLE issue_events ADD CONSTRAINT issue_events_kind_check
    CHECK (kind IN ('comment', 'state', 'priority', 'assignee', 'title', 'due', 'run', 'github'));
