-- What the platform console does to accounts and workspaces, and a record of it.

-- A suspended account cannot sign in. `session_epoch` is copied into every access token;
-- raising it ends the tokens issued so far (on suspension and when the platform role changes),
-- and `session_epoch_at` says when, so servers only watch the accounts whose old tokens
-- could still be alive.
ALTER TABLE users
    ADD COLUMN suspended_at timestamptz,
    ADD COLUMN suspended_reason text NOT NULL DEFAULT '',
    ADD COLUMN session_epoch integer NOT NULL DEFAULT 0,
    ADD COLUMN session_epoch_at timestamptz;

CREATE INDEX users_session_epoch_at ON users (session_epoch_at) WHERE session_epoch_at IS NOT NULL;

-- Every action a platform administrator takes. The names are copied at the time: the entry
-- outlives the account and the workspace it is about.
CREATE TABLE platform_events (
    id uuid PRIMARY KEY,
    actor_id uuid REFERENCES users (id) ON DELETE SET NULL,
    actor_name text NOT NULL,
    action text NOT NULL,
    subject text NOT NULL,
    detail text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX platform_events_recent ON platform_events (created_at DESC, id DESC);
