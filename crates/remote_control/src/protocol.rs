use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Unauthorized,
    ForbiddenOrigin,
    BadHost,
    FeatureDisabled,
    InvalidRequest,
    UnknownCommand,
    NotFound,
    NotAtPrompt,
    NoAgentSession,
    TerminalReadOnly,
    Conflict,
    NotGitProject,
    GitFailed,
    PayloadTooLarge,
    TooManyAttachments,
    RateLimited,
    BridgeUnavailable,
    Unsupported,
}

impl ErrorCode {
    pub fn http_status(self) -> u16 {
        match self {
            ErrorCode::Unauthorized => 401,
            ErrorCode::ForbiddenOrigin => 403,
            ErrorCode::BadHost => 421,
            ErrorCode::FeatureDisabled => 403,
            ErrorCode::InvalidRequest => 400,
            ErrorCode::UnknownCommand => 400,
            ErrorCode::NotFound => 404,
            ErrorCode::NotAtPrompt => 409,
            ErrorCode::NoAgentSession => 409,
            ErrorCode::TerminalReadOnly => 409,
            ErrorCode::Conflict => 409,
            ErrorCode::NotGitProject => 409,
            ErrorCode::GitFailed => 500,
            ErrorCode::PayloadTooLarge => 413,
            ErrorCode::TooManyAttachments => 429,
            ErrorCode::RateLimited => 429,
            ErrorCode::BridgeUnavailable => 503,
            ErrorCode::Unsupported => 501,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommandError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl CommandError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn not_found(what: &str) -> Self {
        Self::new(ErrorCode::NotFound, format!("{what} no longer exists"))
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidRequest, message)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApiError {
    pub error: CommandError,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            error: CommandError::new(code, message),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CommandName {
    #[serde(rename = "app.ping")]
    AppPing,
    #[serde(rename = "project.open")]
    ProjectOpen,
    #[serde(rename = "home.activate")]
    HomeActivate,
    #[serde(rename = "screen.activate")]
    ScreenActivate,
    #[serde(rename = "tab.activate")]
    TabActivate,
    #[serde(rename = "pane.focus")]
    PaneFocus,
    #[serde(rename = "desktop.reveal")]
    DesktopReveal,
    #[serde(rename = "terminal.selection")]
    TerminalSelection,
    #[serde(rename = "terminal.mirror")]
    TerminalMirror,
    #[serde(rename = "terminal.frame")]
    TerminalFrame,
    #[serde(rename = "terminal.interact")]
    TerminalInteract,
    #[serde(rename = "terminal.attach")]
    TerminalAttach,
    #[serde(rename = "terminal.detach")]
    TerminalDetach,
    #[serde(rename = "terminal.input")]
    TerminalInput,
    #[serde(rename = "terminal.paste")]
    TerminalPaste,
    #[serde(rename = "terminal.run_command")]
    TerminalRunCommand,
    #[serde(rename = "terminal.agent_submit")]
    TerminalAgentSubmit,
    #[serde(rename = "terminal.agent_insert")]
    TerminalAgentInsert,
    #[serde(rename = "terminal.signal")]
    TerminalSignal,
    #[serde(rename = "terminal.create")]
    TerminalCreate,
    #[serde(rename = "agent.launch")]
    AgentLaunch,
    #[serde(rename = "worktree.create")]
    WorktreeCreate,
    #[serde(rename = "worktree.dirty_check")]
    WorktreeDirtyCheck,
    #[serde(rename = "worktree.delete")]
    WorktreeDelete,
    #[serde(rename = "worktree.rename")]
    WorktreeRename,
    #[serde(rename = "tab.close")]
    TabClose,
    #[serde(rename = "history.list")]
    HistoryList,
    #[serde(rename = "history.refresh")]
    HistoryRefresh,
    #[serde(rename = "history.resume")]
    HistoryResume,
    #[serde(rename = "project.register")]
    ProjectRegister,
    #[serde(rename = "project.clone")]
    ProjectClone,
    #[serde(rename = "project.clone_cancel")]
    ProjectCloneCancel,
    #[serde(rename = "project.create")]
    ProjectCreate,
    #[serde(rename = "project.rename")]
    ProjectRename,
    #[serde(rename = "project.remove")]
    ProjectRemove,
    #[serde(rename = "project.reveal")]
    ProjectReveal,
    #[serde(rename = "fs.list_dirs")]
    FsListDirs,
    #[serde(rename = "devices.list")]
    DevicesList,
    #[serde(rename = "devices.revoke")]
    DevicesRevoke,
    #[serde(rename = "devices.rename")]
    DevicesRename,
}

impl CommandName {
    pub fn as_str(self) -> &'static str {
        match self {
            CommandName::AppPing => "app.ping",
            CommandName::ProjectOpen => "project.open",
            CommandName::HomeActivate => "home.activate",
            CommandName::ScreenActivate => "screen.activate",
            CommandName::TabActivate => "tab.activate",
            CommandName::PaneFocus => "pane.focus",
            CommandName::DesktopReveal => "desktop.reveal",
            CommandName::TerminalSelection => "terminal.selection",
            CommandName::TerminalMirror => "terminal.mirror",
            CommandName::TerminalFrame => "terminal.frame",
            CommandName::TerminalInteract => "terminal.interact",
            CommandName::TerminalAttach => "terminal.attach",
            CommandName::TerminalDetach => "terminal.detach",
            CommandName::TerminalInput => "terminal.input",
            CommandName::TerminalPaste => "terminal.paste",
            CommandName::TerminalRunCommand => "terminal.run_command",
            CommandName::TerminalAgentSubmit => "terminal.agent_submit",
            CommandName::TerminalAgentInsert => "terminal.agent_insert",
            CommandName::TerminalSignal => "terminal.signal",
            CommandName::TerminalCreate => "terminal.create",
            CommandName::AgentLaunch => "agent.launch",
            CommandName::WorktreeCreate => "worktree.create",
            CommandName::WorktreeDirtyCheck => "worktree.dirty_check",
            CommandName::WorktreeDelete => "worktree.delete",
            CommandName::WorktreeRename => "worktree.rename",
            CommandName::TabClose => "tab.close",
            CommandName::HistoryList => "history.list",
            CommandName::HistoryRefresh => "history.refresh",
            CommandName::HistoryResume => "history.resume",
            CommandName::ProjectRegister => "project.register",
            CommandName::ProjectClone => "project.clone",
            CommandName::ProjectCloneCancel => "project.clone_cancel",
            CommandName::ProjectCreate => "project.create",
            CommandName::ProjectRename => "project.rename",
            CommandName::ProjectRemove => "project.remove",
            CommandName::ProjectReveal => "project.reveal",
            CommandName::FsListDirs => "fs.list_dirs",
            CommandName::DevicesList => "devices.list",
            CommandName::DevicesRevoke => "devices.revoke",
            CommandName::DevicesRename => "devices.rename",
        }
    }

    pub fn all() -> &'static [CommandName] {
        &[
            CommandName::AppPing,
            CommandName::ProjectOpen,
            CommandName::HomeActivate,
            CommandName::ScreenActivate,
            CommandName::TabActivate,
            CommandName::PaneFocus,
            CommandName::DesktopReveal,
            CommandName::TerminalSelection,
            CommandName::TerminalMirror,
            CommandName::TerminalFrame,
            CommandName::TerminalInteract,
            CommandName::TerminalAttach,
            CommandName::TerminalDetach,
            CommandName::TerminalInput,
            CommandName::TerminalPaste,
            CommandName::TerminalRunCommand,
            CommandName::TerminalAgentSubmit,
            CommandName::TerminalAgentInsert,
            CommandName::TerminalSignal,
            CommandName::TerminalCreate,
            CommandName::AgentLaunch,
            CommandName::WorktreeCreate,
            CommandName::WorktreeDirtyCheck,
            CommandName::WorktreeDelete,
            CommandName::WorktreeRename,
            CommandName::TabClose,
            CommandName::HistoryList,
            CommandName::HistoryRefresh,
            CommandName::HistoryResume,
            CommandName::ProjectRegister,
            CommandName::ProjectClone,
            CommandName::ProjectCloneCancel,
            CommandName::ProjectCreate,
            CommandName::ProjectRename,
            CommandName::ProjectRemove,
            CommandName::ProjectReveal,
            CommandName::FsListDirs,
            CommandName::DevicesList,
            CommandName::DevicesRevoke,
            CommandName::DevicesRename,
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalMode {
    Prompt,
    Running,
    AltScreen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSummary {
    None,
    Working,
    NeedsAttention,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    InProgress,
    Success,
    Failed,
    Blocked,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabKind {
    Terminal,
    Code,
    File,
    AgentPicker,
    Settings,
    Mixed,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneKind {
    Terminal,
    Code,
    File,
    AgentPicker,
    Settings,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKindWire {
    Git,
    Folder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeKindWire {
    Primary,
    Linked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalModeWire {
    Yolo,
    Normal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalSignal {
    Interrupt,
    Eof,
}

impl TerminalSignal {
    pub fn byte(self) -> u8 {
        match self {
            TerminalSignal::Interrupt => 0x03,
            TerminalSignal::Eof => 0x04,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClonePhase {
    Starting,
    Cloning,
    Registering,
    Done,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentSessionSummary {
    pub agent: String,
    pub display_name: String,
    pub status: AgentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_input_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub input_open: bool,
    pub brand_color: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerminalSummary {
    pub terminal_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    pub mode: TerminalMode,
    pub cols: usize,
    pub rows: usize,
    pub read_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentSessionSummary>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaneSnapshot {
    pub id: String,
    pub kind: PaneKind,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<TerminalSummary>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TabSnapshot {
    pub id: String,
    pub title: String,
    pub kind: TabKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_id: Option<String>,
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_title: Option<String>,
    pub agent_summary: AgentSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_pane_id: Option<String>,
    pub panes: Vec<PaneSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SectionSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_id: Option<String>,
    pub title: String,
    pub tabs: Vec<TabSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScreenSnapshot {
    pub id: String,
    pub window_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tab_id: Option<String>,
    pub sections: Vec<SectionSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowSnapshot {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_screen_id: Option<String>,
    pub screens: Vec<ScreenSnapshot>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCounts {
    pub working: usize,
    pub needs_attention: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorktreeSnapshot {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub kind: WorktreeKindWire,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    pub created_ts: i64,
    pub agent_summary: AgentSummary,
    pub open_tab_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectSnapshot {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub kind: ProjectKindWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_branch: Option<String>,
    pub last_opened_ts: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_in_window_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_id: Option<String>,
    pub counts: AgentCounts,
    pub worktrees: Vec<WorktreeSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentSessionSnapshot {
    pub terminal_id: String,
    pub tab_id: String,
    pub screen_id: String,
    pub window_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_id: Option<String>,
    pub workspace_name: String,
    pub session: AgentSessionSummary,
    pub rank: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentCatalogEntry {
    pub index: usize,
    pub id: String,
    pub display_name: String,
    pub installed: bool,
    pub supports_yolo: bool,
    pub brand_color: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub connected_clients: usize,
    pub lan_access: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Features {
    pub ade_workspaces: bool,
    pub session_history: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppSnapshot {
    pub version: u64,
    pub instance_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_window_id: Option<String>,
    pub windows: Vec<WindowSnapshot>,
    pub projects: Vec<ProjectSnapshot>,
    pub sessions: Vec<AgentSessionSnapshot>,
    pub agents: Vec<AgentCatalogEntry>,
    pub server: ServerInfo,
    pub features: Features,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistorySession {
    pub id: String,
    pub agent: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub cwd: String,
    pub modified_ts: i64,
    pub message_count: usize,
    pub brand_color: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_command: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceSummary {
    pub id: String,
    pub label: String,
    pub created_ts: i64,
    pub last_seen_ts: i64,
    pub connected: bool,
    pub current: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "snake_case")]
pub enum ServerEvent {
    #[serde(rename = "terminal.resync")]
    TerminalResync {
        attach_id: u32,
        cols: usize,
        rows: usize,
        mode: TerminalMode,
        snapshot: String,
    },
    #[serde(rename = "terminal.closed")]
    TerminalClosed { attach_id: u32 },
    #[serde(rename = "session.status")]
    SessionStatus {
        terminal_id: String,
        agent: String,
        status: AgentStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    #[serde(rename = "project.clone_progress")]
    ProjectCloneProgress {
        job_id: String,
        phase: ClonePhase,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        percent: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    #[serde(rename = "server.shutting_down")]
    ServerShuttingDown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Hello {
        instance_id: String,
        protocol: u32,
        app_version: String,
        client_id: String,
        capabilities: Vec<String>,
        limits: Vec<LimitEntry>,
    },
    State {
        version: u64,
        snapshot: Box<AppSnapshot>,
    },
    Result {
        id: String,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<CommandError>,
    },
    Event {
        #[serde(flatten)]
        event: ServerEvent,
    },
    Pong {
        ts: i64,
    },
}

impl ServerMessage {
    pub fn ok_result(id: impl Into<String>, data: Value) -> Self {
        ServerMessage::Result {
            id: id.into(),
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn error_result(id: impl Into<String>, error: CommandError) -> Self {
        ServerMessage::Result {
            id: id.into(),
            ok: false,
            data: None,
            error: Some(error),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LimitEntry {
    pub name: String,
    pub value: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Ping {
        ts: i64,
    },
    Command {
        id: String,
        name: CommandName,
        #[serde(default)]
        params: Value,
    },
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
