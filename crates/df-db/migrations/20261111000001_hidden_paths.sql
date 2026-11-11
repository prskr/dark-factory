-- Hidden paths of a project (iter3-hidden-paths design D1): `globset` globs without `{}`,
-- validated when saved. A project without a row, and every existing row, hides nothing.
ALTER TABLE project_push_policies
    ADD COLUMN hidden_globs TEXT[] NOT NULL DEFAULT '{}';
