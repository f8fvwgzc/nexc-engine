-- What happened to an account's access, for its holder to read: sign-ins, password changes,
-- reset links issued for it, and what a platform administrator did to it. Kept for 180 days.
CREATE TABLE account_events (
    id uuid PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    kind text NOT NULL,
    -- Where the request came from, when it came from the account's holder.
    ip text,
    detail text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX account_events_user ON account_events (user_id, created_at DESC, id DESC);
CREATE INDEX account_events_age ON account_events (created_at);
