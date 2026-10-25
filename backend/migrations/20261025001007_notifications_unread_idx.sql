-- no-transaction
-- A member's unread notifications in a workspace: the sidebar badge asks for their number every
-- minute and after edits (`repo::issues::unread_count`, `GET .../inbox/unread-count`), and
-- marking all read touches only them (`repo::issues::mark_read`). `notifications_user_idx
-- (user_id, workspace_id, created_at DESC)` holds the read ones too, which pile up for ever.
-- Built concurrently (docs/DATABASE.md, "Migrations"): every issue change writes notifications.
CREATE INDEX CONCURRENTLY IF NOT EXISTS notifications_unread_idx ON notifications (user_id, workspace_id)
    WHERE read_at IS NULL;
