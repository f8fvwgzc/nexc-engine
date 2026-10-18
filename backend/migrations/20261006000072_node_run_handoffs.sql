-- A node attempt cut short because the replica executing it shut down and handed its run to a
-- peer (docs/HA.md, a rolling update) runs again without counting against NEXC_MAX_ATTEMPTS, so
-- a deploy does not use up a node's attempts. `handoffs` counts those attempts and
-- `handed_off_attempt` is the last one counted: an attempt that waited for an approval across
-- several hand-offs counts once.
ALTER TABLE node_runs
    ADD COLUMN handoffs           integer NOT NULL DEFAULT 0 CHECK (handoffs >= 0),
    ADD COLUMN handed_off_attempt integer NOT NULL DEFAULT 0;
