-- Checks the rows already stored against the checks 20261107000001 added or widened. VALIDATE
-- CONSTRAINT takes a SHARE UPDATE EXCLUSIVE lock: edits, comments and the inbox go on while it
-- reads the tables. A constraint that is already valid is not read again.
SET LOCAL lock_timeout = '5s';

ALTER TABLE issues VALIDATE CONSTRAINT issues_version_check;
ALTER TABLE projects VALIDATE CONSTRAINT projects_version_check;
ALTER TABLE notifications VALIDATE CONSTRAINT notifications_kind_check;
ALTER TABLE notifications VALIDATE CONSTRAINT notifications_subject_check;
