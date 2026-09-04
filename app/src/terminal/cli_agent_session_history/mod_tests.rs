use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{TimeZone, Utc};

use super::*;

fn session(agent: CLIAgent, id: &str, cwd: &str, title: &str) -> AgentSession {
    let modified_at = Utc.with_ymd_and_hms(2025, 1, 2, 3, 4, 5).unwrap();
    AgentSession {
        id: id.to_owned(),
        agent,
        session_id: id.to_owned(),
        title: title.to_owned(),
        cwd: Some(PathBuf::from(cwd)),
        branch: Some("main".to_owned()),
        model: Some("sonnet".to_owned()),
        transcript_path: PathBuf::from(format!("{cwd}/{id}.jsonl")),
        codex_home: None,
        created_at: Some(modified_at),
        updated_at: Some(modified_at),
        modified_at,
        message_count: 1,
        total_tokens: 10,
        preview_messages: vec![SessionPreviewMessage {
            role: PreviewRole::User,
            text: "fix search".to_owned(),
            timestamp: None,
        }],
        preview_messages_truncated: false,
        first_user_prompt: Some("fix search".to_owned()),
        last_user_prompt: Some("fix search".to_owned()),
        queued_message_count: 0,
        subagent_transcript_count: 0,
        resume_command: String::new(),
    }
}

#[test]
fn query_operators_filter_repository_and_path() {
    let sessions = vec![session(CLIAgent::Claude, "one", "/tmp/spirit", "Sidebar")];
    let filter = SessionFilter {
        query: "repo:spirit path:/tmp sidebar".to_owned(),
        enabled_agents: HashSet::from([CLIAgent::Claude]),
        ..Default::default()
    };

    let filtered = filter_sessions(&sessions, &filter);

    assert_eq!(filtered.len(), 1);
}

#[test]
fn workspace_scope_includes_nested_session_directory() {
    let sessions = vec![session(
        CLIAgent::Codex,
        "one",
        "/tmp/spirit/crates/app",
        "Work",
    )];
    let filter = SessionFilter {
        enabled_agents: HashSet::from([CLIAgent::Codex]),
        workspace_paths: vec![PathBuf::from("/tmp/spirit")],
        ..Default::default()
    };

    assert_eq!(filter_sessions(&sessions, &filter).len(), 1);
}

#[test]
fn worktree_filter_excludes_sessions_from_other_worktrees() {
    let sessions = vec![
        session(CLIAgent::Claude, "one", "/tmp/spirit", "Primary"),
        session(CLIAgent::Claude, "two", "/tmp/spirit-fix/crates", "Linked"),
    ];
    let filter = SessionFilter {
        enabled_agents: HashSet::from([CLIAgent::Claude]),
        workspace_paths: vec![
            PathBuf::from("/tmp/spirit"),
            PathBuf::from("/tmp/spirit-fix"),
        ],
        worktree_path: Some(PathBuf::from("/tmp/spirit-fix")),
        ..Default::default()
    };

    let filtered = filter_sessions(&sessions, &filter);

    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, "two");
}

#[test]
fn resume_command_uses_agent_specific_flag() {
    let command = build_resume_command(
        CLIAgent::Codex,
        "abc",
        Some(Path::new("/tmp/project")),
        None,
        None,
    );

    assert!(command.contains("codex resume"));
    assert!(command.contains("abc"));
}

#[test]
fn parser_uses_first_user_message_as_title() {
    let value = serde_json::json!({
        "session_id": "abc",
        "cwd": "/tmp/project",
        "role": "user",
        "content": "Build the sessions panel"
    });
    let modified_at = Utc.with_ymd_and_hms(2025, 1, 2, 3, 4, 5).unwrap();
    let mut accumulator =
        SessionAccumulator::new(CLIAgent::Claude, Path::new("abc.jsonl"), modified_at);

    accumulator.visit(&value);
    let session = accumulator.finish().unwrap();

    assert_eq!(session.title, "Build the sessions panel");
    assert_eq!(session.message_count, 1);
}

#[test]
fn indexed_agent_deletion_is_rejected() {
    let session = session(CLIAgent::Codex, "one", "/tmp/spirit", "Work");

    let error = deletion_targets(&session, Path::new("/tmp")).unwrap_err();

    assert!(error.contains("indexes"));
}

#[test]
fn cline_parser_reads_companion_messages_once() {
    let directory = tempfile::tempdir().unwrap();
    let metadata = directory.path().join("task_metadata.json");
    fs::write(
        &metadata,
        r#"{"session_id":"cline-1","cwd":"/tmp/project","title":"Cline task"}"#,
    )
    .unwrap();
    fs::write(
        directory.path().join("messages.json"),
        r#"[{"role":"user","content":"Fix the parser"}]"#,
    )
    .unwrap();

    let parsed = parse_session_file(CLIAgent::Cline, &metadata)
        .unwrap()
        .unwrap();

    assert_eq!(parsed.session_id, "cline-1");
    assert_eq!(parsed.message_count, 1);
    assert_eq!(parsed.first_user_prompt.as_deref(), Some("Fix the parser"));
}

#[test]
fn claude_deletion_includes_session_owned_companions() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join(".claude/projects/project");
    let session_id = "session-1";
    let transcript = root.join(format!("{session_id}.jsonl"));
    let session_artifacts = root.join(session_id);
    let session_env = directory
        .path()
        .join(".claude/session-env")
        .join(session_id);
    fs::create_dir_all(session_artifacts.join("subagents")).unwrap();
    fs::create_dir_all(&session_env).unwrap();
    fs::write(&transcript, "{}").unwrap();
    fs::write(session_artifacts.join("subagents/agent-worker.jsonl"), "{}").unwrap();
    let mut session = session(CLIAgent::Claude, session_id, "/tmp/project", "Work");
    session.transcript_path = transcript.clone();

    let targets = deletion_targets(&session, directory.path()).unwrap();

    assert_eq!(count_subagent_transcripts(CLIAgent::Claude, &transcript), 1);
    assert_eq!(
        targets,
        vec![
            session_env.canonicalize().unwrap(),
            session_artifacts.canonicalize().unwrap(),
            transcript.canonicalize().unwrap(),
        ]
    );
}

#[cfg(not(target_family = "wasm"))]
#[test]
fn opencode_sqlite_sessions_are_discovered() {
    use diesel::Connection;
    use diesel::connection::SimpleConnection;

    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("opencode.db");
    let mut connection =
        diesel::sqlite::SqliteConnection::establish(database.to_str().unwrap()).unwrap();
    connection
        .batch_execute(
            r#"
            CREATE TABLE session (
                id TEXT PRIMARY KEY,
                title TEXT,
                directory TEXT,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                model TEXT,
                parent_id TEXT,
                time_archived INTEGER
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            CREATE TABLE part (
                message_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            INSERT INTO session VALUES (
                'ses_1', 'SQLite session', '/tmp/project', 1735787045000, 1735787105000,
                '{"id":"gpt-5"}', NULL, NULL
            );
            INSERT INTO message VALUES (
                'msg_1', 'ses_1', 1735787055000, '{"role":"user"}'
            );
            INSERT INTO part VALUES (
                'msg_1', 1735787055000, '{"type":"text","text":"Build it"}'
            );
            "#,
        )
        .unwrap();
    drop(connection);

    let sessions = read_opencode_database(&database, SessionLimit::TwoHundredFifty).unwrap();

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_id, "ses_1");
    assert_eq!(sessions[0].model.as_deref(), Some("gpt-5"));
    assert_eq!(sessions[0].first_user_prompt.as_deref(), Some("Build it"));
}

#[cfg(not(target_family = "wasm"))]
#[test]
fn parse_cache_invalidates_when_a_companion_changes() {
    let directory = tempfile::tempdir().unwrap();
    let metadata = directory.path().join("task_metadata.json");
    let messages = directory.path().join("messages.json");
    fs::write(
        &metadata,
        r#"{"session_id":"cline-1","cwd":"/tmp/project"}"#,
    )
    .unwrap();
    fs::write(&messages, r#"[{"role":"user","content":"First"}]"#).unwrap();
    let mut cache = PersistentParseCache {
        version: PARSE_CACHE_VERSION,
        ..Default::default()
    };
    let mut seen = HashSet::new();
    let first = parse_session_file_cached(CLIAgent::Cline, &metadata, &mut cache, &mut seen)
        .unwrap()
        .unwrap();
    fs::write(
        &messages,
        r#"[{"role":"user","content":"Second, longer prompt"}]"#,
    )
    .unwrap();

    let second = parse_session_file_cached(CLIAgent::Cline, &metadata, &mut cache, &mut seen)
        .unwrap()
        .unwrap();

    assert_eq!(first.first_user_prompt.as_deref(), Some("First"));
    assert_eq!(
        second.first_user_prompt.as_deref(),
        Some("Second, longer prompt")
    );
}
