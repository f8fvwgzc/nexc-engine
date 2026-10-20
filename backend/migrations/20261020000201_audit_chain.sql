-- A workspace's audit log is a hash chain (docs/AGENTSECOPS.md §12). Each entry takes the next
-- position (`seq`) in its workspace's chain, and
--   hash = SHA-256('nexc-audit-v1' || prev_hash || workspace_id || seq || id || created_at
--                  || actor_id || actor_digest || action || subject_digest || detail)
-- with 32 zero bytes before the first entry; `audit_entry_hash` has the exact byte layout and the
-- backend's `domain::audit_chain` recomputes it. Names are covered by salted digests,
--   actor_digest = SHA-256(salt || actor_name), subject_digest = SHA-256(salt || subject),
-- so erasing an account can still take its name out of the log: the digests stay, the salt goes
-- (a known name can no longer be tested against the digest), and the chain verifies as before.
-- The database computes the chain itself, under a lock on the workspace's chain head, so no
-- writer can skip it, and the head is what a removed tail is found against.

CREATE TABLE audit_chain_heads (
    workspace_id uuid PRIMARY KEY REFERENCES workspaces (id) ON DELETE CASCADE,
    -- The last entry's position and hash: -1 and 32 zero bytes before the first entry.
    seq          bigint NOT NULL DEFAULT -1,
    hash         bytea NOT NULL DEFAULT decode(repeat('00', 32), 'hex')
                 CHECK (octet_length(hash) = 32)
);

ALTER TABLE audit_log
    ADD COLUMN seq            bigint,
    -- 16 random bytes; NULL once the entry's names were erased.
    ADD COLUMN salt           bytea CHECK (salt IS NULL OR octet_length(salt) = 16),
    ADD COLUMN actor_digest   bytea CHECK (octet_length(actor_digest) = 32),
    ADD COLUMN subject_digest bytea CHECK (octet_length(subject_digest) = 32),
    ADD COLUMN prev_hash      bytea CHECK (octet_length(prev_hash) = 32),
    ADD COLUMN hash           bytea CHECK (octet_length(hash) = 32);

CREATE FUNCTION audit_text_digest(salt bytea, value text) RETURNS bytea
LANGUAGE sql STABLE AS $$ SELECT sha256(salt || convert_to(value, 'UTF8')) $$;

-- A text field as hashed: its length in UTF-8 bytes (4 bytes, big-endian), then the bytes.
CREATE FUNCTION audit_framed(value text) RETURNS bytea
LANGUAGE sql STABLE AS $$
    SELECT int4send(octet_length(convert_to(value, 'UTF8'))) || convert_to(value, 'UTF8')
$$;

-- uuids are their 16 bytes, seq is 8 bytes big-endian, created_at is the 8-byte big-endian
-- count of microseconds since 2000-01-01 UTC (PostgreSQL's own), actor_id is 0x00 when NULL
-- and 0x01 followed by its 16 bytes otherwise. The fields are passed one by one, not as an
-- `audit_log` row, so nothing here depends on the table's row type (a restore can drop it).
CREATE FUNCTION audit_entry_hash(
    prev bytea, workspace_id uuid, seq bigint, id uuid, created_at timestamptz, actor_id uuid,
    actor_digest bytea, action text, subject_digest bytea, detail text
) RETURNS bytea
LANGUAGE sql STABLE AS $$
    SELECT sha256(
        convert_to('nexc-audit-v1', 'UTF8')
        || prev
        || uuid_send(workspace_id)
        || int8send(seq)
        || uuid_send(id)
        || timestamptz_send(created_at)
        || CASE WHEN actor_id IS NULL THEN '\x00'::bytea
                ELSE '\x01'::bytea || uuid_send(actor_id) END
        || actor_digest
        || audit_framed(action)
        || subject_digest
        || audit_framed(detail))
$$;

-- The entries written so far are chained in the order they were written, one update each
-- (about 2.5 s per 100,000 entries on a laptop; the table is locked meanwhile).
DO $$
DECLARE
    e         record;
    ws        uuid;
    prev      bytea;
    n         bigint;
    salted    bytea;
    a_digest  bytea;
    s_digest  bytea;
    h         bytea;
BEGIN
    FOR e IN SELECT id, workspace_id, created_at, actor_id, actor_name, action, subject, detail
               FROM audit_log ORDER BY workspace_id, created_at, id LOOP
        IF ws IS DISTINCT FROM e.workspace_id THEN
            IF ws IS NOT NULL THEN
                INSERT INTO audit_chain_heads (workspace_id, seq, hash) VALUES (ws, n, prev);
            END IF;
            ws := e.workspace_id;
            prev := decode(repeat('00', 32), 'hex');
            n := -1;
        END IF;
        n := n + 1;
        salted := uuid_send(gen_random_uuid());
        a_digest := audit_text_digest(salted, e.actor_name);
        s_digest := audit_text_digest(salted, e.subject);
        h := audit_entry_hash(prev, e.workspace_id, n, e.id, e.created_at, e.actor_id, a_digest,
                              e.action, s_digest, e.detail);
        UPDATE audit_log
           SET seq = n, salt = salted, actor_digest = a_digest, subject_digest = s_digest,
               prev_hash = prev, hash = h
         WHERE id = e.id;
        prev := h;
    END LOOP;
    IF ws IS NOT NULL THEN
        INSERT INTO audit_chain_heads (workspace_id, seq, hash) VALUES (ws, n, prev);
    END IF;
END $$;

ALTER TABLE audit_log
    ALTER COLUMN seq SET NOT NULL,
    ALTER COLUMN actor_digest SET NOT NULL,
    ALTER COLUMN subject_digest SET NOT NULL,
    ALTER COLUMN prev_hash SET NOT NULL,
    ALTER COLUMN hash SET NOT NULL,
    ADD CONSTRAINT audit_log_chain_key UNIQUE (workspace_id, seq);

-- Every new entry is chained here, whoever writes it. The digests are computed from the salt and
-- the names, except for an entry whose names were already erased (no salt, digests given): a
-- workspace transfer copies it as it is. An entry that is already there is skipped before it
-- takes a position: a retried transfer's `ON CONFLICT DO NOTHING` would otherwise run this
-- trigger, move the head, and then drop the row. A head that went missing (a partial restore, a
-- manual cleanup) is taken up again from the workspace's last entry rather than restarting at 0,
-- which every later entry would collide with.
CREATE FUNCTION audit_log_chain() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    head_seq   bigint;
    head_hash  bytea;
BEGIN
    INSERT INTO audit_chain_heads (workspace_id) VALUES (NEW.workspace_id) ON CONFLICT DO NOTHING;
    SELECT seq, hash INTO head_seq, head_hash
      FROM audit_chain_heads WHERE workspace_id = NEW.workspace_id FOR UPDATE;
    -- Under the head's lock, so that a copy written meanwhile by another transaction is seen.
    IF EXISTS (SELECT 1 FROM audit_log WHERE id = NEW.id) THEN
        RETURN NULL;
    END IF;
    IF head_seq < 0 THEN
        SELECT a.seq, a.hash INTO head_seq, head_hash
          FROM audit_log a WHERE a.workspace_id = NEW.workspace_id ORDER BY a.seq DESC LIMIT 1;
        IF NOT FOUND THEN
            head_seq := -1;
            head_hash := decode(repeat('00', 32), 'hex');
        END IF;
    END IF;
    IF NEW.salt IS NOT NULL OR NEW.actor_digest IS NULL OR NEW.subject_digest IS NULL THEN
        NEW.salt := COALESCE(NEW.salt, uuid_send(gen_random_uuid()));
        NEW.actor_digest := audit_text_digest(NEW.salt, NEW.actor_name);
        NEW.subject_digest := audit_text_digest(NEW.salt, NEW.subject);
    END IF;
    NEW.seq := head_seq + 1;
    NEW.prev_hash := head_hash;
    NEW.hash := audit_entry_hash(NEW.prev_hash, NEW.workspace_id, NEW.seq, NEW.id, NEW.created_at,
                                 NEW.actor_id, NEW.actor_digest, NEW.action, NEW.subject_digest,
                                 NEW.detail);
    UPDATE audit_chain_heads SET seq = NEW.seq, hash = NEW.hash
     WHERE workspace_id = NEW.workspace_id;
    RETURN NEW;
END $$;
CREATE TRIGGER audit_log_chain BEFORE INSERT ON audit_log
    FOR EACH ROW EXECUTE FUNCTION audit_log_chain();
