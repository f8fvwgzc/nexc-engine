-- Single sign-on with an OpenID Connect provider (NEXC_OIDC_*).

-- Who an account is at a provider: the (issuer, subject) pair the provider's ID tokens carry,
-- which never changes, unlike the e-mail address. An account has at most one identity per
-- issuer; an identity belongs to one account.
CREATE TABLE user_identities (
    id            uuid PRIMARY KEY,
    user_id       uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    issuer        text NOT NULL,
    subject       text NOT NULL,
    -- The address the provider last gave, as it gave it (the account's own address is not
    -- changed by a sign-in).
    email         text NOT NULL DEFAULT '',
    created_at    timestamptz NOT NULL DEFAULT now(),
    last_login_at timestamptz,
    UNIQUE (issuer, subject),
    UNIQUE (user_id, issuer)
);

-- Sign-ins in flight: one row per authorization request, taken (deleted) by the callback, so a
-- state works once, and for ten minutes. Only the digests of the state and of the browser's
-- binding cookie are kept; the nonce and the PKCE verifier are the server's own.
CREATE TABLE oidc_logins (
    state_hash    text PRIMARY KEY,
    browser_hash  text NOT NULL,
    nonce         text NOT NULL,
    code_verifier text NOT NULL,
    -- The same-origin path the browser goes to once signed in.
    return_to     text NOT NULL,
    -- Set when the account's holder started the sign-in from their account page to link the
    -- provider's identity to it (POST /auth/oidc/link); NULL: the account is found by identity,
    -- or by verified address.
    link_user_id  uuid REFERENCES users (id) ON DELETE CASCADE,
    expires_at    timestamptz NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX oidc_logins_expiry ON oidc_logins (expires_at);

-- Workspace roles the group mapping (NEXC_OIDC_WORKSPACE_ROLES) gave, so that a sign-in without
-- the group takes back what the mapping gave and nothing else. `prior_role` is the role the
-- account had before (NULL: it was not a member), which taking the grant back restores.
CREATE TABLE sso_role_grants (
    user_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    role         text NOT NULL CHECK (role IN ('owner', 'admin', 'member', 'guest')),
    prior_role   text CHECK (prior_role IN ('owner', 'admin', 'member', 'guest')),
    created_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, workspace_id)
);
CREATE INDEX sso_role_grants_workspace ON sso_role_grants (workspace_id);

-- The platform role came from NEXC_OIDC_ADMIN_GROUPS: a sign-in without the group takes it back.
-- One given in the platform console is never touched by a sign-in.
ALTER TABLE users ADD COLUMN admin_by_sso boolean NOT NULL DEFAULT false;
