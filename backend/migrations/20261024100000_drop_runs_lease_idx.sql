-- no-transaction
-- Lease renewals become HOT updates (docs/DATABASE.md, "Hot updates"). The holder of every
-- running run rewrites its `heartbeat_at`, `lease_expires_at` and `lease_secs` every third of
-- NEXC_RUN_LEASE_SECS (`repo::runs::renew`). While `lease_expires_at` was indexed, each renewal
-- put a new entry into every index of runs. The index bought little back: the takeover poll
-- (`repo::runs::take_over`, every dispatcher tick) also takes runs whose lease is NULL, which no
-- range of the index holds, so every poll that found nothing to take over read every running run
-- in it. `runs_active_idx (status, created_at) WHERE status IN ('queued', 'running')` finds the
-- same few rows. Dropped concurrently: every replica writes its leases to runs.
DROP INDEX CONCURRENTLY IF EXISTS runs_lease_idx;
