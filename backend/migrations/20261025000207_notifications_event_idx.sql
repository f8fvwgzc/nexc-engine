-- no-transaction
-- The inbox entries about a comment: for the key's SET NULL when it is deleted (docs/DATABASE.md,
-- "Foreign keys"), and for the entry notify_reaction looks for before it writes another. Entries
-- about no comment (every kind but `reaction`) stay out. Built concurrently (docs/DATABASE.md,
-- "Building indexes"): a run writes notifications inside its lease, so a plain build would hold
-- those runs while it reads every inbox.
CREATE INDEX CONCURRENTLY IF NOT EXISTS notifications_event_id_idx ON notifications (event_id)
    WHERE event_id IS NOT NULL;
