-- A `reaction` inbox entry names the comment it is about in `event_id`, so the same emoji from
-- the same person tells the author once a day per comment rather than per issue
-- (repo::issues::notify_reaction). Deleting the comment keeps the entry, about the issue.
--
-- Every comment, move and finished run writes notifications, and a finished run writes its report
-- into issue_events, both inside its lease: the key comes back NOT VALID, so adding it reads
-- neither table, and 20261025000209 checks the rows already there (all of them without an entry).
-- Its index is built concurrently by 20261025000207.
SET LOCAL lock_timeout = '5s';

ALTER TABLE notifications ADD COLUMN IF NOT EXISTS event_id uuid;
ALTER TABLE notifications DROP CONSTRAINT IF EXISTS notifications_event_id_fkey;
ALTER TABLE notifications ADD CONSTRAINT notifications_event_id_fkey
    FOREIGN KEY (event_id) REFERENCES issue_events (id) ON DELETE SET NULL NOT VALID;
