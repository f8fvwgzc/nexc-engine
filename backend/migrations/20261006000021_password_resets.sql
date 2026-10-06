-- A way back in for someone who forgot their password. This installation sends no e-mail: a
-- platform administrator issues a link that works once, for an hour, and hands it over.
CREATE TABLE password_resets (
    id uuid PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- SHA-256 of the token. The token itself is shown once, to whoever issued it, and never kept.
    token_hash text NOT NULL UNIQUE,
    created_by uuid REFERENCES users (id) ON DELETE SET NULL,
    expires_at timestamptz NOT NULL,
    used_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX password_resets_user ON password_resets (user_id);
