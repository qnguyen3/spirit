use std::collections::HashSet;
use std::path::PathBuf;

use remote_control::protocol::{CommandError, ErrorCode, HistorySession};
use serde::Deserialize;
use serde_json::{Value, json};
use warp_core::features::FeatureFlag;
use warpui::{AppContext, EntityId, ModelContext, SingletonEntity as _, ViewHandle};

use super::bridge::{CommandOutcome, RemoteControlBridge};
use super::resolve;
use crate::projects::registry::ProjectRegistryModel;
use crate::terminal::cli_agent::CLIAgent;
use crate::terminal::cli_agent_session_history::{
    AgentSession, AgentSessionHistoryModel, SUPPORTED_AGENTS, ScanState, SessionFilter,
    SessionSort, filter_sessions, path_contains,
};
use crate::workspace::WorkspaceRegistry;
use crate::workspace::view::Workspace;

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;
const DEFAULT_BRAND_COLOR: &str = "#8a8f98";

#[derive(Deserialize)]
struct ListSessions {
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    worktree_id: Option<String>,
    #[serde(default)]
    agents: Option<Vec<String>>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct ResumeSession {
    history_id: String,
    #[serde(default)]
    screen_id: Option<String>,
}

fn params<T: for<'de> Deserialize<'de>>(raw: Value) -> Result<T, CommandError> {
    serde_json::from_value(raw)
        .map_err(|error| CommandError::invalid_request(format!("bad parameters: {error}")))
}

fn ensure_enabled() -> Result<(), CommandError> {
    if FeatureFlag::AgentSessionHistory.is_enabled() {
        Ok(())
    } else {
        Err(CommandError::new(
            ErrorCode::FeatureDisabled,
            "Agent session history is not enabled in this build",
        ))
    }
}

fn brand_color(agent: CLIAgent) -> String {
    match agent.brand_color() {
        Some(color) => format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b),
        None => DEFAULT_BRAND_COLOR.to_owned(),
    }
}

fn scan_state_name(state: ScanState) -> &'static str {
    match state {
        ScanState::Idle => "idle",
        ScanState::Loading => "loading",
    }
}

fn wire_session(session: &AgentSession) -> HistorySession {
    let title = Some(session.title.clone()).filter(|title| !title.is_empty());
    let resume_command = Some(session.resume_command.clone())
        .filter(|command| !command.is_empty() && session.has_resumable_content());
    HistorySession {
        id: session.id.clone(),
        agent: session.agent.to_serialized_name(),
        display_name: session.agent.display_name().to_owned(),
        title,
        cwd: session
            .cwd
            .as_ref()
            .map(|cwd| cwd.to_string_lossy().into_owned())
            .unwrap_or_default(),
        modified_ts: session.modified_at.timestamp(),
        message_count: session.message_count,
        brand_color: brand_color(session.agent),
        resume_command,
    }
}

fn requested_agents(agents: Option<Vec<String>>) -> HashSet<CLIAgent> {
    match agents {
        Some(names) => names
            .iter()
            .map(|name| CLIAgent::from_serialized_name(name))
            .collect(),
        None => SUPPORTED_AGENTS.into_iter().collect(),
    }
}

fn scope_paths(project_id: Option<&str>, ctx: &AppContext) -> Result<Vec<PathBuf>, CommandError> {
    let Some(project_id) = project_id else {
        return Ok(Vec::new());
    };
    let project_id = resolve::parse_project_id(project_id)?;
    let registry = ProjectRegistryModel::as_ref(ctx);
    let project = registry
        .project(project_id)
        .ok_or_else(|| CommandError::not_found("that Workspace"))?;
    let mut paths = vec![project.root_path.clone()];
    paths.extend(
        registry
            .worktrees_for_project(project_id)
            .into_iter()
            .filter_map(|worktree| registry.worktree_directory(worktree.id)),
    );
    Ok(paths)
}

fn worktree_scope(
    worktree_id: Option<&str>,
    ctx: &AppContext,
) -> Result<Option<PathBuf>, CommandError> {
    let Some(worktree_id) = worktree_id else {
        return Ok(None);
    };
    let worktree_id = resolve::parse_worktree_id(worktree_id)?;
    ProjectRegistryModel::as_ref(ctx)
        .worktree_directory(worktree_id)
        .map(Some)
        .ok_or_else(|| CommandError::not_found("that worktree"))
}

pub(crate) fn list(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> CommandOutcome {
    let request: ListSessions = match params(raw) {
        Ok(request) => request,
        Err(error) => return CommandOutcome::Immediate(Err(error)),
    };
    if let Err(error) = ensure_enabled() {
        return CommandOutcome::Immediate(Err(error));
    }
    let workspace_paths = match scope_paths(request.project_id.as_deref(), ctx) {
        Ok(paths) => paths,
        Err(error) => return CommandOutcome::Immediate(Err(error)),
    };
    let worktree_path = match worktree_scope(request.worktree_id.as_deref(), ctx) {
        Ok(path) => path,
        Err(error) => return CommandOutcome::Immediate(Err(error)),
    };
    let filter = SessionFilter {
        query: String::new(),
        enabled_agents: requested_agents(request.agents),
        sort: SessionSort::Updated,
        workspace_paths,
        worktree_path,
        hide_empty: false,
    };
    let limit = request.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT);

    let history = AgentSessionHistoryModel::as_ref(ctx);
    let sessions: Vec<HistorySession> = filter_sessions(history.sessions(), &filter)
        .iter()
        .take(limit)
        .map(wire_session)
        .collect();
    let payload = json!({
        "sessions": sessions,
        "state": scan_state_name(history.state()),
        "issues": history.issues().len(),
    });
    CommandOutcome::Immediate(Ok(payload))
}

pub(crate) fn refresh(ctx: &mut ModelContext<RemoteControlBridge>) -> CommandOutcome {
    if let Err(error) = ensure_enabled() {
        return CommandOutcome::Immediate(Err(error));
    }
    AgentSessionHistoryModel::handle(ctx).update(ctx, |history, ctx| history.refresh(true, ctx));
    CommandOutcome::Immediate(Ok(json!({})))
}

pub(crate) fn resume(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> CommandOutcome {
    let request: ResumeSession = match params(raw) {
        Ok(request) => request,
        Err(error) => return CommandOutcome::Immediate(Err(error)),
    };
    if let Err(error) = ensure_enabled() {
        return CommandOutcome::Immediate(Err(error));
    }
    let session = AgentSessionHistoryModel::as_ref(ctx)
        .sessions()
        .iter()
        .find(|session| session.id == request.history_id)
        .cloned();
    let Some(session) = session else {
        return CommandOutcome::Immediate(Err(CommandError::not_found("that session")));
    };
    if !session.has_resumable_content() || session.resume_command.is_empty() {
        return CommandOutcome::Immediate(Err(CommandError::new(
            ErrorCode::Conflict,
            "that session has nothing to resume",
        )));
    }

    let target = match request.screen_id {
        Some(screen_id) => match resolve::screen(&screen_id, ctx) {
            Ok(target) => target,
            Err(error) => return CommandOutcome::Immediate(Err(error)),
        },
        None => match screen_for_session(&session, ctx) {
            Some(target) => target,
            None => {
                return CommandOutcome::Immediate(Err(CommandError::not_found(
                    "a screen to resume into",
                )));
            }
        },
    };
    if let Err(error) = activate_screen(&target, ctx) {
        return CommandOutcome::Immediate(Err(error));
    }

    let workspace = target.workspace;
    let before = tab_ids(&workspace, ctx);
    workspace.update(ctx, |workspace, ctx| {
        workspace.resume_agent_session(&session, ctx)
    });

    match created_tab(&workspace, &before, ctx) {
        Some((tab_id, terminal_id)) => CommandOutcome::Immediate(Ok(json!({
            "tab_id": tab_id.to_string(),
            "terminal_id": terminal_id.map(|id| id.to_string()),
        }))),
        None => CommandOutcome::Immediate(Err(CommandError::new(
            ErrorCode::Conflict,
            "the session could not be resumed",
        ))),
    }
}

fn screen_for_session(session: &AgentSession, ctx: &AppContext) -> Option<resolve::ScreenTarget> {
    let cwd = session.cwd.as_deref();
    let owning = cwd.and_then(|cwd| {
        let registry = ProjectRegistryModel::as_ref(ctx);
        registry
            .projects_mru()
            .into_iter()
            .filter(|project| path_contains(&project.root_path, cwd))
            .max_by_key(|project| project.root_path.as_os_str().len())
            .map(|project| project.id)
    });
    if let Some(project_id) = owning
        && let Some(target) = resolve::screen_for_project(project_id, ctx)
    {
        return Some(target);
    }
    let window_id = resolve::default_window(ctx).ok()?;
    let registry = WorkspaceRegistry::as_ref(ctx);
    registry
        .workspaces_for_window(window_id, ctx)
        .into_iter()
        .find(|workspace| workspace.as_ref(ctx).project_id().is_none())
        .or_else(|| registry.active_workspace(window_id, ctx))
        .map(|workspace| resolve::ScreenTarget {
            window_id,
            workspace,
        })
}

fn activate_screen(
    target: &resolve::ScreenTarget,
    ctx: &mut ModelContext<RemoteControlBridge>,
) -> Result<(), CommandError> {
    let wanted = target.workspace.id();
    if WorkspaceRegistry::as_ref(ctx).active_workspace_view_id(target.window_id) == Some(wanted) {
        return Ok(());
    }
    let index = resolve::screen_index(target.window_id, wanted, ctx)?;
    let host = resolve::project_host(target.window_id, ctx)?;
    host.update(ctx, |host, ctx| host.activate_screen(index, ctx));
    Ok(())
}

fn tab_ids(workspace: &ViewHandle<Workspace>, ctx: &AppContext) -> Vec<EntityId> {
    workspace
        .as_ref(ctx)
        .tabs
        .iter()
        .map(|tab| tab.pane_group.id())
        .collect()
}

fn created_tab(
    workspace: &ViewHandle<Workspace>,
    before: &[EntityId],
    ctx: &AppContext,
) -> Option<(EntityId, Option<EntityId>)> {
    let view = workspace.as_ref(ctx);
    let tab = view
        .tabs
        .iter()
        .find(|tab| !before.contains(&tab.pane_group.id()))
        .or_else(|| view.tabs.get(view.active_tab_index()))?;
    let tab_id = tab.pane_group.id();
    let pane_group = tab.pane_group.clone();
    let group = pane_group.as_ref(ctx);
    let terminal_id = group
        .visible_pane_ids()
        .into_iter()
        .find_map(|pane_id| group.terminal_view_from_pane_id(pane_id, ctx))
        .map(|terminal| terminal.id());
    Some((tab_id, terminal_id))
}
