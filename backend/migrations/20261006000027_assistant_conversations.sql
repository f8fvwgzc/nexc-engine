-- Conversations with the workspace assistant are kept, so a member can read them back and
-- continue one. A conversation is personal: its member's, inside one workspace.
CREATE TABLE assistant_conversations (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    title text NOT NULL,
    -- Where in the app the member was when the conversation started.
    page_path text NOT NULL DEFAULT '',
    page_title text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX assistant_conversations_recent_idx
    ON assistant_conversations (workspace_id, user_id, updated_at DESC);

CREATE TABLE assistant_messages (
    id uuid PRIMARY KEY,
    conversation_id uuid NOT NULL REFERENCES assistant_conversations (id) ON DELETE CASCADE,
    role text NOT NULL CHECK (role IN ('user', 'assistant')),
    content text NOT NULL,
    -- For an assistant turn: the issues it filed, what it could not file, memories it used.
    outcome jsonb,
    -- Where the member was when they wrote a user turn.
    page_path text NOT NULL DEFAULT '',
    page_title text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX assistant_messages_conversation_idx
    ON assistant_messages (conversation_id, created_at);
