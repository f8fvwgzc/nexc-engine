-- A deleted account keeps its row, so that what its owner made in shared workspaces keeps
-- pointing at something, but the row no longer identifies anyone: the name and address are
-- replaced, the password and the sessions are gone. `deleted_at` tells it from a suspension.
ALTER TABLE users ADD COLUMN deleted_at timestamptz;
