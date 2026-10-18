-- The A2A adapter: an agent an admin exposes serves an A2A agent card and
-- takes tasks over JSON-RPC; a workspace registers remote A2A agents as
-- counterparts, each of them also an external agent principal whose API
-- token is what its inbound calls carry; every inbound task is a one-node
-- run of the exposed agent in a managed graph of its own.

-- Whether the agent is published as an A2A agent (admins; audited).
ALTER TABLE agents ADD COLUMN a2a_exposed boolean NOT NULL DEFAULT false;

-- The managed graph an exposed agent's inbound tasks run in: one node per task.
ALTER TABLE graphs ADD COLUMN a2a_agent_id uuid REFERENCES agents (id) ON DELETE CASCADE;
CREATE INDEX graphs_a2a_agent_idx ON graphs (a2a_agent_id) WHERE a2a_agent_id IS NOT NULL;

-- Remote A2A agents a workspace's agents may call, and that may call its exposed agents.
CREATE TABLE a2a_counterparts (
    id                uuid PRIMARY KEY,
    workspace_id      uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    -- `[a-z0-9_-]{1,64}`: the tool is `a2a:<name>`, and the capabilities' counterpart list names it.
    name              text NOT NULL,
    -- Where its agent card is fetched from.
    card_url          text NOT NULL,
    -- The card as last fetched (`{}` until one was).
    card              jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(card) = 'object'),
    -- The `Authorization` header sent with every task, AES-256-GCM under the master key.
    auth_header       bytea,
    -- Ids of the local agents that may call it; empty means any agent the policy allows.
    allowed_agents    jsonb NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(allowed_agents) = 'array'),
    -- The strictest classification of data that may be sent to it.
    max_label         text NOT NULL DEFAULT 'internal'
                      CHECK (max_label IN ('public', 'internal', 'confidential', 'restricted')),
    -- The external agent principal its inbound calls act as (its API tokens are the credential).
    agent_id          uuid NOT NULL UNIQUE REFERENCES agents (id) ON DELETE CASCADE,
    last_checked_at   timestamptz,
    last_check_ok     boolean,
    last_check_detail text NOT NULL DEFAULT '',
    created_by        uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, name)
);

-- Tasks received over A2A: who called, which run answers it, how deep in a chain it sits.
CREATE TABLE a2a_tasks (
    -- The A2A task id, a UUID v7 (the caller's when it chose one, else minted here).
    id                uuid PRIMARY KEY,
    workspace_id      uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    -- The exposed agent the task was sent to.
    agent_id          uuid NOT NULL REFERENCES agents (id) ON DELETE CASCADE,
    -- The counterpart that called, as the external agent principal its token names.
    caller_agent_id   uuid REFERENCES agents (id) ON DELETE SET NULL,
    caller_principal  text NOT NULL,
    -- The one-node run that answers the task (its node carries the message).
    run_id            uuid NOT NULL REFERENCES runs (id) ON DELETE CASCADE,
    node_id           uuid NOT NULL,
    session_id        text,
    -- A2A hops before this task (0 for a counterpart's own call) and the principals so far.
    hop               integer NOT NULL DEFAULT 0 CHECK (hop >= 0),
    chain             jsonb NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(chain) = 'array'),
    -- The rule and reason when the firewall refused the task (its run failed at once).
    refused_rule      text,
    refused_reason    text,
    -- The message as received, file bytes replaced by the documents they became.
    message           jsonb NOT NULL,
    created_at        timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX a2a_tasks_agent_idx ON a2a_tasks (agent_id, id DESC);
CREATE INDEX a2a_tasks_run_idx ON a2a_tasks (run_id);
CREATE INDEX a2a_tasks_caller_idx ON a2a_tasks (caller_agent_id, id DESC) WHERE caller_agent_id IS NOT NULL;
