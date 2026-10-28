-- Checks the rows already stored against the checks 20261025000204 widened or added and the key
-- 20261025000201 added. VALIDATE CONSTRAINT takes a SHARE UPDATE EXCLUSIVE lock: inbox and
-- timeline writes go on while it reads the tables. A constraint that is already valid is not read
-- again.
SET LOCAL lock_timeout = '5s';

ALTER TABLE notifications VALIDATE CONSTRAINT notifications_kind_check;
ALTER TABLE notifications VALIDATE CONSTRAINT notifications_subject_check;
ALTER TABLE notifications VALIDATE CONSTRAINT notifications_emoji_check;
ALTER TABLE issue_events VALIDATE CONSTRAINT issue_events_parent_id_fkey;
