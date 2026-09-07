use std::collections::HashMap;

use remote_control::protocol::{
    AgentCatalogEntry, AgentCounts, AgentSessionSnapshot, AgentSessionSummary, AgentStatus,
    AgentSummary, AppSnapshot, Features, PaneKind, PaneSnapshot, ProjectKindWire, ProjectSnapshot,
    ScreenSnapshot, SectionSnapshot, ServerInfo, TabKind, TabSnapshot, TerminalMode,
    TerminalSummary, WindowSnapshot, WorktreeKindWire, WorktreeSnapshot,
};
use warp_core::features::FeatureFlag;
use warpui::{AppContext, EntityId, SingletonEntity as _, ViewHandle, WindowId};

use crate::agent_launcher::catalog::{AgentDefinition, agent_catalog, is_installed};
use crate::pane_group::PaneGroup;
use crate::pane_group::pane::PaneId;
use crate::projects::agent_status::{WorktreeAgentSummary, project_counts, summarize_tab};
use crate::projects::registry::ProjectRegistryModel;
use crate::projects::{ProjectKind, Worktree, WorktreeKind};
use crate::terminal::cli_agent::CLIAgent;
use crate::terminal::cli_agent_sessions::{
    CLIAgentInputState, CLIAgentSession, CLIAgentSessionStatus, CLIAgentSessionsModel,
};
use crate::terminal::view::TerminalView;
use crate::workspace::WorkspaceRegistry;
use crate::workspace::view::Workspace;
use crate::workspace::view::worktrees::worktree_sections_for_bindings;

const UNBOUND_SECTION_TITLE: &str = "Tabs";
const DEFAULT_BRAND_COLOR: &str = "#8a8f98";

struct TerminalLocation {
    tab_id: String,
    screen_id: String,
    window_id: String,
    project_id: Option<String>,
    worktree_id: Option<String>,
    workspace_name: String,
}

pub(crate) fn build_snapshot(
    instance_id: &str,
    connected_clients: usize,
    path_env: Option<&str>,
    app: &AppContext,
) -> AppSnapshot {
    let registry = WorkspaceRegistry::as_ref(app);
    let mut workspaces_by_window: HashMap<WindowId, Vec<ViewHandle<Workspace>>> = HashMap::new();
    for (window_id, workspace) in registry.all_workspaces(app) {
        workspaces_by_window
            .entry(window_id)
            .or_default()
            .push(workspace);
    }

    let mut window_ids: Vec<WindowId> = workspaces_by_window.keys().copied().collect();
    window_ids.sort();

    let mut locations: HashMap<EntityId, TerminalLocation> = HashMap::new();
    let mut open_tabs_per_worktree: HashMap<String, usize> = HashMap::new();
    let mut windows = Vec::with_capacity(window_ids.len());
    for window_id in &window_ids {
        let ordered = ordered_workspaces(*window_id, &workspaces_by_window, registry);
        let mut screens = Vec::with_capacity(ordered.len());
        for workspace in &ordered {
            screens.push(build_screen(
                *window_id,
                workspace,
                &mut locations,
                &mut open_tabs_per_worktree,
                app,
            ));
        }
        windows.push(WindowSnapshot {
            id: window_id.to_string(),
            active_screen_id: registry
                .active_workspace_view_id(*window_id)
                .map(|id| id.to_string()),
            screens,
        });
    }

    let projects = build_projects(
        &window_ids,
        &workspaces_by_window,
        &open_tabs_per_worktree,
        app,
    );
    let sessions = build_sessions(&locations, app);
    let agents = build_agent_catalog(path_env);

    AppSnapshot {
        version: 0,
        instance_id: instance_id.to_owned(),
        active_window_id: app.windows().active_window().map(|id| id.to_string()),
        windows,
        projects,
        sessions,
        agents,
        server: ServerInfo {
            connected_clients,
            lan_access: lan_access_enabled(app),
        },
        features: Features {
            ade_workspaces: FeatureFlag::AdeWorkspaces.is_enabled(),
            session_history: FeatureFlag::AgentSessionHistory.is_enabled(),
        },
    }
}

fn lan_access_enabled(app: &AppContext) -> bool {
    FeatureFlag::RemoteControl.is_enabled()
        && crate::settings::RemoteControlSettings::as_ref(app).allows_lan_access()
}

fn ordered_workspaces(
    window_id: WindowId,
    workspaces_by_window: &HashMap<WindowId, Vec<ViewHandle<Workspace>>>,
    registry: &WorkspaceRegistry,
) -> Vec<ViewHandle<Workspace>> {
    let available = workspaces_by_window
        .get(&window_id)
        .cloned()
        .unwrap_or_default();
    let mut ordered = Vec::with_capacity(available.len());
    for screen_id in registry.screen_ids_for_window(window_id) {
        if let Some(handle) = available.iter().find(|handle| handle.id() == screen_id) {
            ordered.push(handle.clone());
        }
    }
    for handle in available {
        if !ordered.iter().any(|existing| existing.id() == handle.id()) {
            ordered.push(handle);
        }
    }
    ordered
}

fn build_screen(
    window_id: WindowId,
    workspace_handle: &ViewHandle<Workspace>,
    locations: &mut HashMap<EntityId, TerminalLocation>,
    open_tabs_per_worktree: &mut HashMap<String, usize>,
    app: &AppContext,
) -> ScreenSnapshot {
    let workspace = workspace_handle.as_ref(app);
    let screen_id = workspace.screen_id().to_string();
    let window_id_string = window_id.to_string();
    let project_id = workspace.project_id().map(|id| id.to_string());
    let name = workspace.workspace_switcher_label(app);
    let active_tab_id = workspace
        .tabs
        .get(workspace.active_tab_index())
        .map(|tab| tab.pane_group.id().to_string());

    let bindings: Vec<Option<crate::projects::WorktreeId>> = (0..workspace.tabs.len())
        .map(|index| workspace.resolved_worktree_id_of_tab(index, app))
        .collect();

    let registry_worktrees: Vec<&Worktree> = match workspace.project_id() {
        Some(project_id) if workspace.worktree_sections_enabled() => {
            ProjectRegistryModel::as_ref(app).worktrees_for_project(project_id)
        }
        Some(_) | None => Vec::new(),
    };

    let mut tabs_by_index: Vec<Option<TabSnapshot>> = Vec::with_capacity(workspace.tabs.len());
    for (index, tab) in workspace.tabs.iter().enumerate() {
        let worktree_id = bindings.get(index).copied().flatten();
        let snapshot = build_tab(
            tab,
            worktree_id,
            &window_id_string,
            &screen_id,
            project_id.as_deref(),
            &name,
            locations,
            app,
        );
        if let Some(worktree_id) = worktree_id {
            *open_tabs_per_worktree
                .entry(worktree_id.to_string())
                .or_insert(0) += 1;
        }
        tabs_by_index.push(Some(snapshot));
    }

    let sections = assemble_sections(&bindings, &registry_worktrees, &mut tabs_by_index, app);

    ScreenSnapshot {
        id: screen_id,
        window_id: window_id_string,
        project_id,
        name,
        active_tab_id,
        sections,
    }
}

fn assemble_sections(
    bindings: &[Option<crate::projects::WorktreeId>],
    registry_worktrees: &[&Worktree],
    tabs_by_index: &mut [Option<TabSnapshot>],
    app: &AppContext,
) -> Vec<SectionSnapshot> {
    let mut sections = Vec::new();
    if registry_worktrees.is_empty() {
        let mut index = 0;
        for (binding, run_length) in
            crate::workspace::view::worktrees::worktree_run_partition(bindings)
        {
            let tabs = take_tabs(tabs_by_index, index..index + run_length);
            index += run_length;
            match binding.and_then(|id| ProjectRegistryModel::as_ref(app).worktree(id)) {
                Some(worktree) => sections.push(SectionSnapshot {
                    worktree_id: Some(worktree.id.to_string()),
                    title: worktree.name.clone(),
                    tabs,
                }),
                None => append_unbound(&mut sections, tabs),
            }
        }
        return sections;
    }

    for (worktree, members) in worktree_sections_for_bindings(bindings, registry_worktrees) {
        let tabs = members
            .iter()
            .filter_map(|index| tabs_by_index.get_mut(*index).and_then(Option::take))
            .collect();
        sections.push(SectionSnapshot {
            worktree_id: Some(worktree.id.to_string()),
            title: worktree.name.clone(),
            tabs,
        });
    }
    let unbound: Vec<TabSnapshot> = tabs_by_index.iter_mut().filter_map(Option::take).collect();
    append_unbound(&mut sections, unbound);
    sections
}

fn take_tabs(
    tabs_by_index: &mut [Option<TabSnapshot>],
    range: std::ops::Range<usize>,
) -> Vec<TabSnapshot> {
    range
        .filter_map(|index| tabs_by_index.get_mut(index).and_then(Option::take))
        .collect()
}

fn append_unbound(sections: &mut Vec<SectionSnapshot>, tabs: Vec<TabSnapshot>) {
    if tabs.is_empty() {
        return;
    }
    match sections
        .last_mut()
        .filter(|section| section.worktree_id.is_none())
    {
        Some(section) => section.tabs.extend(tabs),
        None => sections.push(SectionSnapshot {
            worktree_id: None,
            title: UNBOUND_SECTION_TITLE.to_owned(),
            tabs,
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_tab(
    tab: &crate::tab::TabData,
    worktree_id: Option<crate::projects::WorktreeId>,
    window_id: &str,
    screen_id: &str,
    project_id: Option<&str>,
    workspace_name: &str,
    locations: &mut HashMap<EntityId, TerminalLocation>,
    app: &AppContext,
) -> TabSnapshot {
    let tab_id = tab.pane_group.id().to_string();
    let pane_group = tab.pane_group.as_ref(app);
    let focused_pane_id = pane_group.focused_pane_id(app);
    let visible = pane_group.visible_pane_ids();

    let mut panes = Vec::with_capacity(visible.len());
    for pane_id in visible {
        panes.push(build_pane(
            &tab.pane_group,
            pane_id,
            &tab_id,
            window_id,
            screen_id,
            project_id,
            worktree_id,
            workspace_name,
            locations,
            app,
        ));
    }

    let title = pane_group
        .custom_title(app)
        .or_else(|| {
            panes
                .iter()
                .find(|pane| pane.id == focused_pane_id.to_string())
                .map(|pane| pane.title.clone())
        })
        .or_else(|| panes.first().map(|pane| pane.title.clone()))
        .unwrap_or_else(|| "Tab".to_owned());

    TabSnapshot {
        id: tab_id,
        title,
        kind: tab_kind(&panes),
        worktree_id: worktree_id.map(|id| id.to_string()),
        pinned: tab.pinned,
        group_title: None,
        agent_summary: wire_agent_summary(summarize_tab(tab, app)),
        focused_pane_id: Some(focused_pane_id.to_string()),
        panes,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_pane(
    pane_group_handle: &ViewHandle<PaneGroup>,
    pane_id: PaneId,
    tab_id: &str,
    window_id: &str,
    screen_id: &str,
    project_id: Option<&str>,
    worktree_id: Option<crate::projects::WorktreeId>,
    workspace_name: &str,
    locations: &mut HashMap<EntityId, TerminalLocation>,
    app: &AppContext,
) -> PaneSnapshot {
    let pane_group = pane_group_handle.as_ref(app);
    let terminal_view = pane_group.terminal_view_from_pane_id(pane_id, app);
    let code_view = if terminal_view.is_none() {
        pane_group.code_view_from_pane_id(pane_id, app)
    } else {
        None
    };

    let terminal = terminal_view.as_ref().map(|handle| {
        locations.insert(
            handle.id(),
            TerminalLocation {
                tab_id: tab_id.to_owned(),
                screen_id: screen_id.to_owned(),
                window_id: window_id.to_owned(),
                project_id: project_id.map(str::to_owned),
                worktree_id: worktree_id.map(|id| id.to_string()),
                workspace_name: workspace_name.to_owned(),
            },
        );
        build_terminal_summary(handle, app)
    });

    let kind = match (&terminal, &code_view) {
        (Some(_), _) => PaneKind::Terminal,
        (None, Some(_)) => PaneKind::Code,
        (None, None) => pane_kind_from_id(pane_id),
    };

    let title = terminal
        .as_ref()
        .and_then(|summary| summary.title.clone())
        .unwrap_or_else(|| default_pane_title(kind));

    PaneSnapshot {
        id: pane_id.to_string(),
        kind,
        title,
        terminal,
    }
}

fn pane_kind_from_id(pane_id: PaneId) -> PaneKind {
    let rendered = pane_id.to_string();
    if rendered.contains("Terminal") {
        PaneKind::Terminal
    } else if rendered.contains("Code") {
        PaneKind::Code
    } else if rendered.contains("File") {
        PaneKind::File
    } else if rendered.contains("Settings") {
        PaneKind::Settings
    } else if rendered.contains("Agent Picker") {
        PaneKind::AgentPicker
    } else {
        PaneKind::Other
    }
}

fn default_pane_title(kind: PaneKind) -> String {
    match kind {
        PaneKind::Terminal => "Terminal",
        PaneKind::Code => "Code",
        PaneKind::File => "File",
        PaneKind::AgentPicker => "Agents",
        PaneKind::Settings => "Settings",
        PaneKind::Other => "Pane",
    }
    .to_owned()
}

fn tab_kind(panes: &[PaneSnapshot]) -> TabKind {
    let mut kinds = panes.iter().map(|pane| pane.kind);
    let Some(first) = kinds.next() else {
        return TabKind::Other;
    };
    if kinds.any(|kind| kind != first) {
        return TabKind::Mixed;
    }
    match first {
        PaneKind::Terminal => TabKind::Terminal,
        PaneKind::Code => TabKind::Code,
        PaneKind::File => TabKind::File,
        PaneKind::AgentPicker => TabKind::AgentPicker,
        PaneKind::Settings => TabKind::Settings,
        PaneKind::Other => TabKind::Other,
    }
}

pub(crate) fn build_terminal_summary(
    handle: &ViewHandle<TerminalView>,
    app: &AppContext,
) -> TerminalSummary {
    let view = handle.as_ref(app);
    let (mode, read_only, model_title, cols, rows) = {
        let model = view.model.lock();
        let size = model.block_list().size();
        let mode = if model.is_alt_screen_active() {
            TerminalMode::AltScreen
        } else if view.is_input_box_visible(&model, app) {
            TerminalMode::Prompt
        } else {
            TerminalMode::Running
        };
        (
            mode,
            model.is_read_only(),
            model.custom_title().or_else(|| model.terminal_title()),
            size.columns,
            size.rows,
        )
    };

    let agent = CLIAgentSessionsModel::as_ref(app)
        .session(handle.id())
        .map(agent_summary);

    TerminalSummary {
        terminal_id: handle.id().to_string(),
        title: model_title,
        cwd: view.pwd(),
        mode,
        cols,
        rows,
        read_only,
        agent,
    }
}

fn agent_summary(session: &CLIAgentSession) -> AgentSessionSummary {
    AgentSessionSummary {
        agent: session.agent.to_serialized_name(),
        display_name: session.agent.display_name().to_owned(),
        status: wire_status(&session.status),
        status_message: status_message(&session.status),
        title: session.session_context.title_like_text(),
        tool_name: session.session_context.tool_name.clone(),
        tool_input_preview: session.session_context.tool_input_preview.clone(),
        summary: session.session_context.summary.clone(),
        input_open: matches!(session.input_state, CLIAgentInputState::Open),
        brand_color: brand_color(session.agent),
    }
}

fn brand_color(agent: CLIAgent) -> String {
    match agent.brand_color() {
        Some(color) => format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b),
        None => DEFAULT_BRAND_COLOR.to_owned(),
    }
}

fn wire_status(status: &CLIAgentSessionStatus) -> AgentStatus {
    match status {
        CLIAgentSessionStatus::Idle => AgentStatus::Idle,
        CLIAgentSessionStatus::InProgress => AgentStatus::InProgress,
        CLIAgentSessionStatus::Success => AgentStatus::Success,
        CLIAgentSessionStatus::Failed { .. } => AgentStatus::Failed,
        CLIAgentSessionStatus::Blocked { .. } => AgentStatus::Blocked,
        CLIAgentSessionStatus::Cancelled => AgentStatus::Cancelled,
    }
}

fn status_message(status: &CLIAgentSessionStatus) -> Option<String> {
    match status {
        CLIAgentSessionStatus::Failed { message, .. } => message.clone(),
        CLIAgentSessionStatus::Blocked { message } => message.clone(),
        CLIAgentSessionStatus::Idle
        | CLIAgentSessionStatus::InProgress
        | CLIAgentSessionStatus::Success
        | CLIAgentSessionStatus::Cancelled => None,
    }
}

pub(crate) fn status_rank(status: &CLIAgentSessionStatus) -> u8 {
    match status {
        CLIAgentSessionStatus::Blocked { .. } | CLIAgentSessionStatus::Failed { .. } => 0,
        CLIAgentSessionStatus::InProgress => 1,
        CLIAgentSessionStatus::Idle
        | CLIAgentSessionStatus::Success
        | CLIAgentSessionStatus::Cancelled => 2,
    }
}

fn wire_agent_summary(summary: WorktreeAgentSummary) -> AgentSummary {
    match summary {
        WorktreeAgentSummary::None => AgentSummary::None,
        WorktreeAgentSummary::Working => AgentSummary::Working,
        WorktreeAgentSummary::NeedsAttention => AgentSummary::NeedsAttention,
    }
}

fn build_projects(
    window_ids: &[WindowId],
    workspaces_by_window: &HashMap<WindowId, Vec<ViewHandle<Workspace>>>,
    open_tabs_per_worktree: &HashMap<String, usize>,
    app: &AppContext,
) -> Vec<ProjectSnapshot> {
    let registry = ProjectRegistryModel::as_ref(app);
    registry
        .projects_mru()
        .into_iter()
        .map(|project| {
            let (open_in_window_id, screen_id) =
                locate_project(project.id, window_ids, workspaces_by_window, app);
            let (working, needs_attention) = project_counts(project.id, app);
            let worktrees = registry
                .worktrees_for_project(project.id)
                .into_iter()
                .map(|worktree| build_worktree(project, worktree, open_tabs_per_worktree))
                .collect();
            ProjectSnapshot {
                id: project.id.to_string(),
                name: project.display_name.clone(),
                root_path: project.root_path.to_string_lossy().into_owned(),
                kind: match project.kind {
                    ProjectKind::Git => ProjectKindWire::Git,
                    ProjectKind::Folder => ProjectKindWire::Folder,
                },
                primary_branch: project.primary_branch.clone(),
                last_opened_ts: project.last_opened_ts,
                open_in_window_id,
                screen_id,
                counts: AgentCounts {
                    working,
                    needs_attention,
                },
                worktrees,
            }
        })
        .collect()
}

fn locate_project(
    project_id: crate::projects::ProjectId,
    window_ids: &[WindowId],
    workspaces_by_window: &HashMap<WindowId, Vec<ViewHandle<Workspace>>>,
    app: &AppContext,
) -> (Option<String>, Option<String>) {
    for window_id in window_ids {
        let Some(workspaces) = workspaces_by_window.get(window_id) else {
            continue;
        };
        for workspace in workspaces {
            if workspace.as_ref(app).project_id() == Some(project_id) {
                return (
                    Some(window_id.to_string()),
                    Some(workspace.as_ref(app).screen_id().to_string()),
                );
            }
        }
    }
    (None, None)
}

fn build_worktree(
    project: &crate::projects::Project,
    worktree: &Worktree,
    open_tabs_per_worktree: &HashMap<String, usize>,
) -> WorktreeSnapshot {
    let id = worktree.id.to_string();
    let (kind, base_branch) = match &worktree.kind {
        WorktreeKind::Primary => (WorktreeKindWire::Primary, None),
        WorktreeKind::Linked { base_branch, .. } => {
            (WorktreeKindWire::Linked, Some(base_branch.clone()))
        }
    };
    WorktreeSnapshot {
        open_tab_count: open_tabs_per_worktree.get(&id).copied().unwrap_or(0),
        id,
        project_id: project.id.to_string(),
        name: worktree.name.clone(),
        kind,
        path: worktree.directory(project).to_string_lossy().into_owned(),
        branch: worktree
            .branch()
            .map(str::to_owned)
            .or_else(|| project.primary_branch.clone()),
        base_branch,
        created_ts: worktree.created_ts,
        agent_summary: AgentSummary::None,
    }
}

fn build_sessions(
    locations: &HashMap<EntityId, TerminalLocation>,
    app: &AppContext,
) -> Vec<AgentSessionSnapshot> {
    let mut sessions: Vec<AgentSessionSnapshot> = CLIAgentSessionsModel::as_ref(app)
        .sessions()
        .filter_map(|(terminal_view_id, session)| {
            let location = locations.get(&terminal_view_id)?;
            Some(AgentSessionSnapshot {
                terminal_id: terminal_view_id.to_string(),
                tab_id: location.tab_id.clone(),
                screen_id: location.screen_id.clone(),
                window_id: location.window_id.clone(),
                project_id: location.project_id.clone(),
                worktree_id: location.worktree_id.clone(),
                workspace_name: location.workspace_name.clone(),
                session: agent_summary(session),
                rank: status_rank(&session.status),
            })
        })
        .collect();
    sessions.sort_by(|left, right| {
        left.rank
            .cmp(&right.rank)
            .then_with(|| left.workspace_name.cmp(&right.workspace_name))
            .then_with(|| left.terminal_id.cmp(&right.terminal_id))
    });
    sessions
}

fn build_agent_catalog(path_env: Option<&str>) -> Vec<AgentCatalogEntry> {
    agent_catalog()
        .iter()
        .enumerate()
        .map(|(index, definition)| AgentCatalogEntry {
            index,
            id: definition.cli_agent.to_serialized_name(),
            display_name: definition.display_name.to_owned(),
            installed: is_installed(definition, path_env),
            supports_yolo: supports_yolo(definition),
            brand_color: brand_color(definition.cli_agent),
        })
        .collect()
}

fn supports_yolo(definition: &AgentDefinition) -> bool {
    definition.yolo_args.is_some()
}

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;
