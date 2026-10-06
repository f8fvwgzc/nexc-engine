-- Topics of a workspace's memory, found without supervision (see knowledge_topics).
-- Their names are drawn only from memories of graphs the whole workspace can see.
CREATE TABLE memory_topics (
    id            uuid PRIMARY KEY,
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    label         text NOT NULL,
    terms         text[] NOT NULL DEFAULT '{}',
    -- The centre of the cluster, little-endian f32s, in the space of memory embeddings.
    centroid      bytea NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX memory_topics_workspace_idx ON memory_topics (workspace_id);

ALTER TABLE memories ADD COLUMN topic_id uuid REFERENCES memory_topics (id) ON DELETE SET NULL;
CREATE INDEX memories_topic_idx ON memories (topic_id) WHERE topic_id IS NOT NULL;
