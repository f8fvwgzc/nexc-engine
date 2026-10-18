-- Idempotency keys (docs/CONTRACT.md §3): a client that may retry a write
-- sends `Idempotency-Key` with it. The first request with a key is handled
-- and its response kept here; a retry with the same key and the same body
-- gets that response again instead of a second run, issue, node or
-- decision. Keys belong to an account and to the method and path they were
-- sent to, and a housekeeping job deletes them after 24 hours.
CREATE TABLE idempotency_keys (
    user_id         uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- The method and path the key was sent with (`POST /api/v1/graphs/<id>/runs`).
    route           text NOT NULL,
    key             text NOT NULL CHECK (char_length(key) BETWEEN 1 AND 255),
    -- SHA-256 of the request body (canonical JSON when it is JSON).
    request_hash    bytea NOT NULL CHECK (octet_length(request_hash) = 32),
    -- The first response, once there is one: NULL while the first request is handled.
    response_status smallint,
    response_body   bytea,
    response_type   text,
    created_at      timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, route, key)
);
CREATE INDEX idempotency_keys_created_idx ON idempotency_keys (created_at);
