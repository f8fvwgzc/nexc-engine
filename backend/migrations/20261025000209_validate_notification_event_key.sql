-- Checks the rows already stored against the key 20261025000206 added. VALIDATE CONSTRAINT takes a
-- SHARE UPDATE EXCLUSIVE lock: inbox and timeline writes go on while it reads notifications. A key
-- that is already valid is not read again.
SET LOCAL lock_timeout = '5s';

ALTER TABLE notifications VALIDATE CONSTRAINT notifications_event_id_fkey;
