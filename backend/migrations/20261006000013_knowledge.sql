-- The knowledge base of a workspace: uploaded documents, split into passages
-- that are searched by keywords and by meaning.
CREATE TABLE documents (
    id            uuid PRIMARY KEY,
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name          text NOT NULL,
    size_bytes    bigint NOT NULL,
    -- Of the file's bytes: the same file is stored once per workspace.
    sha256        text NOT NULL,
    status        text NOT NULL DEFAULT 'pending'
                  CHECK (status IN ('pending', 'parsing', 'embedding', 'ready', 'failed')),
    -- Why it failed, in words for the person who uploaded it.
    error         text NOT NULL DEFAULT '',
    page_count    integer,
    chunk_count   integer NOT NULL DEFAULT 0,
    uploaded_by   uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, sha256)
);
CREATE INDEX documents_workspace_idx ON documents (workspace_id, created_at DESC);
CREATE INDEX documents_unfinished_idx ON documents (status) WHERE status NOT IN ('ready', 'failed');

CREATE TABLE document_chunks (
    id               uuid PRIMARY KEY,
    document_id      uuid NOT NULL REFERENCES documents (id) ON DELETE CASCADE,
    -- Copied from the document so a search is one index scan, without a join.
    workspace_id     uuid NOT NULL,
    ordinal          integer NOT NULL,
    page             integer,
    -- The headings above the passage: "Report › Revenue › By region".
    section_path     text NOT NULL DEFAULT '',
    kind             text NOT NULL CHECK (kind IN ('text', 'table')),
    content          text NOT NULL,
    content_tsv      tsvector GENERATED ALWAYS AS
                     (to_tsvector('simple', section_path || ' ' || content)) STORED,
    -- Little-endian f32s; NULL until the passage is embedded.
    embedding        bytea,
    -- The model that produced `embedding`: vectors of different models do not compare.
    embedding_model  text,
    UNIQUE (document_id, ordinal)
);
CREATE INDEX document_chunks_tsv_idx ON document_chunks USING gin (content_tsv);
CREATE INDEX document_chunks_workspace_idx ON document_chunks (workspace_id);

-- How a workspace embeds and uses its documents. No row means the defaults.
CREATE TABLE workspace_knowledge_settings (
    workspace_id    uuid PRIMARY KEY REFERENCES workspaces (id) ON DELETE CASCADE,
    -- An OpenAI-compatible embeddings endpoint; NULL uses the server's, else the built-in one.
    embed_base_url  text,
    embed_model     text,
    embed_dims      integer,
    api_key_enc     bytea,
    key_hint        text,
    -- Passages given to a node or to the planner, and the characters they may take together.
    passages        integer NOT NULL DEFAULT 5 CHECK (passages BETWEEN 0 AND 20),
    budget_chars    integer NOT NULL DEFAULT 6000 CHECK (budget_chars BETWEEN 500 AND 40000),
    use_in_nodes    boolean NOT NULL DEFAULT true,
    use_in_plan     boolean NOT NULL DEFAULT true,
    updated_by      uuid REFERENCES users (id) ON DELETE SET NULL,
    updated_at      timestamptz NOT NULL DEFAULT now()
);
