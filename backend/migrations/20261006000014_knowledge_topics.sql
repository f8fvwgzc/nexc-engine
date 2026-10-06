-- Topics of a workspace's documents, found without supervision: passages are
-- clustered by their embeddings and each cluster is named by the words that
-- set it apart.
CREATE TABLE knowledge_topics (
    id            uuid PRIMARY KEY,
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    -- The few words that distinguish the topic: "customs · invoices · port".
    label         text NOT NULL,
    terms         text[] NOT NULL DEFAULT '{}',
    -- The centre of the cluster, little-endian f32s, in the space of `embedding_model`.
    centroid      bytea NOT NULL,
    embedding_model text NOT NULL,
    chunk_count   integer NOT NULL DEFAULT 0,
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX knowledge_topics_workspace_idx ON knowledge_topics (workspace_id);

ALTER TABLE document_chunks
    ADD COLUMN topic_id uuid REFERENCES knowledge_topics (id) ON DELETE SET NULL;
CREATE INDEX document_chunks_topic_idx ON document_chunks (topic_id) WHERE topic_id IS NOT NULL;
