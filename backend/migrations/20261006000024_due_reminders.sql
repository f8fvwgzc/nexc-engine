-- One reminder in the assignee's inbox when an open issue's due date arrives. `due_notified_on`
-- is the due date the reminder was sent for, so a moved date is reminded of again.
ALTER TABLE issues ADD COLUMN due_notified_on date;

ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check
    CHECK (kind IN ('assigned', 'comment', 'state', 'due'));
