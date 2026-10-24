-- Every task received over A2A is answered by a one-node run of its own (repo::a2a), and the
-- code reads a run's task as the only one (`repo::a2a::hops_of_run`, `WHERE run_id = $1 LIMIT
-- 1`: the hop count and the chain that bound delegation). The run id becomes unique, and the
-- unique index takes over from `a2a_tasks_run_idx` as the index of the foreign key to runs.
--
-- Created only when no run already answers two tasks, with a warning otherwise; the plain index
-- then stays (docs/DATABASE.md, "Invariants").
SET LOCAL lock_timeout = '5s';

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM a2a_tasks GROUP BY run_id HAVING count(*) > 1) THEN
        CREATE UNIQUE INDEX IF NOT EXISTS a2a_tasks_run_key ON a2a_tasks (run_id);
        DROP INDEX IF EXISTS a2a_tasks_run_idx;
    ELSE
        RAISE WARNING 'a2a_tasks_run_key not created: a run answers more than one A2A task';
    END IF;
END $$;
