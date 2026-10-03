-- nexc-engine schema (PostgreSQL 17). Enumerations are TEXT with CHECK
-- constraints so that the wire spelling and the stored spelling are identical.

CREATE TABLE users (
    id             uuid PRIMARY KEY,
    email          text NOT NULL,
    name           text NOT NULL,
    role           text NOT NULL CHECK (role IN ('admin', 'user')),
    password_hash  text NOT NULL,
    failed_logins  integer NOT NULL DEFAULT 0,
    locked_until   timestamptz,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX users_email_key ON users (lower(email));

CREATE TABLE refresh_tokens (
    id          uuid PRIMARY KEY,
    user_id     uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    family_id   uuid NOT NULL,
    token_hash  text NOT NULL UNIQUE,
    expires_at  timestamptz NOT NULL,
    used_at     timestamptz,
    revoked_at  timestamptz,
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX refresh_tokens_user_idx ON refresh_tokens (user_id);
CREATE INDEX refresh_tokens_family_idx ON refresh_tokens (family_id);

CREATE TABLE llm_settings (
    user_id      uuid PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    provider     text NOT NULL CHECK (provider IN ('anthropic', 'openai_compatible', 'demo')),
    model        text NOT NULL,
    base_url     text,
    api_key_enc  bytea,
    key_hint     text,
    updated_at   timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE graphs (
    id           uuid PRIMARY KEY,
    owner_id     uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name         text NOT NULL,
    description  text NOT NULL DEFAULT '',
    goal         text NOT NULL DEFAULT '',
    version      bigint NOT NULL DEFAULT 1,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX graphs_owner_idx ON graphs (owner_id, updated_at DESC);

CREATE TABLE nodes (
    id          uuid PRIMARY KEY,
    graph_id    uuid NOT NULL REFERENCES graphs (id) ON DELETE CASCADE,
    title       text NOT NULL,
    content     text NOT NULL DEFAULT '',
    kind        text NOT NULL CHECK (kind IN ('topic', 'task', 'research', 'code', 'document', 'output')),
    tags        jsonb NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(tags) = 'array'),
    x           double precision NOT NULL DEFAULT 0,
    y           double precision NOT NULL DEFAULT 0,
    status      text NOT NULL DEFAULT 'idle'
                CHECK (status IN ('idle', 'queued', 'running', 'succeeded', 'failed', 'skipped', 'cancelled')),
    agent_role  text,
    executor    text NOT NULL DEFAULT 'llm' CHECK (executor IN ('llm', 'agent', 'symphony')),
    output      text,
    origin      text NOT NULL DEFAULT 'user' CHECK (origin IN ('user', 'plan')),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX nodes_graph_idx ON nodes (graph_id, created_at);

CREATE TABLE edges (
    id          uuid PRIMARY KEY,
    graph_id    uuid NOT NULL REFERENCES graphs (id) ON DELETE CASCADE,
    source      uuid NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    target      uuid NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    kind        text NOT NULL CHECK (kind IN ('depends_on', 'relates_to')),
    origin      text NOT NULL CHECK (origin IN ('user', 'auto', 'plan')),
    weight      double precision NOT NULL DEFAULT 1,
    created_at  timestamptz NOT NULL DEFAULT now(),
    CHECK (source <> target),
    UNIQUE (source, target, kind)
);
CREATE INDEX edges_graph_idx ON edges (graph_id);
CREATE INDEX edges_target_idx ON edges (target);

CREATE TABLE plans (
    id            uuid PRIMARY KEY,
    graph_id      uuid NOT NULL REFERENCES graphs (id) ON DELETE CASCADE,
    owner_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    status        text NOT NULL CHECK (status IN ('streaming', 'ready', 'failed', 'applied')),
    instructions  text NOT NULL DEFAULT '',
    summary       text NOT NULL DEFAULT '',
    nodes         jsonb NOT NULL DEFAULT '[]'::jsonb,
    edges         jsonb NOT NULL DEFAULT '[]'::jsonb,
    error         text,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX plans_graph_idx ON plans (graph_id, created_at DESC);

CREATE TABLE runs (
    id                uuid PRIMARY KEY,
    graph_id          uuid NOT NULL REFERENCES graphs (id) ON DELETE CASCADE,
    owner_id          uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    status            text NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
    tokens_in         bigint NOT NULL DEFAULT 0,
    tokens_out        bigint NOT NULL DEFAULT 0,
    cost_usd          double precision NOT NULL DEFAULT 0,
    max_concurrency   integer NOT NULL CHECK (max_concurrency BETWEEN 1 AND 32),
    force             boolean NOT NULL DEFAULT false,
    cancel_requested  boolean NOT NULL DEFAULT false,
    claimed_by        uuid,
    heartbeat_at      timestamptz,
    error             text,
    started_at        timestamptz,
    finished_at       timestamptz,
    created_at        timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX runs_graph_idx ON runs (graph_id, created_at DESC);
CREATE INDEX runs_active_idx ON runs (status, created_at) WHERE status IN ('queued', 'running');

CREATE TABLE node_runs (
    run_id        uuid NOT NULL REFERENCES runs (id) ON DELETE CASCADE,
    node_id       uuid NOT NULL,
    position      integer NOT NULL,
    status        text NOT NULL
                  CHECK (status IN ('idle', 'queued', 'running', 'succeeded', 'failed', 'skipped', 'cancelled')),
    attempt       integer NOT NULL DEFAULT 0,
    executor      text NOT NULL CHECK (executor IN ('llm', 'agent', 'symphony')),
    tokens_in     bigint NOT NULL DEFAULT 0,
    tokens_out    bigint NOT NULL DEFAULT 0,
    cost_usd      double precision NOT NULL DEFAULT 0,
    cached        boolean NOT NULL DEFAULT false,
    error         text,
    output        text,
    content_hash  text,
    agent_id      uuid,
    started_at    timestamptz,
    finished_at   timestamptz,
    PRIMARY KEY (run_id, node_id)
);
CREATE INDEX node_runs_hash_idx ON node_runs (content_hash) WHERE status = 'succeeded';

CREATE TABLE artifacts (
    id            uuid PRIMARY KEY,
    run_id        uuid NOT NULL REFERENCES runs (id) ON DELETE CASCADE,
    node_id       uuid NOT NULL,
    path          text NOT NULL,
    size          bigint NOT NULL CHECK (size >= 0),
    mime          text NOT NULL,
    storage_path  text NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (run_id, node_id, path)
);
CREATE INDEX artifacts_run_idx ON artifacts (run_id);

CREATE TABLE agents (
    id             uuid PRIMARY KEY,
    owner_id       uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name           text NOT NULL,
    role           text NOT NULL,
    title          text NOT NULL DEFAULT '',
    model          text NOT NULL,
    system_prompt  text NOT NULL DEFAULT '',
    reports_to     uuid REFERENCES agents (id) ON DELETE SET NULL,
    budget_tokens  bigint NOT NULL DEFAULT 0 CHECK (budget_tokens >= 0),
    spent_tokens   bigint NOT NULL DEFAULT 0 CHECK (spent_tokens >= 0),
    status         text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'paused', 'over_budget')),
    runtime        text NOT NULL DEFAULT 'builtin' CHECK (runtime IN ('python', 'builtin')),
    heartbeat_at   timestamptz,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (owner_id, name)
);
CREATE INDEX agents_owner_role_idx ON agents (owner_id, role);

CREATE TABLE memories (
    id                uuid PRIMARY KEY,
    owner_id          uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    scope             text NOT NULL CHECK (scope IN ('user', 'graph', 'node')),
    graph_id          uuid REFERENCES graphs (id) ON DELETE CASCADE,
    node_id           uuid,
    kind              text NOT NULL CHECK (kind IN ('fact', 'experience', 'observation', 'preference')),
    content           text NOT NULL,
    -- 256 little-endian f32 values (feature-hashing embedding from the C kernel).
    embedding         bytea NOT NULL CHECK (octet_length(embedding) = 1024),
    importance        double precision NOT NULL DEFAULT 0.5 CHECK (importance BETWEEN 0 AND 1),
    access_count      bigint NOT NULL DEFAULT 0,
    last_accessed_at  timestamptz,
    content_tsv       tsvector GENERATED ALWAYS AS (to_tsvector('simple', content)) STORED,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    CHECK (scope <> 'graph' OR graph_id IS NOT NULL),
    CHECK (scope <> 'node' OR node_id IS NOT NULL)
);
CREATE INDEX memories_owner_idx ON memories (owner_id, graph_id, updated_at DESC);
CREATE INDEX memories_tsv_idx ON memories USING gin (content_tsv);

CREATE TABLE realtime_tickets (
    ticket_hash  text PRIMARY KEY,
    user_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    graph_id     uuid NOT NULL REFERENCES graphs (id) ON DELETE CASCADE,
    expires_at   timestamptz NOT NULL
);
CREATE INDEX realtime_tickets_expiry_idx ON realtime_tickets (expires_at);

-- Realtime events too large for a NOTIFY payload (8000 bytes) are parked here
-- and referenced by id; rows older than a few minutes are purged.
CREATE TABLE realtime_outbox (
    id          bigserial PRIMARY KEY,
    payload     jsonb NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now()
);
