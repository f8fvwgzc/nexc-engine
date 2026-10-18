-- Evidence-bound outputs (docs/AGENTSECOPS.md §2): what the verifier found about a node's
-- output once it finished: the verdict, the coverage, the ids it cited and the ones it forged.
-- Null until the node answered, and for a node that produced no output.
ALTER TABLE node_runs
    ADD COLUMN evidence jsonb CHECK (evidence IS NULL OR jsonb_typeof(evidence) = 'object');

-- The console counts the day's outputs by verdict.
CREATE INDEX node_runs_evidence_idx ON node_runs ((evidence->>'verdict'), finished_at)
    WHERE evidence IS NOT NULL;
