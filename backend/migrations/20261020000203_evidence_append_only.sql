-- Evidence is append-only in the database itself (docs/AGENTSECOPS.md §12). A span is never
-- updated; an audit entry only has its names erased when its account is (the salt goes, the
-- chained digests stay, the name becomes `Deleted account`, engine::account::DELETED_NAME);
-- neither is ever deleted, except in a transaction that first says it is one of the purge paths:
-- the retention purge, the erasure of a whole workspace, and an erased account's graphs from
-- before workspaces existed. It says so by naming itself, its own transaction id:
--   SELECT set_config('nexc.evidence_purge', pg_current_xact_id()::text, true)   -- deletes
--   SELECT set_config('nexc.evidence_redact', pg_current_xact_id()::text, true)  -- name erasure
-- so a value left on a role, a database or a connection string opens nothing. This keeps
-- mistakes and paths that did not opt in from rewriting evidence; it does not stop the database's
-- owner, who can drop a trigger: the hash chains, the signed seals and evidence bundles exported
-- earlier are what show that.

CREATE FUNCTION evidence_allowed(what text) RETURNS boolean
LANGUAGE sql VOLATILE AS $$
    SELECT COALESCE(current_setting('nexc.evidence_' || what, true), '') = pg_current_xact_id()::text
$$;

CREATE FUNCTION agent_spans_append_only() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP IN ('DELETE', 'TRUNCATE') AND evidence_allowed('purge') THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'agent_spans is append-only: % refused', TG_OP
        USING ERRCODE = 'insufficient_privilege',
              HINT = 'spans leave only through the retention purge (docs/AGENTSECOPS.md §12)';
END $$;
CREATE TRIGGER agent_spans_append_only BEFORE UPDATE OR DELETE ON agent_spans
    FOR EACH ROW EXECUTE FUNCTION agent_spans_append_only();
CREATE TRIGGER agent_spans_no_truncate BEFORE TRUNCATE ON agent_spans
    FOR EACH STATEMENT EXECUTE FUNCTION agent_spans_append_only();

CREATE FUNCTION audit_log_append_only() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP IN ('DELETE', 'TRUNCATE') THEN
        IF evidence_allowed('purge') THEN
            RETURN OLD;
        END IF;
    ELSIF evidence_allowed('redact')
          AND NEW.salt IS NULL
          AND NEW.actor_name IN (OLD.actor_name, 'Deleted account')
          AND NEW.subject IN (OLD.subject, 'Deleted account')
          AND (NEW.id, NEW.workspace_id, NEW.actor_id, NEW.action, NEW.detail, NEW.created_at,
               NEW.seq, NEW.actor_digest, NEW.subject_digest, NEW.prev_hash, NEW.hash)
              IS NOT DISTINCT FROM
              (OLD.id, OLD.workspace_id, OLD.actor_id, OLD.action, OLD.detail, OLD.created_at,
               OLD.seq, OLD.actor_digest, OLD.subject_digest, OLD.prev_hash, OLD.hash) THEN
        -- Only the names change, to the erased name, and the salt goes with them.
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'audit_log is append-only: % refused', TG_OP
        USING ERRCODE = 'insufficient_privilege',
              HINT = 'entries leave only through the retention purge (docs/AGENTSECOPS.md §12)';
END $$;
CREATE TRIGGER audit_log_append_only BEFORE UPDATE OR DELETE ON audit_log
    FOR EACH ROW EXECUTE FUNCTION audit_log_append_only();
CREATE TRIGGER audit_log_no_truncate BEFORE TRUNCATE ON audit_log
    FOR EACH STATEMENT EXECUTE FUNCTION audit_log_append_only();

-- A sealed run's seal is final: its trace hash and its signature cannot be changed afterwards.
CREATE FUNCTION runs_seal_final() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.trace_hash IS NOT NULL
       AND (NEW.trace_hash, NEW.seal_signed, NEW.seal_signature, NEW.seal_kid)
           IS DISTINCT FROM (OLD.trace_hash, OLD.seal_signed, OLD.seal_signature, OLD.seal_kid) THEN
        RAISE EXCEPTION 'the seal of run % is final', OLD.id
            USING ERRCODE = 'insufficient_privilege';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER runs_seal_final
    BEFORE UPDATE OF trace_hash, seal_signed, seal_signature, seal_kid ON runs
    FOR EACH ROW EXECUTE FUNCTION runs_seal_final();
