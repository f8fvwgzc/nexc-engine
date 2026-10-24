-- no-transaction
-- The artifacts the DLP screen kept in and nobody released, on the AgentSecOps console
-- (`repo::secops::artifacts_quarantined`, `WHERE a.quarantined_at IS NOT NULL AND a.released_at IS
-- NULL`), which read every artifact of the workspace's runs to count the few still held. The
-- partial index holds only those: a release takes the row out. Built concurrently
-- (docs/DATABASE.md, "Migrations"): every node writes its artifacts inside its run's lease
-- (engine::artifacts), so a plain build would hold those runs while it reads the table.
CREATE INDEX CONCURRENTLY IF NOT EXISTS artifacts_quarantined_idx ON artifacts (run_id)
    WHERE quarantined_at IS NOT NULL AND released_at IS NULL;
