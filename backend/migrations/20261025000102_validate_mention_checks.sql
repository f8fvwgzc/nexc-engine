-- Checks the rows already stored against the checks 20261025000100 and 20261025000101 widened.
-- VALIDATE CONSTRAINT takes a SHARE UPDATE EXCLUSIVE lock: inbox and timeline writes go on while
-- it reads the tables. A check that is already valid is not read again.
SET LOCAL lock_timeout = '5s';

ALTER TABLE notifications VALIDATE CONSTRAINT notifications_kind_check;
ALTER TABLE notifications VALIDATE CONSTRAINT notifications_subject_check;
ALTER TABLE issue_events VALIDATE CONSTRAINT issue_events_kind_check;
