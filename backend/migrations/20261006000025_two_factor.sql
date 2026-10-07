-- Two-factor sign-in with an authenticator app (TOTP, RFC 6238). The secret is sealed with the
-- server's master key. `totp_last_step` is the time step of the last code that was accepted, so
-- a code cannot be used twice. Recovery codes are stored as digests and work once each.
ALTER TABLE users
    ADD COLUMN totp_secret_enc bytea,
    ADD COLUMN totp_enabled_at timestamptz,
    ADD COLUMN totp_last_step bigint;

CREATE TABLE recovery_codes (
    id uuid PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    code_hash text NOT NULL,
    used_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX recovery_codes_user ON recovery_codes (user_id);
