-- Agent branches (iter3-branch-workflows design D11): the existing branches an agent may name in
-- `create_task`. Existing projects get the branches of dependency bots.
ALTER TABLE projects
    ADD COLUMN agent_branch_globs TEXT[] NOT NULL DEFAULT '{renovate/**,dependabot/**}';
