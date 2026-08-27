CREATE TABLE ai_memory_panes (
  id INTEGER PRIMARY KEY NOT NULL,
  kind TEXT NOT NULL DEFAULT 'ai_memory' CHECK (kind = 'ai_memory'),
  FOREIGN KEY (id, kind) REFERENCES pane_leaves (pane_node_id, kind)
);

CREATE TABLE ai_document_panes (
    id INTEGER PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL DEFAULT 'ai_document' CHECK (kind = 'ai_document'),
    document_id TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    content TEXT,
    title TEXT,
    FOREIGN KEY (id, kind) REFERENCES pane_leaves (pane_node_id, kind)
);

CREATE TABLE mcp_server_panes (
  id INTEGER PRIMARY KEY NOT NULL,
  kind TEXT NOT NULL DEFAULT 'mcp_server' CHECK (kind = 'mcp_server'),
  FOREIGN KEY (id, kind) REFERENCES pane_leaves (pane_node_id, kind)
);

CREATE TABLE ambient_agent_panes (
    id INTEGER PRIMARY KEY NOT NULL REFERENCES pane_nodes(id),
    kind TEXT NOT NULL DEFAULT 'ambient_agent' CHECK (kind = 'ambient_agent'),
    uuid BLOB NOT NULL,
    task_id TEXT
);

CREATE TABLE agent_conversations (
    id INTEGER PRIMARY KEY NOT NULL,
    conversation_id TEXT NOT NULL UNIQUE,
    conversation_data TEXT NOT NULL,
    last_modified_at TIMESTAMP NOT NULL,
    summary TEXT
);

CREATE TABLE agent_tasks (
    id INTEGER PRIMARY KEY NOT NULL,
    conversation_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    task BLOB NOT NULL,
    last_modified_at TIMESTAMP NOT NULL,
    FOREIGN KEY (conversation_id) REFERENCES agent_conversations (conversation_id)
);

CREATE TABLE ai_queries (
    id INTEGER PRIMARY KEY NOT NULL,
    exchange_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    start_ts TIMESTAMP NOT NULL,
    input TEXT NOT NULL,
    working_directory TEXT,
    output_status TEXT NOT NULL,
    model_id TEXT NOT NULL DEFAULT '',
    planning_model_id TEXT NOT NULL DEFAULT '',
    coding_model_id TEXT NOT NULL DEFAULT ''
);

CREATE UNIQUE INDEX ux_ai_queries_exchange_id ON ai_queries(exchange_id);

CREATE TABLE active_mcp_servers (
    id INTEGER PRIMARY KEY NOT NULL,
    mcp_server_uuid TEXT NOT NULL
);

CREATE TABLE mcp_environment_variables (
    mcp_server_uuid BLOB PRIMARY KEY NOT NULL,
    environment_variables TEXT NOT NULL
);

CREATE TABLE mcp_server_installations (
    id TEXT PRIMARY KEY NOT NULL,
    templatable_mcp_server TEXT NOT NULL,
    template_version_ts TIMESTAMP NOT NULL,
    variable_values TEXT NOT NULL,
    restore_running BOOLEAN NOT NULL DEFAULT FALSE,
    last_modified_at TIMESTAMP NOT NULL
);

CREATE TABLE project_rules (
    id INTEGER PRIMARY KEY NOT NULL,
    path TEXT NOT NULL,
    project_root TEXT NOT NULL
);
