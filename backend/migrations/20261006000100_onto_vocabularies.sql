-- onto-intelli is the one place graph vocabularies live (docs/ONTOLOGY.md).
--
-- `nexc onto migrate` (also run when the backend starts) copies each graph's stored ontology
-- into onto-intelli once, with SetVocabulary; a graph read before that is initialised the
-- same way on its first read. A graph with a row here that has `migrated_at` is in
-- onto-intelli, and from then on `graphs.ontology` is neither read nor written for it. The
-- column is dropped one release later (docs/ONTOLOGY.md, "Dropping graphs.ontology").
CREATE TABLE onto_vocabulary_migrations (
    graph_id      uuid PRIMARY KEY REFERENCES graphs (id) ON DELETE CASCADE,
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    -- Set once onto-intelli holds the graph's vocabulary.
    migrated_at   timestamptz,
    attempts      integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    -- Why the last attempt failed (onto-intelli unreachable, a type it refused).
    last_error    text,
    updated_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX onto_vocabulary_migrations_pending_idx
    ON onto_vocabulary_migrations (graph_id) WHERE migrated_at IS NULL;
