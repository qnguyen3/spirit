-- Warp Drive, Teams, login, and server experiments were removed from the
-- client, so the tables that backed them are dropped. Blocks, command history,
-- session/pane layout, projects, and workspace metadata are untouched.

-- Pane payload tables first: they carry (id, kind) foreign keys into
-- pane_leaves, so the corresponding pane_leaves rows can only be cleaned up
-- once these are gone. `notebook_panes` stays: its `local_path` half restores
-- local Markdown panes, and its `notebook_id` column is orphaned in place.
DROP TABLE IF EXISTS workflow_panes;
DROP TABLE IF EXISTS env_var_collection_panes;

-- Cloud object tables, children first.
DROP TABLE IF EXISTS object_permissions;
DROP TABLE IF EXISTS object_actions;
DROP TABLE IF EXISTS cloud_objects_refreshes;
DROP TABLE IF EXISTS object_metadata;
DROP TABLE IF EXISTS workflows;
DROP TABLE IF EXISTS notebooks;
DROP TABLE IF EXISTS folders;
DROP TABLE IF EXISTS generic_string_objects;

-- Team and workspace tables, children first.
DROP TABLE IF EXISTS team_members;
DROP TABLE IF EXISTS team_settings;
DROP TABLE IF EXISTS workspace_teams;
DROP TABLE IF EXISTS teams;
DROP TABLE IF EXISTS workspaces;

-- Identity tables.
DROP TABLE IF EXISTS user_profiles;
DROP TABLE IF EXISTS current_user_information;
DROP TABLE IF EXISTS users;

-- Server-driven experiments.
DROP TABLE IF EXISTS server_experiments;

-- Leaf rows for the removed pane kinds would fail restoration with
-- "Unrecognized pane kind", so drop them. Cloud notebook panes keep their leaf
-- row: the client maps them to a fresh terminal pane, except when they carry
-- neither a local path nor a notebook id and so describe nothing at all.
DELETE FROM pane_leaves
    WHERE kind IN ('workflow', 'env_var_collection');

DELETE FROM notebook_panes
    WHERE local_path IS NULL AND notebook_id IS NULL;

DELETE FROM pane_leaves
    WHERE kind = 'notebook'
    AND pane_node_id NOT IN (SELECT id FROM notebook_panes);

DELETE FROM pane_nodes
    WHERE is_leaf = 1
    AND id NOT IN (SELECT pane_node_id FROM pane_leaves);
