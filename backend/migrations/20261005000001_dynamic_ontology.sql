-- Node and relation types become data: every graph owns an ontology, node and
-- edge kinds are keys into it, and each edge records why it exists.
ALTER TABLE graphs ADD COLUMN ontology jsonb NOT NULL DEFAULT '{}'::jsonb
    CHECK (jsonb_typeof(ontology) = 'object');

ALTER TABLE nodes DROP CONSTRAINT nodes_kind_check;
ALTER TABLE nodes ADD CONSTRAINT nodes_kind_check CHECK (kind ~ '^[a-z][a-z0-9_]{0,39}$');

ALTER TABLE edges DROP CONSTRAINT edges_kind_check;
ALTER TABLE edges ADD CONSTRAINT edges_kind_check CHECK (kind ~ '^[a-z][a-z0-9_]{0,39}$');
ALTER TABLE edges ADD COLUMN blocking boolean NOT NULL DEFAULT false;
ALTER TABLE edges ADD COLUMN reason text NOT NULL DEFAULT '' CHECK (char_length(reason) <= 500);
UPDATE edges SET blocking = (kind = 'depends_on');

-- Types a plan proposes to add to the graph's ontology.
ALTER TABLE plans ADD COLUMN ontology jsonb NOT NULL DEFAULT '{}'::jsonb
    CHECK (jsonb_typeof(ontology) = 'object');
