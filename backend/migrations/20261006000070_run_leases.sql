-- Run leases (docs/HA.md). The instance executing a run holds a lease on
-- it: `claimed_by` names the instance, `lease_expires_at` is when the lease
-- runs out unless renewed (every third of NEXC_RUN_LEASE_SECS), and
-- `lease_epoch` goes up every time the run changes hands (a claim, a
-- takeover, a release). A run whose lease ran out is taken over by another
-- instance with one UPDATE that raises the epoch; every write the executor
-- makes for the run first locks the run row at the epoch it holds, so a
-- former holder's write is refused (fencing).
ALTER TABLE runs
    ADD COLUMN lease_epoch      bigint NOT NULL DEFAULT 0 CHECK (lease_epoch >= 0),
    ADD COLUMN lease_expires_at timestamptz;

-- A run executing while this migration ran keeps its holder: the lease runs
-- out a while after its last heartbeat, and an instance of an earlier version
-- that still sends heartbeats is never taken over (the takeover also wants
-- the heartbeat to be older than a lease).
UPDATE runs SET lease_epoch = 1,
                lease_expires_at = COALESCE(heartbeat_at, now()) + interval '30 seconds'
WHERE status = 'running' AND claimed_by IS NOT NULL;

CREATE INDEX runs_lease_idx ON runs (lease_expires_at) WHERE status = 'running';

-- A command is addressed to whoever holds the run: the epoch when it was
-- recorded says whose attempt it is about. The holder that took the run
-- over takes the commands its former holder never took.
ALTER TABLE run_commands ADD COLUMN lease_epoch bigint NOT NULL DEFAULT 0;

-- What happened to each lease, for the operator: claimed, taken over from a
-- holder that stopped renewing, released by a holder that shut down, lost
-- (its holder found the epoch moved) and fenced (a former holder's write was
-- refused). One row per event; the run's rows go with it.
CREATE TABLE run_lease_events (
    id         uuid PRIMARY KEY,
    run_id     uuid NOT NULL REFERENCES runs (id) ON DELETE CASCADE,
    -- The epoch the event is about: the new one for a claim or a takeover,
    -- the one held for a release, a loss or a refused write.
    epoch      bigint NOT NULL CHECK (epoch >= 0),
    instance   uuid NOT NULL,
    kind       text NOT NULL CHECK (kind IN ('claimed', 'taken_over', 'released', 'lost', 'fenced')),
    -- The instance that held the run before a takeover.
    previous   uuid,
    detail     text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX run_lease_events_run_idx ON run_lease_events (run_id, created_at);
