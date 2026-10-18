-- The lease the holder of a running run works under (the NEXC_RUN_LEASE_SECS of the replica that
-- claimed, took over or renewed it). A takeover waits for the holder's lease rather than its own,
-- so replicas whose setting differs still take a dead peer's runs over a lease after it stopped,
-- and an operator sees the difference. NULL: claimed before this column existed.
ALTER TABLE runs ADD COLUMN lease_secs double precision CHECK (lease_secs > 0);
