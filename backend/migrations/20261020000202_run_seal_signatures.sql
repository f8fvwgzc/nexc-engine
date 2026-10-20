-- A run's seal is signed (docs/AGENTSECOPS.md §12). `seal_signed` is the statement the backend
-- signed with its Ed25519 key when the run ended: canonical JSON of the run's id, its span count,
-- its trace hash, when it was sealed and the key id. `seal_kid` names the key (the RFC 7638
-- thumbprint of its public half, published at /.well-known/jwks.json). Runs sealed before this
-- carry a trace hash without a signature.
ALTER TABLE runs
    ADD COLUMN seal_signed    bytea,
    ADD COLUMN seal_signature bytea CHECK (seal_signature IS NULL OR octet_length(seal_signature) = 64),
    ADD COLUMN seal_kid       text,
    ADD CONSTRAINT runs_seal_whole CHECK (
        (seal_signed IS NULL AND seal_signature IS NULL AND seal_kid IS NULL)
        OR (seal_signed IS NOT NULL AND seal_signature IS NOT NULL AND seal_kid IS NOT NULL
            AND trace_hash IS NOT NULL));
