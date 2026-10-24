-- Room on each page for the next version of a row, so the updates these tables take all day
-- stay HOT (heap-only: no new index entries, the old version pruned in place; docs/DATABASE.md,
-- "Hot updates"). Every one of them changes only columns no index covers:
--   runs:   the lease renewal (`heartbeat_at`, `lease_expires_at`, `lease_secs`, every third of
--           NEXC_RUN_LEASE_SECS per running run, `repo::runs::renew`; 20261024100000 dropped
--           the index on `lease_expires_at`), the token totals after each node
--           (`repo::runs::finish_node`), `cancel_requested`, `policy_mismatches`,
--           `policy_versions`;
--   agents: `heartbeat_at` every 10 s while busy (`repo::agents::heartbeat`), `spent_tokens`
--           after each node (`repo::agents::add_spent`), the risk score after each run
--           (`repo::agents::set_risk`);
--   nodes:  status and output while a run executes, positions on the canvas.
-- New pages keep the room; pages written before stay full until rewritten.
SET LOCAL lock_timeout = '5s';

ALTER TABLE runs SET (fillfactor = 90);
ALTER TABLE agents SET (fillfactor = 90);
ALTER TABLE nodes SET (fillfactor = 90);
