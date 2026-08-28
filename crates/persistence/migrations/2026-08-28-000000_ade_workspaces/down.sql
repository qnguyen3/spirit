DROP INDEX IF EXISTS idx_project_worktrees_project_id;
DROP TABLE IF EXISTS project_worktrees;
DROP TABLE IF EXISTS projects;

CREATE TABLE projects (
    path TEXT PRIMARY KEY NOT NULL,
    added_ts TIMESTAMP NOT NULL,
    last_opened_ts TIMESTAMP
);

ALTER TABLE tabs DROP COLUMN project_id;
ALTER TABLE tabs DROP COLUMN worktree_id;
ALTER TABLE tab_groups DROP COLUMN project_id;
ALTER TABLE windows DROP COLUMN active_project_id;
