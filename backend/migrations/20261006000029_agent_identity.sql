-- Agent identity (AgentSecOps phase 1): every agent is a principal with an
-- owner, a team, a purpose and a capabilities document that a per-run token
-- carries to the runtime; data carries a classification the capabilities
-- are checked against; an agent can be quarantined.
ALTER TABLE agents
    ADD COLUMN principal_id   text,
    -- The person answerable for the agent (the creator at first; `owner_id` stays the creator).
    ADD COLUMN owner_user_id  uuid REFERENCES users (id) ON DELETE SET NULL,
    ADD COLUMN team_id        uuid REFERENCES teams (id) ON DELETE SET NULL,
    ADD COLUMN purpose        text NOT NULL DEFAULT '',
    -- `domain::capabilities::Capabilities`: tools, data scopes, egress hosts,
    -- counterparts, delegation limits, model allow-list. `{}` is the default policy.
    ADD COLUMN capabilities   jsonb NOT NULL DEFAULT '{}'::jsonb,
    -- Raised on every change to the capabilities, and on quarantine and release;
    -- a per-run token records the version that governed the run.
    ADD COLUMN policy_version integer NOT NULL DEFAULT 0 CHECK (policy_version >= 0),
    ADD COLUMN quarantined_at timestamptz;

-- `agent://{workspace}/{name}`; an agent that no workspace adopted is named after its creator.
UPDATE agents
   SET principal_id = 'agent://' || COALESCE(workspace_id::text, 'user-' || owner_id::text) || '/' || name,
       owner_user_id = owner_id;
ALTER TABLE agents
    ALTER COLUMN principal_id SET NOT NULL,
    ADD CONSTRAINT agents_principal_id_key UNIQUE (principal_id);

ALTER TABLE agents DROP CONSTRAINT agents_status_check;
ALTER TABLE agents
    ADD CONSTRAINT agents_status_check
    CHECK (status IN ('active', 'paused', 'over_budget', 'quarantined'));
ALTER TABLE agents
    ADD CONSTRAINT agents_quarantined_check
    CHECK ((status = 'quarantined') = (quarantined_at IS NOT NULL));

-- Classification of what agents read. An agent only receives data whose
-- label is in its clearance (`capabilities.data.labels`).
ALTER TABLE documents
    ADD COLUMN label text NOT NULL DEFAULT 'internal'
    CHECK (label IN ('public', 'internal', 'confidential', 'restricted'));
-- `knowledge_topics.label` is the topic's name, so its classification has its own name.
ALTER TABLE knowledge_topics
    ADD COLUMN classification text NOT NULL DEFAULT 'internal'
    CHECK (classification IN ('public', 'internal', 'confidential', 'restricted'));
ALTER TABLE memories
    ADD COLUMN label text NOT NULL DEFAULT 'internal'
    CHECK (label IN ('public', 'internal', 'confidential', 'restricted'));

-- A run in which the runtime's report of an action disagreed with the policy
-- the backend minted for it (`audit_log` action `policy_mismatch` has the detail).
ALTER TABLE runs
    ADD COLUMN policy_mismatches integer NOT NULL DEFAULT 0 CHECK (policy_mismatches >= 0);
