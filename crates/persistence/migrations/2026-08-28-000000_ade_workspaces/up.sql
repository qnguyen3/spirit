DROP TABLE IF EXISTS projects;

CREATE TABLE projects (
    id TEXT PRIMARY KEY NOT NULL,
    root_path TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    kind TEXT NOT NULL,
    primary_branch TEXT,
    created_ts BIGINT NOT NULL,
    last_opened_ts BIGINT NOT NULL
);

CREATE TABLE project_worktrees (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    path TEXT,
    branch TEXT,
    base_branch TEXT,
    created_ts BIGINT NOT NULL
);

CREATE INDEX idx_project_worktrees_project_id ON project_worktrees(project_id);

ALTER TABLE tabs ADD COLUMN project_id TEXT;
ALTER TABLE tabs ADD COLUMN worktree_id TEXT;
ALTER TABLE tab_groups ADD COLUMN project_id TEXT;
ALTER TABLE windows ADD COLUMN active_project_id TEXT;
