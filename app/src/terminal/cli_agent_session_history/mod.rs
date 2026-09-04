use std::collections::{HashMap, HashSet};
use std::fs::File;
#[cfg(not(target_family = "wasm"))]
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
#[cfg(not(target_family = "wasm"))]
use diesel::connection::SimpleConnection;
#[cfg(not(target_family = "wasm"))]
use diesel::sqlite::SqliteConnection;
#[cfg(not(target_family = "wasm"))]
use diesel::{Connection, RunQueryDsl};
use instant::Instant;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use walkdir::{DirEntry, WalkDir};
use warpui::{Entity, ModelContext, SingletonEntity};

use super::CLIAgent;

const PREVIEW_LIMIT: usize = 5;
const PREVIEW_TEXT_LIMIT: usize = 2_000;
const FIRST_PROMPT_LIMIT: usize = 64 * 1_024;
const TITLE_LIMIT: usize = 120;
const QUERY_LIMIT: usize = 2 * 1_024;
const CACHE_TTL: Duration = Duration::from_secs(60);
#[cfg(not(target_family = "wasm"))]
const PARSE_CACHE_VERSION: u32 = 1;
#[cfg(not(target_family = "wasm"))]
const PARSE_CACHE_MAX_ENTRIES: usize = 5_000;

pub const SUPPORTED_AGENTS: [CLIAgent; 14] = [
    CLIAgent::Claude,
    CLIAgent::Gemini,
    CLIAgent::Codex,
    CLIAgent::Droid,
    CLIAgent::OpenCode,
    CLIAgent::Copilot,
    CLIAgent::Pi,
    CLIAgent::OhMyPi,
    CLIAgent::CursorCli,
    CLIAgent::Hermes,
    CLIAgent::Antigravity,
    CLIAgent::Grok,
    CLIAgent::Cline,
    CLIAgent::Devin,
];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionSort {
    #[default]
    Updated,
    Created,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionGroup {
    #[default]
    Project,
    Folder,
    Agent,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionLimit {
    #[default]
    TwoHundredFifty,
    FiveHundred,
    OneThousand,
    Unlimited,
}

impl SessionLimit {
    pub fn count(self) -> Option<usize> {
        match self {
            Self::TwoHundredFifty => Some(250),
            Self::FiveHundred => Some(500),
            Self::OneThousand => Some(1_000),
            Self::Unlimited => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewRole {
    User,
    Assistant,
    System,
    Tool,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionPreviewMessage {
    pub role: PreviewRole,
    pub text: String,
    pub timestamp: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AgentSession {
    pub id: String,
    pub agent: CLIAgent,
    pub session_id: String,
    pub title: String,
    pub cwd: Option<PathBuf>,
    pub branch: Option<String>,
    pub model: Option<String>,
    pub transcript_path: PathBuf,
    pub codex_home: Option<PathBuf>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub modified_at: DateTime<Utc>,
    pub message_count: usize,
    pub total_tokens: u64,
    pub preview_messages: Vec<SessionPreviewMessage>,
    pub preview_messages_truncated: bool,
    pub first_user_prompt: Option<String>,
    pub last_user_prompt: Option<String>,
    pub queued_message_count: usize,
    pub subagent_transcript_count: usize,
    pub resume_command: String,
}

impl AgentSession {
    pub fn has_resumable_content(&self) -> bool {
        self.message_count > 0
            || self
                .preview_messages
                .iter()
                .any(|message| matches!(message.role, PreviewRole::User | PreviewRole::Assistant))
    }

    pub fn is_recoverable_empty(&self) -> bool {
        !self.has_resumable_content()
            && self.queued_message_count + self.subagent_transcript_count > 0
    }

    pub fn effective_updated_at(&self) -> DateTime<Utc> {
        self.updated_at.unwrap_or(self.modified_at)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionScanIssue {
    pub agent: CLIAgent,
    pub path: PathBuf,
    pub message: String,
}

#[derive(Clone, Debug, Default)]
pub struct SessionFilter {
    pub query: String,
    pub enabled_agents: HashSet<CLIAgent>,
    pub sort: SessionSort,
    pub workspace_paths: Vec<PathBuf>,
    pub worktree_path: Option<PathBuf>,
    pub hide_empty: bool,
}

#[derive(Clone, Debug)]
pub struct SessionGroupResult {
    pub key: String,
    pub label: String,
    pub sessions: Vec<AgentSession>,
}

pub fn filter_sessions(sessions: &[AgentSession], filter: &SessionFilter) -> Vec<AgentSession> {
    if filter.query.len() > QUERY_LIMIT {
        return Vec::new();
    }
    let query = ParsedQuery::parse(&filter.query);
    let mut filtered: Vec<_> = sessions
        .iter()
        .filter(|session| {
            filter.enabled_agents.contains(&session.agent)
                && (!filter.hide_empty
                    || session.has_resumable_content()
                    || session.is_recoverable_empty())
                && matches_scope(session, filter)
                && query.matches(session)
        })
        .cloned()
        .collect();
    filtered.sort_by(|left, right| {
        let left_time = match filter.sort {
            SessionSort::Updated => left.effective_updated_at(),
            SessionSort::Created => left.created_at.unwrap_or(left.modified_at),
        };
        let right_time = match filter.sort {
            SessionSort::Updated => right.effective_updated_at(),
            SessionSort::Created => right.created_at.unwrap_or(right.modified_at),
        };
        right_time.cmp(&left_time)
    });
    filtered
}

pub fn group_sessions(sessions: Vec<AgentSession>, group: SessionGroup) -> Vec<SessionGroupResult> {
    let mut groups = Vec::<SessionGroupResult>::new();
    let mut indices = HashMap::<String, usize>::new();
    for session in sessions {
        let (key, label) = match group {
            SessionGroup::Agent => {
                let label = session.agent.display_name().to_owned();
                (format!("agent:{label}"), label)
            }
            SessionGroup::Project => project_group(&session),
            SessionGroup::Folder => folder_group(&session),
        };
        if let Some(index) = indices.get(&key).copied() {
            groups[index].sessions.push(session);
        } else {
            indices.insert(key.clone(), groups.len());
            groups.push(SessionGroupResult {
                key,
                label,
                sessions: vec![session],
            });
        }
    }
    groups
}

fn project_group(session: &AgentSession) -> (String, String) {
    let Some(cwd) = &session.cwd else {
        return ("unknown".to_owned(), "Unknown project".to_owned());
    };
    for directory in cwd.ancestors() {
        if directory.join(".git").exists() {
            let label = directory
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| directory.display().to_string());
            return (format!("project:{}", normalized_path(directory)), label);
        }
    }
    folder_group(session)
}

fn folder_group(session: &AgentSession) -> (String, String) {
    let Some(cwd) = &session.cwd else {
        return ("unknown".to_owned(), "Unknown location".to_owned());
    };
    let components: Vec<_> = cwd.components().collect();
    let label = components
        .iter()
        .rev()
        .take(2)
        .rev()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join(std::path::MAIN_SEPARATOR_STR);
    (format!("folder:{}", normalized_path(cwd)), label)
}

fn matches_scope(session: &AgentSession, filter: &SessionFilter) -> bool {
    let paths = match &filter.worktree_path {
        Some(worktree_path) => std::slice::from_ref(worktree_path),
        None => filter.workspace_paths.as_slice(),
    };
    if paths.is_empty() {
        return true;
    }
    let Some(cwd) = &session.cwd else {
        return false;
    };
    paths.iter().any(|path| path_contains(path, cwd))
}

pub fn path_contains(root: &Path, candidate: &Path) -> bool {
    let root = normalized_path(root);
    let candidate = normalized_path(candidate);
    candidate == root
        || candidate
            .strip_prefix(&root)
            .is_some_and(|suffix| suffix.starts_with(std::path::MAIN_SEPARATOR))
}

fn normalized_path(path: &Path) -> String {
    let value = path
        .to_string_lossy()
        .trim_end_matches(['/', '\\'])
        .to_owned();
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
}

struct ParsedQuery {
    terms: Vec<String>,
    repo_terms: Vec<String>,
    path_terms: Vec<String>,
}

impl ParsedQuery {
    fn parse(query: &str) -> Self {
        let mut parsed = Self {
            terms: Vec::new(),
            repo_terms: Vec::new(),
            path_terms: Vec::new(),
        };
        for token in tokenize_query(query) {
            let token = token.to_lowercase();
            if let Some(value) = token.strip_prefix("repo:") {
                if !value.is_empty() {
                    parsed.repo_terms.push(value.to_owned());
                }
            } else if let Some(value) = token.strip_prefix("path:") {
                if !value.is_empty() {
                    parsed.path_terms.push(value.to_owned());
                }
            } else {
                parsed.terms.push(token);
            }
        }
        parsed
    }

    fn matches(&self, session: &AgentSession) -> bool {
        let previews = session
            .preview_messages
            .iter()
            .map(|message| message.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let searchable = format!(
            "{} {} {} {} {} {} {} {previews}",
            session.title,
            session.session_id,
            session.agent.display_name(),
            session.branch.as_deref().unwrap_or_default(),
            session.model.as_deref().unwrap_or_default(),
            session
                .cwd
                .as_deref()
                .map(Path::display)
                .map(|value| value.to_string())
                .unwrap_or_default(),
            session.transcript_path.display(),
        )
        .to_lowercase();
        if self.terms.iter().any(|term| !searchable.contains(term)) {
            return false;
        }
        let repo = session
            .cwd
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if self.repo_terms.iter().any(|term| !repo.contains(term)) {
            return false;
        }
        let paths = format!(
            "{} {}",
            session
                .cwd
                .as_deref()
                .map(Path::display)
                .map(|value| value.to_string())
                .unwrap_or_default(),
            session.transcript_path.display()
        )
        .to_lowercase();
        !self.path_terms.iter().any(|term| !paths.contains(term))
    }
}

fn tokenize_query(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    for character in query.chars() {
        match (quote, character) {
            (Some(expected), value) if value == expected => quote = None,
            (None, '\'' | '"') => quote = Some(character),
            (None, value) if value.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(character),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[derive(Clone, Debug)]
struct AgentSource {
    agent: CLIAgent,
    root: PathBuf,
}

fn agent_sources(home: &Path) -> Vec<AgentSource> {
    let env_path = |name: &str, fallback: PathBuf| {
        std::env::var_os(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or(fallback)
    };
    vec![
        AgentSource {
            agent: CLIAgent::Claude,
            root: home.join(".claude/projects"),
        },
        AgentSource {
            agent: CLIAgent::Gemini,
            root: home.join(".gemini/tmp"),
        },
        AgentSource {
            agent: CLIAgent::Codex,
            root: env_path("CODEX_HOME", home.join(".codex")).join("sessions"),
        },
        AgentSource {
            agent: CLIAgent::Droid,
            root: home.join(".factory/sessions"),
        },
        AgentSource {
            agent: CLIAgent::Droid,
            root: home.join(".factory/projects"),
        },
        AgentSource {
            agent: CLIAgent::OpenCode,
            root: env_path("XDG_DATA_HOME", home.join(".local/share")).join("opencode/storage"),
        },
        AgentSource {
            agent: CLIAgent::Copilot,
            root: env_path("COPILOT_HOME", home.join(".copilot")).join("session-state"),
        },
        AgentSource {
            agent: CLIAgent::Pi,
            root: normalized_agent_root(
                "PI_CODING_AGENT_DIR",
                home.join(".pi/agent/sessions"),
                ".pi",
            ),
        },
        AgentSource {
            agent: CLIAgent::OhMyPi,
            root: normalized_agent_root(
                "OMP_CODING_AGENT_DIR",
                home.join(".omp/agent/sessions"),
                ".omp",
            ),
        },
        AgentSource {
            agent: CLIAgent::CursorCli,
            root: home.join(".cursor/projects"),
        },
        AgentSource {
            agent: CLIAgent::Hermes,
            root: home.join(".hermes/sessions"),
        },
        AgentSource {
            agent: CLIAgent::Antigravity,
            root: home.join(".gemini/antigravity-cli/brain"),
        },
        AgentSource {
            agent: CLIAgent::Grok,
            root: env_path("GROK_HOME", home.join(".grok")).join("sessions"),
        },
        AgentSource {
            agent: CLIAgent::Cline,
            root: env_path("CLINE_SESSION_DATA_DIR", home.join(".cline/data/sessions")),
        },
        AgentSource {
            agent: CLIAgent::Devin,
            root: env_path("DEVIN_HOME", home.join(".local/share/devin/cli")).join("transcripts"),
        },
    ]
}

fn normalized_agent_root(variable: &str, fallback: PathBuf, suffix: &str) -> PathBuf {
    let Some(value) = std::env::var_os(variable).filter(|value| !value.is_empty()) else {
        return fallback;
    };
    let root = PathBuf::from(value);
    if root.ends_with("agent/sessions") {
        root
    } else if root.ends_with(suffix) {
        root.join("agent/sessions")
    } else {
        root
    }
}

fn should_descend(source: &AgentSource, entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    let name = entry.file_name().to_string_lossy();
    match source.agent {
        CLIAgent::Claude => name != "subagents",
        CLIAgent::OhMyPi => {
            !name.starts_with("subagent-")
                && name != "subagents"
                && !(entry.file_type().is_dir() && entry.path().with_extension("jsonl").is_file())
        }
        CLIAgent::Antigravity => match entry.depth() {
            1 => true,
            2 => name == ".system_generated",
            3 => name == "logs",
            _ => true,
        },
        CLIAgent::Cline => entry.depth() <= 2,
        CLIAgent::Gemini
        | CLIAgent::Codex
        | CLIAgent::Droid
        | CLIAgent::OpenCode
        | CLIAgent::Copilot
        | CLIAgent::Pi
        | CLIAgent::CursorCli
        | CLIAgent::Hermes
        | CLIAgent::Grok
        | CLIAgent::Devin
        | CLIAgent::Amp
        | CLIAgent::Auggie
        | CLIAgent::Goose
        | CLIAgent::Vibe
        | CLIAgent::Trae
        | CLIAgent::QwenCode
        | CLIAgent::Unknown => true,
    }
}

fn is_session_file(source: &AgentSource, path: &Path) -> bool {
    let extension = path.extension().and_then(|value| value.to_str());
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    match source.agent {
        CLIAgent::CursorCli => {
            extension == Some("jsonl")
                && path
                    .components()
                    .any(|part| part.as_os_str() == "agent-transcripts")
        }
        CLIAgent::Hermes => extension == Some("json") && name.starts_with("session_"),
        CLIAgent::Grok => name == "summary.json",
        CLIAgent::Cline => {
            extension == Some("json") && matches!(name, "task_metadata.json" | "metadata.json")
        }
        CLIAgent::Antigravity => {
            name == "transcript.jsonl" && path.to_string_lossy().contains(".system_generated")
        }
        CLIAgent::Gemini | CLIAgent::Devin => matches!(extension, Some("json" | "jsonl")),
        CLIAgent::Claude
        | CLIAgent::Codex
        | CLIAgent::Droid
        | CLIAgent::Copilot
        | CLIAgent::Pi
        | CLIAgent::OhMyPi => extension == Some("jsonl"),
        CLIAgent::OpenCode => extension == Some("json"),
        CLIAgent::Amp
        | CLIAgent::Auggie
        | CLIAgent::Goose
        | CLIAgent::Vibe
        | CLIAgent::Trae
        | CLIAgent::QwenCode
        | CLIAgent::Unknown => false,
    }
}

pub fn scan_home(home: &Path, limit: SessionLimit) -> (Vec<AgentSession>, Vec<SessionScanIssue>) {
    let mut sessions = Vec::new();
    let mut issues = Vec::new();
    #[cfg(not(target_family = "wasm"))]
    let mut parse_cache = load_parse_cache();
    #[cfg(not(target_family = "wasm"))]
    let mut seen_cache_paths = HashSet::new();
    for source in agent_sources(home) {
        if !source.root.is_dir() {
            continue;
        }
        let walker = WalkDir::new(&source.root)
            .follow_links(false)
            .max_depth(8)
            .into_iter()
            .filter_entry(|entry| should_descend(&source, entry));
        for entry in walker {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    issues.push(SessionScanIssue {
                        agent: source.agent,
                        path: source.root.clone(),
                        message: error.to_string(),
                    });
                    continue;
                }
            };
            if !entry.file_type().is_file() || !is_session_file(&source, entry.path()) {
                continue;
            }
            #[cfg(not(target_family = "wasm"))]
            let parsed = parse_session_file_cached(
                source.agent,
                entry.path(),
                &mut parse_cache,
                &mut seen_cache_paths,
            );
            #[cfg(target_family = "wasm")]
            let parsed = parse_session_file(source.agent, entry.path());
            match parsed {
                Ok(Some(session)) => sessions.push(session),
                Ok(None) => {}
                Err(error) => issues.push(SessionScanIssue {
                    agent: source.agent,
                    path: entry.path().to_path_buf(),
                    message: error,
                }),
            }
        }
    }
    #[cfg(not(target_family = "wasm"))]
    persist_parse_cache(&mut parse_cache, &seen_cache_paths);
    #[cfg(not(target_family = "wasm"))]
    scan_opencode_databases(home, limit, &mut sessions, &mut issues);
    sessions.sort_by_key(|session| std::cmp::Reverse(session.effective_updated_at()));
    let mut indexed_sessions = HashSet::new();
    sessions.retain(|session| {
        !matches!(session.agent, CLIAgent::Codex | CLIAgent::OpenCode)
            || indexed_sessions.insert((session.agent, session.session_id.clone()))
    });
    if let Some(limit) = limit.count() {
        sessions.truncate(limit);
    }
    (sessions, issues)
}

#[cfg(not(target_family = "wasm"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct SessionFileFingerprint {
    length: u64,
    modified_millis: u64,
    dependency_fingerprint: u64,
}

#[cfg(not(target_family = "wasm"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
struct CachedSessionFile {
    fingerprint: SessionFileFingerprint,
    session: Option<AgentSession>,
}

#[cfg(not(target_family = "wasm"))]
#[derive(Default, Serialize, Deserialize)]
struct PersistentParseCache {
    version: u32,
    entries: HashMap<PathBuf, CachedSessionFile>,
}

#[cfg(not(target_family = "wasm"))]
fn parse_cache_path() -> PathBuf {
    warp_core::paths::cache_dir()
        .join("agent-session-history")
        .join("session-parse-cache.json")
}

#[cfg(not(target_family = "wasm"))]
fn load_parse_cache() -> PersistentParseCache {
    let path = parse_cache_path();
    let Ok(metadata) = path.metadata() else {
        return PersistentParseCache {
            version: PARSE_CACHE_VERSION,
            ..Default::default()
        };
    };
    if metadata.len() > 64 * 1_024 * 1_024 {
        return PersistentParseCache {
            version: PARSE_CACHE_VERSION,
            ..Default::default()
        };
    }
    let cache = File::open(path)
        .ok()
        .and_then(|file| serde_json::from_reader::<_, PersistentParseCache>(file).ok());
    match cache {
        Some(cache) if cache.version == PARSE_CACHE_VERSION => cache,
        Some(_) | None => PersistentParseCache {
            version: PARSE_CACHE_VERSION,
            ..Default::default()
        },
    }
}

#[cfg(not(target_family = "wasm"))]
fn persist_parse_cache(cache: &mut PersistentParseCache, seen_paths: &HashSet<PathBuf>) {
    cache.entries.retain(|path, _| seen_paths.contains(path));
    if cache.entries.len() > PARSE_CACHE_MAX_ENTRIES {
        let mut paths = cache
            .entries
            .iter()
            .map(|(path, entry)| (path.clone(), entry.fingerprint.modified_millis))
            .collect::<Vec<_>>();
        paths.sort_by_key(|(_, modified_millis)| std::cmp::Reverse(*modified_millis));
        let retained = paths
            .into_iter()
            .take(PARSE_CACHE_MAX_ENTRIES)
            .map(|(path, _)| path)
            .collect::<HashSet<_>>();
        cache.entries.retain(|path, _| retained.contains(path));
    }
    let path = parse_cache_path();
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    #[cfg(unix)]
    let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let temporary = path.with_extension(format!("json.{}.{nonce}.tmp", std::process::id()));
    let mut open_options = OpenOptions::new();
    open_options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    open_options.mode(0o600);
    let Ok(file) = open_options.open(&temporary) else {
        return;
    };
    if serde_json::to_writer(file, cache).is_err() {
        let _ = std::fs::remove_file(temporary);
        return;
    }
    if std::fs::rename(&temporary, &path).is_err() {
        let _ = std::fs::copy(&temporary, &path);
        let _ = std::fs::remove_file(temporary);
    }
    #[cfg(unix)]
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(target_family = "wasm"))]
fn parse_session_file_cached(
    agent: CLIAgent,
    path: &Path,
    cache: &mut PersistentParseCache,
    seen_paths: &mut HashSet<PathBuf>,
) -> Result<Option<AgentSession>, String> {
    let fingerprint = session_file_fingerprint(agent, path)?;
    seen_paths.insert(path.to_path_buf());
    if let Some(entry) = cache.entries.get(path)
        && entry.fingerprint == fingerprint
    {
        return Ok(entry.session.clone());
    }
    let session = parse_session_file(agent, path)?;
    cache.entries.insert(
        path.to_path_buf(),
        CachedSessionFile {
            fingerprint,
            session: session.clone(),
        },
    );
    Ok(session)
}

#[cfg(not(target_family = "wasm"))]
fn session_file_fingerprint(
    agent: CLIAgent,
    path: &Path,
) -> Result<SessionFileFingerprint, String> {
    let metadata = path.metadata().map_err(|error| error.to_string())?;
    let mut dependency_fingerprint = 0;
    let dependencies: &[&str] = match agent {
        CLIAgent::Grok => &["chat_history.jsonl"],
        CLIAgent::Cline => &["api_conversation_history.json", "messages.json"],
        CLIAgent::Claude
        | CLIAgent::Gemini
        | CLIAgent::Codex
        | CLIAgent::Droid
        | CLIAgent::OpenCode
        | CLIAgent::Copilot
        | CLIAgent::Pi
        | CLIAgent::OhMyPi
        | CLIAgent::CursorCli
        | CLIAgent::Hermes
        | CLIAgent::Antigravity
        | CLIAgent::Devin
        | CLIAgent::Amp
        | CLIAgent::Auggie
        | CLIAgent::Goose
        | CLIAgent::Vibe
        | CLIAgent::Trae
        | CLIAgent::QwenCode
        | CLIAgent::Unknown => &[],
    };
    for dependency in dependencies {
        if let Ok(metadata) = path.with_file_name(dependency).metadata() {
            dependency_fingerprint ^= metadata.len().rotate_left(17);
            dependency_fingerprint ^=
                modified_millis(&metadata).rotate_left(dependency.len() as u32);
        }
    }
    let subagent_directory = match agent {
        CLIAgent::Claude => Some(path.with_extension("").join("subagents")),
        CLIAgent::OhMyPi => Some(path.with_extension("")),
        CLIAgent::Gemini
        | CLIAgent::Codex
        | CLIAgent::Droid
        | CLIAgent::OpenCode
        | CLIAgent::Copilot
        | CLIAgent::Pi
        | CLIAgent::CursorCli
        | CLIAgent::Hermes
        | CLIAgent::Antigravity
        | CLIAgent::Grok
        | CLIAgent::Cline
        | CLIAgent::Devin
        | CLIAgent::Amp
        | CLIAgent::Auggie
        | CLIAgent::Goose
        | CLIAgent::Vibe
        | CLIAgent::Trae
        | CLIAgent::QwenCode
        | CLIAgent::Unknown => None,
    };
    if let Some(directory) = subagent_directory
        && let Ok(entries) = directory.read_dir()
    {
        for entry in entries.filter_map(Result::ok) {
            if let Ok(metadata) = entry.metadata() {
                dependency_fingerprint ^= metadata.len().rotate_left(7);
                dependency_fingerprint ^= modified_millis(&metadata).rotate_left(23);
            }
        }
    }
    Ok(SessionFileFingerprint {
        length: metadata.len(),
        modified_millis: modified_millis(&metadata),
        dependency_fingerprint,
    })
}

#[cfg(not(target_family = "wasm"))]
fn modified_millis(metadata: &std::fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

#[cfg(not(target_family = "wasm"))]
#[derive(diesel::QueryableByName)]
struct SqliteColumn {
    #[diesel(sql_type = diesel::sql_types::Text)]
    name: String,
}

#[cfg(not(target_family = "wasm"))]
#[derive(diesel::QueryableByName)]
struct OpenCodeSessionRow {
    #[diesel(sql_type = diesel::sql_types::Text)]
    id: String,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
    title: Option<String>,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
    directory: Option<String>,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    time_created: i64,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    time_updated: i64,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
    model_json: Option<String>,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    tokens_input: i64,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    tokens_output: i64,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    tokens_reasoning: i64,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    message_count: i64,
}

#[cfg(not(target_family = "wasm"))]
#[derive(diesel::QueryableByName)]
struct OpenCodePreviewRow {
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
    role: Option<String>,
    #[diesel(sql_type = diesel::sql_types::Text)]
    part_data: String,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    time_created: i64,
}

#[cfg(not(target_family = "wasm"))]
fn scan_opencode_databases(
    home: &Path,
    limit: SessionLimit,
    sessions: &mut Vec<AgentSession>,
    issues: &mut Vec<SessionScanIssue>,
) {
    let data_dir = std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"))
        .join("opencode");
    let Ok(entries) = data_dir.read_dir() else {
        return;
    };
    let mut database_paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(is_opencode_database_name)
        })
        .collect::<Vec<_>>();
    database_paths.sort();
    for database_path in database_paths {
        match read_opencode_database(&database_path, limit) {
            Ok(database_sessions) => sessions.extend(database_sessions),
            Err(message) => issues.push(SessionScanIssue {
                agent: CLIAgent::OpenCode,
                path: database_path,
                message,
            }),
        }
    }
}

#[cfg(not(target_family = "wasm"))]
fn is_opencode_database_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".db") else {
        return false;
    };
    stem == "opencode"
        || stem.strip_prefix("opencode-").is_some_and(|suffix| {
            !suffix.is_empty()
                && suffix
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || "_.-".contains(character))
        })
}

#[cfg(not(target_family = "wasm"))]
fn sqlite_columns(
    connection: &mut SqliteConnection,
    table: &str,
) -> Result<HashSet<String>, String> {
    diesel::sql_query(format!("PRAGMA table_info(\"{table}\")"))
        .load::<SqliteColumn>(connection)
        .map(|columns| columns.into_iter().map(|column| column.name).collect())
        .map_err(|error| error.to_string())
}

#[cfg(not(target_family = "wasm"))]
fn optional_text_column(columns: &HashSet<String>, name: &str) -> String {
    if columns.contains(name) {
        format!("s.\"{name}\"")
    } else {
        "NULL".to_owned()
    }
}

#[cfg(not(target_family = "wasm"))]
fn optional_number_column(columns: &HashSet<String>, name: &str) -> String {
    if columns.contains(name) {
        format!("COALESCE(s.\"{name}\", 0)")
    } else {
        "0".to_owned()
    }
}

#[cfg(not(target_family = "wasm"))]
fn read_opencode_database(
    database_path: &Path,
    limit: SessionLimit,
) -> Result<Vec<AgentSession>, String> {
    let database_url = format!("file:{}?mode=ro", database_path.display());
    let mut connection =
        SqliteConnection::establish(&database_url).map_err(|error| error.to_string())?;
    connection
        .batch_execute("PRAGMA query_only = ON; PRAGMA busy_timeout = 1000;")
        .map_err(|error| error.to_string())?;
    let session_columns = sqlite_columns(&mut connection, "session")?;
    if !["id", "time_created", "time_updated"]
        .into_iter()
        .all(|column| session_columns.contains(column))
    {
        return Ok(Vec::new());
    }
    let message_columns = sqlite_columns(&mut connection, "message")?;
    let message_count = if ["id", "session_id", "data"]
        .into_iter()
        .all(|column| message_columns.contains(column))
    {
        "(SELECT COUNT(*) FROM message m WHERE m.session_id = s.id AND json_extract(m.data, '$.role') IN ('user','assistant'))"
    } else {
        "0"
    };
    let parent_filter = if session_columns.contains("parent_id") {
        "AND s.parent_id IS NULL"
    } else {
        ""
    };
    let archived_filter = if session_columns.contains("time_archived") {
        "AND s.time_archived IS NULL"
    } else {
        ""
    };
    let row_limit = limit
        .count()
        .map(|limit| format!("LIMIT {limit}"))
        .unwrap_or_default();
    let query = format!(
        "SELECT s.id AS id,
                {} AS title,
                {} AS directory,
                s.time_created AS time_created,
                s.time_updated AS time_updated,
                {} AS model_json,
                {} AS tokens_input,
                {} AS tokens_output,
                {} AS tokens_reasoning,
                {message_count} AS message_count
         FROM session s
         WHERE 1 = 1 {parent_filter} {archived_filter}
         ORDER BY CASE WHEN s.time_updated > 0 THEN s.time_updated ELSE s.time_created END DESC
         {row_limit}",
        optional_text_column(&session_columns, "title"),
        optional_text_column(&session_columns, "directory"),
        optional_text_column(&session_columns, "model"),
        optional_number_column(&session_columns, "tokens_input"),
        optional_number_column(&session_columns, "tokens_output"),
        optional_number_column(&session_columns, "tokens_reasoning"),
    );
    let rows = diesel::sql_query(query)
        .load::<OpenCodeSessionRow>(&mut connection)
        .map_err(|error| error.to_string())?;
    let part_columns = sqlite_columns(&mut connection, "part")?;
    rows.into_iter()
        .map(|row| {
            open_code_session_from_row(
                &mut connection,
                database_path,
                row,
                &message_columns,
                &part_columns,
            )
        })
        .collect()
}

#[cfg(not(target_family = "wasm"))]
fn open_code_session_from_row(
    connection: &mut SqliteConnection,
    database_path: &Path,
    row: OpenCodeSessionRow,
    message_columns: &HashSet<String>,
    part_columns: &HashSet<String>,
) -> Result<AgentSession, String> {
    let can_preview = ["id", "session_id", "data", "time_created"]
        .into_iter()
        .all(|column| message_columns.contains(column))
        && ["message_id", "data", "time_created"]
            .into_iter()
            .all(|column| part_columns.contains(column));
    let mut previews = if can_preview {
        diesel::sql_query(
            "SELECT json_extract(m.data, '$.role') AS role,
                    p.data AS part_data,
                    p.time_created AS time_created
             FROM (SELECT id, data, time_created FROM message
                   WHERE session_id = ?
                   ORDER BY time_created DESC, id DESC
                   LIMIT 100) m
             JOIN part p ON p.message_id = m.id
             WHERE json_extract(m.data, '$.role') IN ('user','assistant')
               AND json_extract(p.data, '$.type') = 'text'
             ORDER BY p.time_created DESC
             LIMIT 6",
        )
        .bind::<diesel::sql_types::Text, _>(&row.id)
        .load::<OpenCodePreviewRow>(connection)
        .map_err(|error| error.to_string())?
    } else {
        Vec::new()
    };
    let preview_messages_truncated = previews.len() > PREVIEW_LIMIT;
    previews.truncate(PREVIEW_LIMIT);
    previews.reverse();
    let preview_messages = previews
        .into_iter()
        .filter_map(|preview| {
            let value = serde_json::from_str::<Value>(&preview.part_data).ok()?;
            let text = extract_text(Some(&value))?;
            Some(SessionPreviewMessage {
                role: preview
                    .role
                    .as_deref()
                    .map(preview_role)
                    .unwrap_or(PreviewRole::Unknown),
                text: truncate(text.trim(), PREVIEW_TEXT_LIMIT),
                timestamp: epoch_timestamp(preview.time_created),
            })
        })
        .collect::<Vec<_>>();
    let first_user_prompt = (!preview_messages_truncated)
        .then(|| {
            preview_messages
                .iter()
                .find(|message| message.role == PreviewRole::User)
                .map(|message| message.text.clone())
        })
        .flatten();
    let last_user_prompt = preview_messages
        .iter()
        .rev()
        .find(|message| message.role == PreviewRole::User)
        .map(|message| message.text.clone());
    let created_at = epoch_timestamp(row.time_created);
    let updated_at = epoch_timestamp(row.time_updated);
    let modified_at = updated_at
        .or(created_at)
        .or_else(|| {
            database_path
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .map(DateTime::<Utc>::from)
        })
        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
    let cwd = row
        .directory
        .filter(|directory| !directory.trim().is_empty())
        .map(PathBuf::from);
    let model = row.model_json.as_deref().and_then(open_code_model_id);
    let title = row
        .title
        .filter(|title| !title.trim().is_empty())
        .or_else(|| {
            first_user_prompt
                .clone()
                .map(|prompt| truncate(&prompt, TITLE_LIMIT))
        })
        .unwrap_or_else(|| format!("OpenCode {}", truncate(&row.id, 8)));
    let resume_command =
        build_resume_command(CLIAgent::OpenCode, &row.id, cwd.as_deref(), None, None);
    Ok(AgentSession {
        id: format!("OpenCode:{}#{}", database_path.display(), row.id),
        agent: CLIAgent::OpenCode,
        session_id: row.id,
        title,
        cwd,
        branch: None,
        model,
        transcript_path: database_path.to_path_buf(),
        codex_home: None,
        created_at,
        updated_at,
        modified_at,
        message_count: row.message_count.max(0) as usize,
        total_tokens: row
            .tokens_input
            .saturating_add(row.tokens_output)
            .saturating_add(row.tokens_reasoning)
            .max(0) as u64,
        preview_messages,
        preview_messages_truncated,
        first_user_prompt,
        last_user_prompt,
        queued_message_count: 0,
        subagent_transcript_count: 0,
        resume_command,
    })
}

#[cfg(not(target_family = "wasm"))]
fn open_code_model_id(model: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(model).ok()?;
    ["id", "modelID", "model_id"]
        .into_iter()
        .find_map(|key| value.get(key).and_then(Value::as_str))
        .map(str::to_owned)
}

fn epoch_timestamp(value: i64) -> Option<DateTime<Utc>> {
    (value > 0)
        .then(|| parse_timestamp(&Value::from(value)))
        .flatten()
}

fn parse_session_file(agent: CLIAgent, path: &Path) -> Result<Option<AgentSession>, String> {
    let metadata = path.metadata().map_err(|error| error.to_string())?;
    let modified_at = DateTime::<Utc>::from(metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH));
    let mut accumulator = SessionAccumulator::new(agent, path, modified_at);
    parse_session_contents(&mut accumulator, path)?;
    if agent == CLIAgent::Grok {
        let history = path.with_file_name("chat_history.jsonl");
        if history.is_file() {
            parse_session_contents(&mut accumulator, &history)?;
        }
    }
    if agent == CLIAgent::Cline
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| matches!(name, "task_metadata.json" | "metadata.json"))
    {
        for name in ["api_conversation_history.json", "messages.json"] {
            let messages = path.with_file_name(name);
            if messages.is_file() {
                parse_session_contents(&mut accumulator, &messages)?;
            }
        }
    }
    Ok(accumulator.finish())
}

fn parse_session_contents(accumulator: &mut SessionAccumulator, path: &Path) -> Result<(), String> {
    if path
        .extension()
        .is_some_and(|extension| extension == "jsonl")
    {
        let reader = BufReader::new(File::open(path).map_err(|error| error.to_string())?);
        let mut parsed_records = 0;
        let mut malformed_records = 0;
        for line in reader.lines() {
            let line = line.map_err(|error| error.to_string())?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(&line) {
                Ok(value) => {
                    parsed_records += 1;
                    accumulator.visit(&value);
                }
                Err(_) => malformed_records += 1,
            }
        }
        if parsed_records == 0 && malformed_records > 0 {
            return Err("The transcript contains no valid JSON records.".to_owned());
        }
    } else {
        let mut file = File::open(path)
            .map_err(|error| error.to_string())?
            .take(64 * 1_024 * 1_024);
        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .map_err(|error| error.to_string())?;
        let value: Value = serde_json::from_str(&contents).map_err(|error| error.to_string())?;
        accumulator.visit(&value);
    }
    Ok(())
}

struct SessionAccumulator {
    agent: CLIAgent,
    path: PathBuf,
    modified_at: DateTime<Utc>,
    session_id: Option<String>,
    title: Option<String>,
    cwd: Option<PathBuf>,
    branch: Option<String>,
    model: Option<String>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
    total_tokens: u64,
    previews: Vec<SessionPreviewMessage>,
    preview_messages_truncated: bool,
    first_user_prompt: Option<String>,
    last_user_prompt: Option<String>,
    message_count: usize,
    queued_message_count: usize,
}

impl SessionAccumulator {
    fn new(agent: CLIAgent, path: &Path, modified_at: DateTime<Utc>) -> Self {
        Self {
            agent,
            path: path.to_path_buf(),
            modified_at,
            session_id: None,
            title: None,
            cwd: None,
            branch: None,
            model: None,
            created_at: None,
            updated_at: None,
            total_tokens: 0,
            previews: Vec::new(),
            preview_messages_truncated: false,
            first_user_prompt: None,
            last_user_prompt: None,
            message_count: 0,
            queued_message_count: 0,
        }
    }

    fn visit(&mut self, value: &Value) {
        match value {
            Value::Array(values) => values.iter().for_each(|value| self.visit(value)),
            Value::Object(object) => {
                self.capture_metadata(object);
                self.capture_message(object);
                for (key, child) in object {
                    if !matches!(
                        key.as_str(),
                        "content" | "text" | "message" | "query" | "response"
                    ) {
                        self.visit(child);
                    }
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }

    fn capture_metadata(&mut self, object: &serde_json::Map<String, Value>) {
        self.session_id = self.session_id.take().or_else(|| {
            string_field(
                object,
                &[
                    "sessionId",
                    "sessionID",
                    "session_id",
                    "conversationId",
                    "conversation_id",
                    "taskId",
                ],
            )
            .or_else(|| {
                object
                    .keys()
                    .any(|key| {
                        matches!(
                            key.as_str(),
                            "cwd" | "directory" | "project_path" | "workingDirectory"
                        )
                    })
                    .then(|| string_field(object, &["id"]))
                    .flatten()
            })
        });
        self.title = self.title.take().or_else(|| {
            string_field(object, &["title", "summary", "name"])
                .map(|value| truncate(&value, TITLE_LIMIT))
        });
        self.cwd = self.cwd.take().or_else(|| {
            string_field(
                object,
                &["cwd", "directory", "project_path", "workingDirectory"],
            )
            .map(PathBuf::from)
        });
        self.branch = self
            .branch
            .take()
            .or_else(|| string_field(object, &["branch", "git_branch"]));
        self.model = self
            .model
            .take()
            .or_else(|| string_field(object, &["model", "model_id", "modelId"]));
        for key in ["timestamp", "created_at", "createdAt", "time_created"] {
            if let Some(timestamp) = object.get(key).and_then(parse_timestamp) {
                self.created_at = Some(
                    self.created_at
                        .map_or(timestamp, |current| current.min(timestamp)),
                );
                self.updated_at = Some(
                    self.updated_at
                        .map_or(timestamp, |current| current.max(timestamp)),
                );
            }
        }
        for key in ["updated_at", "updatedAt", "time_updated"] {
            if let Some(timestamp) = object.get(key).and_then(parse_timestamp) {
                self.updated_at = Some(
                    self.updated_at
                        .map_or(timestamp, |current| current.max(timestamp)),
                );
            }
        }
        self.total_tokens = self.total_tokens.saturating_add(number_field(
            object,
            &["total_tokens", "tokens", "input_tokens", "output_tokens"],
        ));
        if object.get("queued").and_then(Value::as_bool) == Some(true) {
            self.queued_message_count += 1;
        }
    }

    fn capture_message(&mut self, object: &serde_json::Map<String, Value>) {
        let role = string_field(object, &["role", "type", "speaker"])
            .map(|value| preview_role(&value))
            .unwrap_or(PreviewRole::Unknown);
        if role == PreviewRole::Unknown {
            return;
        }
        let text = extract_text(object.get("content"))
            .or_else(|| extract_text(object.get("text")))
            .or_else(|| extract_text(object.get("message")))
            .or_else(|| extract_text(object.get("query")))
            .or_else(|| extract_text(object.get("response")));
        let Some(text) = text.filter(|text| !text.trim().is_empty()) else {
            return;
        };
        if matches!(role, PreviewRole::User | PreviewRole::Assistant) {
            self.message_count += 1;
        }
        let full_text = text.trim();
        if role == PreviewRole::User {
            if self.first_user_prompt.is_none() {
                self.first_user_prompt = Some(truncate(full_text, FIRST_PROMPT_LIMIT));
            }
            self.last_user_prompt = Some(truncate(full_text, PREVIEW_TEXT_LIMIT));
        }
        let text = truncate(full_text, PREVIEW_TEXT_LIMIT);
        self.previews.push(SessionPreviewMessage {
            role,
            text,
            timestamp: object.get("timestamp").and_then(parse_timestamp),
        });
        if self.previews.len() > PREVIEW_LIMIT {
            self.previews.remove(0);
            self.preview_messages_truncated = true;
        }
    }

    fn finish(self) -> Option<AgentSession> {
        let fallback_id = self.path.file_stem()?.to_string_lossy().to_string();
        let session_id = self.session_id.unwrap_or(fallback_id);
        let subagent_transcript_count = count_subagent_transcripts(self.agent, &self.path);
        let first_user_prompt = self.first_user_prompt;
        let last_user_prompt = self.last_user_prompt;
        let title = self
            .title
            .or_else(|| {
                first_user_prompt
                    .clone()
                    .map(|value| truncate(&value, TITLE_LIMIT))
            })
            .unwrap_or_else(|| "Untitled session".to_owned());
        let resume_command = build_resume_command(
            self.agent,
            &session_id,
            self.cwd.as_deref(),
            Some(&self.path),
            None,
        );
        Some(AgentSession {
            id: format!(
                "{}:{}",
                self.agent.to_serialized_name(),
                self.path.display()
            ),
            agent: self.agent,
            session_id,
            title,
            cwd: self.cwd,
            branch: self.branch,
            model: self.model,
            transcript_path: self.path,
            codex_home: None,
            created_at: self.created_at,
            updated_at: self.updated_at,
            modified_at: self.modified_at,
            message_count: self.message_count,
            total_tokens: self.total_tokens,
            preview_messages: self.previews,
            preview_messages_truncated: self.preview_messages_truncated,
            first_user_prompt,
            last_user_prompt,
            queued_message_count: self.queued_message_count,
            subagent_transcript_count,
            resume_command,
        })
    }
}

fn count_subagent_transcripts(agent: CLIAgent, transcript_path: &Path) -> usize {
    let directory = match agent {
        CLIAgent::Claude => transcript_path.with_extension("").join("subagents"),
        CLIAgent::OhMyPi => transcript_path.with_extension(""),
        CLIAgent::Gemini
        | CLIAgent::Codex
        | CLIAgent::Droid
        | CLIAgent::OpenCode
        | CLIAgent::Copilot
        | CLIAgent::Pi
        | CLIAgent::CursorCli
        | CLIAgent::Hermes
        | CLIAgent::Antigravity
        | CLIAgent::Grok
        | CLIAgent::Cline
        | CLIAgent::Devin
        | CLIAgent::Amp
        | CLIAgent::Auggie
        | CLIAgent::Goose
        | CLIAgent::Vibe
        | CLIAgent::Trae
        | CLIAgent::QwenCode
        | CLIAgent::Unknown => return 0,
    };
    let Ok(entries) = directory.read_dir() else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_ok_and(|file_type| file_type.is_file())
                && entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "jsonl")
                && (agent != CLIAgent::Claude
                    || entry.file_name().to_string_lossy().starts_with("agent-"))
        })
        .count()
}

fn string_field(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn number_field(object: &serde_json::Map<String, Value>, keys: &[&str]) -> u64 {
    keys.iter()
        .filter_map(|key| object.get(*key).and_then(Value::as_u64))
        .sum()
}

fn extract_text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) => Some(value.clone()),
        Value::Array(values) => {
            let text = values
                .iter()
                .filter_map(|value| extract_text(Some(value)))
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then_some(text)
        }
        Value::Object(object) => ["text", "content", "message"]
            .into_iter()
            .find_map(|key| extract_text(object.get(key))),
        Value::Null | Value::Bool(_) | Value::Number(_) => None,
    }
}

fn preview_role(value: &str) -> PreviewRole {
    let value = value.to_ascii_lowercase();
    if value.contains("user") || value == "human" || value.contains("prompt") {
        PreviewRole::User
    } else if value.contains("assistant") || value == "ai" || value.contains("agent") {
        PreviewRole::Assistant
    } else if value.contains("system") {
        PreviewRole::System
    } else if value.contains("tool") || value.contains("function") {
        PreviewRole::Tool
    } else {
        PreviewRole::Unknown
    }
}

fn parse_timestamp(value: &Value) -> Option<DateTime<Utc>> {
    if let Some(value) = value.as_str() {
        return DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|value| value.with_timezone(&Utc));
    }
    let number = value.as_i64()?;
    let milliseconds = if number > 10_000_000_000_000_000 {
        number / 1_000_000
    } else if number > 10_000_000_000_000 {
        number / 1_000
    } else if number > 10_000_000_000 {
        number
    } else {
        number * 1_000
    };
    DateTime::from_timestamp_millis(milliseconds)
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let result: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{result}…")
    } else {
        result
    }
}

pub fn build_resume_command(
    agent: CLIAgent,
    session_id: &str,
    cwd: Option<&Path>,
    transcript_path: Option<&Path>,
    codex_home: Option<&Path>,
) -> String {
    let target = if agent == CLIAgent::OhMyPi {
        transcript_path
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| session_id.to_owned())
    } else {
        session_id.to_owned()
    };
    let target = quote_shell_argument(&target);
    let base = agent.command_prefix();
    let invocation = match agent {
        CLIAgent::Codex => format!("{base} resume {target}"),
        CLIAgent::OpenCode | CLIAgent::Pi => format!("{base} --session {target}"),
        CLIAgent::Copilot => format!("{base} --resume={target}"),
        CLIAgent::Cline => format!("{base} --id {target}"),
        CLIAgent::Antigravity => format!("{base} --conversation {target}"),
        CLIAgent::Claude
        | CLIAgent::Gemini
        | CLIAgent::Droid
        | CLIAgent::OhMyPi
        | CLIAgent::CursorCli
        | CLIAgent::Hermes
        | CLIAgent::Grok
        | CLIAgent::Devin => format!("{base} --resume {target}"),
        CLIAgent::Amp
        | CLIAgent::Auggie
        | CLIAgent::Goose
        | CLIAgent::Vibe
        | CLIAgent::Trae
        | CLIAgent::QwenCode
        | CLIAgent::Unknown => return String::new(),
    };
    let invocation = if agent == CLIAgent::Codex
        && let Some(codex_home) = codex_home
    {
        if cfg!(windows) {
            format!(
                "set \"CODEX_HOME={}\" && {invocation}",
                codex_home.display()
            )
        } else {
            format!(
                "CODEX_HOME={} {invocation}",
                quote_shell_argument(&codex_home.to_string_lossy())
            )
        }
    } else {
        invocation
    };
    match cwd {
        Some(cwd) if cfg!(windows) => format!(
            "cd /d {} && {invocation}",
            quote_shell_argument(&cwd.to_string_lossy())
        ),
        Some(cwd) => format!(
            "cd {} && {invocation}",
            quote_shell_argument(&cwd.to_string_lossy())
        ),
        None => invocation,
    }
}

#[cfg(not(target_family = "wasm"))]
fn deletion_targets(session: &AgentSession, home: &Path) -> Result<Vec<PathBuf>, String> {
    if matches!(
        session.agent,
        CLIAgent::Codex | CLIAgent::OpenCode | CLIAgent::Antigravity
    ) {
        return Err(
            "This agent stores session indexes that Spirit cannot safely update.".to_owned(),
        );
    }
    if !SUPPORTED_AGENTS.contains(&session.agent) {
        return Err("This agent is not supported by Session History.".to_owned());
    }
    let source = agent_sources(home)
        .into_iter()
        .find(|source| {
            source.agent == session.agent
                && session.transcript_path.starts_with(&source.root)
                && is_session_file(source, &session.transcript_path)
        })
        .ok_or_else(|| "The transcript is outside the agent's session directory.".to_owned())?;
    let root = source
        .root
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let mut candidates = if matches!(session.agent, CLIAgent::Cline | CLIAgent::Grok) {
        vec![(
            session
                .transcript_path
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| "The session directory is unavailable.".to_owned())?,
            source.root.clone(),
            true,
        )]
    } else {
        vec![(session.transcript_path.clone(), source.root.clone(), false)]
    };
    if session.agent == CLIAgent::Claude {
        let session_id = session
            .transcript_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty() && *stem != "." && *stem != "..")
            .ok_or_else(|| "The Claude session ID is invalid.".to_owned())?;
        let session_dir = session.transcript_path.with_file_name(session_id);
        if session_dir.exists() {
            candidates.insert(0, (session_dir, source.root.clone(), true));
        }
        let session_env_root = source
            .root
            .parent()
            .ok_or_else(|| "The Claude session root is invalid.".to_owned())?
            .join("session-env");
        let session_env = session_env_root.join(session_id);
        if session_env.exists() {
            candidates.insert(0, (session_env, session_env_root, true));
        }
    }

    let mut targets = Vec::with_capacity(candidates.len());
    for (candidate, candidate_root, expects_directory) in candidates {
        let metadata = candidate
            .symlink_metadata()
            .map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink()
            || (expects_directory && !metadata.is_dir())
            || (!expects_directory && !metadata.is_file())
        {
            return Err("The session target has an unexpected file type.".to_owned());
        }
        let candidate_root = if candidate_root == source.root {
            root.clone()
        } else {
            candidate_root
                .canonicalize()
                .map_err(|error| error.to_string())?
        };
        let candidate = candidate
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if candidate == candidate_root || !candidate.starts_with(&candidate_root) {
            return Err("The session target failed root validation.".to_owned());
        }
        targets.push(candidate);
    }
    Ok(targets)
}

#[cfg(not(target_family = "wasm"))]
pub fn move_session_to_trash(session: &AgentSession, home: &Path) -> Result<(), String> {
    for target in deletion_targets(session, home)? {
        trash::delete(target).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(target_family = "wasm")]
pub fn move_session_to_trash(session: &AgentSession, home: &Path) -> Result<(), String> {
    let _ = (session, home);
    Err("Moving sessions to Trash is unavailable on this platform.".to_owned())
}

fn quote_shell_argument(value: &str) -> String {
    if cfg!(windows) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanState {
    Idle,
    Loading,
}

#[derive(Clone, Debug)]
pub enum AgentSessionHistoryEvent {
    Updated,
}

pub struct AgentSessionHistoryModel {
    sessions: Vec<AgentSession>,
    issues: Vec<SessionScanIssue>,
    state: ScanState,
    generation: u64,
    last_scan: Option<Instant>,
    limit: SessionLimit,
}

impl AgentSessionHistoryModel {
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            issues: Vec::new(),
            state: ScanState::Idle,
            generation: 0,
            last_scan: None,
            limit: SessionLimit::default(),
        }
    }

    pub fn sessions(&self) -> &[AgentSession] {
        &self.sessions
    }
    pub fn issues(&self) -> &[SessionScanIssue] {
        &self.issues
    }
    pub fn state(&self) -> ScanState {
        self.state
    }

    pub fn refresh(&mut self, force: bool, ctx: &mut ModelContext<Self>) {
        if !force
            && self
                .last_scan
                .is_some_and(|last_scan| last_scan.elapsed() < CACHE_TTL)
        {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let limit = self.limit;
        self.state = ScanState::Loading;
        ctx.emit(AgentSessionHistoryEvent::Updated);
        let home = dirs::home_dir();
        ctx.spawn(
            async move { home.map(|home| scan_home(&home, limit)).unwrap_or_default() },
            move |model, (sessions, issues), ctx| {
                if model.generation != generation {
                    return;
                }
                model.sessions = sessions;
                model.issues = issues;
                model.state = ScanState::Idle;
                model.last_scan = Some(Instant::now());
                ctx.emit(AgentSessionHistoryEvent::Updated);
            },
        );
    }

    pub fn set_limit(&mut self, limit: SessionLimit, ctx: &mut ModelContext<Self>) {
        if self.limit == limit {
            return;
        }
        self.limit = limit;
        self.last_scan = None;
        self.refresh(true, ctx);
    }
}

impl Entity for AgentSessionHistoryModel {
    type Event = AgentSessionHistoryEvent;
}
impl SingletonEntity for AgentSessionHistoryModel {}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
