-- An issue can be part of another one. Deleting the parent leaves its parts as
-- issues of their own.
ALTER TABLE issues ADD COLUMN parent_id uuid REFERENCES issues (id) ON DELETE SET NULL;
CREATE INDEX issues_parent_idx ON issues (parent_id) WHERE parent_id IS NOT NULL;
