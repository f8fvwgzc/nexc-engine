-- A member mentioned in a comment hears of it: a `mention` entry in their inbox, about the issue.
-- Every comment, move and finished run writes notifications (docs/DATABASE.md, "Building
-- indexes"), so both checks come back NOT VALID: they hold every row written from now on, without
-- reading the table under the exclusive lock a plain check would keep for the whole file.
-- 20261025000102 then checks the rows already there.
SET LOCAL lock_timeout = '5s';

ALTER TABLE notifications DROP CONSTRAINT IF EXISTS notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check
    CHECK (kind IN ('assigned', 'comment', 'state', 'due', 'approval', 'alert', 'run', 'mention'))
    NOT VALID;
ALTER TABLE notifications DROP CONSTRAINT IF EXISTS notifications_subject_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_subject_check
    CHECK ((kind IN ('assigned', 'comment', 'state', 'due', 'run', 'mention')) = (issue_id IS NOT NULL))
    NOT VALID;
