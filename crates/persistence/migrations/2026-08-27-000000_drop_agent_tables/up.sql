-- Agent Mode and MCP were removed from the client, so the tables that backed
-- them are dropped. Blocks, command history, session/pane layout, and cloud
-- object tables are untouched.

-- Pane tables first: they carry (pane_node_id, kind) foreign keys into
-- pane_leaves, so the corresponding pane_leaves rows can only be cleaned up
-- once these are gone.
DROP TABLE IF EXISTS ai_memory_panes;
DROP TABLE IF EXISTS ai_document_panes;
DROP TABLE IF EXISTS mcp_server_panes;
DROP TABLE IF EXISTS ambient_agent_panes;

DROP TABLE IF EXISTS agent_tasks;
DROP TABLE IF EXISTS agent_conversations;
DROP TABLE IF EXISTS ai_queries;
DROP TABLE IF EXISTS active_mcp_servers;
DROP TABLE IF EXISTS mcp_environment_variables;
DROP TABLE IF EXISTS mcp_server_installations;
DROP TABLE IF EXISTS project_rules;

-- Leaf rows for the removed pane kinds would fail restoration with
-- "Unrecognized pane kind", so drop them and the pane_nodes they orphan.
DELETE FROM pane_leaves
    WHERE kind IN ('ai_memory', 'ai_document', 'mcp_server', 'ambient_agent', 'execution_profile_editor');

DELETE FROM pane_nodes
    WHERE is_leaf = 1
    AND id NOT IN (SELECT pane_node_id FROM pane_leaves);
