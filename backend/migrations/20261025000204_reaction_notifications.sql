-- The author of a comment hears when someone else reacts to it: a `reaction` entry in their inbox,
-- about the issue, with the emoji in `emoji` (which only that kind carries). As in
-- 20261025000100, every comment, move and finished run writes notifications, so the checks come
-- back NOT VALID and 20261025000205 checks the rows already there.
SET LOCAL lock_timeout = '5s';

ALTER TABLE notifications ADD COLUMN IF NOT EXISTS emoji text;
ALTER TABLE notifications DROP CONSTRAINT IF EXISTS notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check
    CHECK (kind IN ('assigned', 'comment', 'state', 'due', 'approval', 'alert', 'run', 'mention',
                    'reaction'))
    NOT VALID;
ALTER TABLE notifications DROP CONSTRAINT IF EXISTS notifications_subject_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_subject_check
    CHECK ((kind IN ('assigned', 'comment', 'state', 'due', 'run', 'mention', 'reaction'))
           = (issue_id IS NOT NULL))
    NOT VALID;
ALTER TABLE notifications DROP CONSTRAINT IF EXISTS notifications_emoji_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_emoji_check
    CHECK ((kind = 'reaction') = (emoji IS NOT NULL))
    NOT VALID;
