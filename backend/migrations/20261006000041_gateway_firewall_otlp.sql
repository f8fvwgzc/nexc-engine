-- gRPC, the tools gateway, the Agent Firewall API and OTLP export (AgentSecOps
-- phase 4): a workspace registers the MCP servers its agents may call through
-- the backend; an external agent holds an API token for the firewall API; the
-- OTLP export keeps a cursor over the trace spine.

-- The MCP servers of a workspace. The backend connects to them on an agent's
-- behalf (never the runtime); a tool of a server is offered to agents as
-- `mcp:<name>/<tool>` under the policy.
CREATE TABLE mcp_servers (
    id                uuid PRIMARY KEY,
    workspace_id      uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name              text NOT NULL,
    -- `stdio`: a child process the backend starts for each call; `http`: streamable HTTP.
    transport         text NOT NULL CHECK (transport IN ('stdio', 'http')),
    command           text,
    args              jsonb NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(args) = 'array'),
    url               text,
    -- The `Authorization` header sent to an HTTP server, AES-256-GCM under the master key.
    auth_header       bytea,
    -- Tools of the server agents may call; empty means every tool it lists.
    allowed_tools     jsonb NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(allowed_tools) = 'array'),
    -- The strictest classification of data that may be sent to the server.
    max_label         text NOT NULL DEFAULT 'internal'
                      CHECK (max_label IN ('public', 'internal', 'confidential', 'restricted')),
    -- What the last connection test found: the tools the server lists.
    tools             jsonb NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(tools) = 'array'),
    last_checked_at   timestamptz,
    last_check_ok     boolean,
    last_check_detail text NOT NULL DEFAULT '',
    created_by        uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, name),
    CHECK ((transport = 'stdio') = (command IS NOT NULL)),
    CHECK ((transport = 'http') = (url IS NOT NULL))
);

-- An agent that runs somewhere else and uses nexc as its policy and trace
-- service through the Agent Firewall API.
ALTER TABLE agents DROP CONSTRAINT agents_runtime_check;
ALTER TABLE agents
    ADD CONSTRAINT agents_runtime_check CHECK (runtime IN ('python', 'builtin', 'external'));

-- API tokens of external agents: shown once, kept as a hash.
CREATE TABLE agent_api_tokens (
    id            uuid PRIMARY KEY,
    agent_id      uuid NOT NULL REFERENCES agents (id) ON DELETE CASCADE,
    -- SHA-256 of the token.
    token_hash    bytea NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    -- The first characters, to tell tokens apart in the console.
    prefix        text NOT NULL,
    name          text NOT NULL DEFAULT '',
    created_by    uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    last_used_at  timestamptz,
    revoked_at    timestamptz
);
CREATE INDEX agent_api_tokens_agent_idx ON agent_api_tokens (agent_id, created_at DESC);

-- Runs of an external agent live in a managed graph of its workspace, with
-- one node per run; the node keeps the platform's own run id.
ALTER TABLE graphs ADD COLUMN external_agent_id uuid REFERENCES agents (id) ON DELETE CASCADE;
CREATE INDEX graphs_external_agent_idx ON graphs (external_agent_id) WHERE external_agent_id IS NOT NULL;
ALTER TABLE nodes ADD COLUMN external_run_id text;

-- Where the OTLP export stands: every span with a lower or equal insertion
-- sequence went out. One row; instances take it with SKIP LOCKED.
CREATE TABLE otlp_export_cursor (
    id                integer PRIMARY KEY CHECK (id = 1),
    span_seq          bigint NOT NULL DEFAULT 0 CHECK (span_seq >= 0),
    spans_exported    bigint NOT NULL DEFAULT 0,
    last_exported_at  timestamptz,
    last_error        text,
    updated_at        timestamptz NOT NULL DEFAULT now()
);
INSERT INTO otlp_export_cursor (id) VALUES (1);
