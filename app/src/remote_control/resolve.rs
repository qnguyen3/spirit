use remote_control::protocol::{CommandError, ErrorCode};
use warpui::{AppContext, EntityId, SingletonEntity as _, ViewHandle, WindowId};

use crate::pane_group::PaneGroup;
use crate::pane_group::pane::PaneId;
use crate::projects::host::ProjectHost;
use crate::projects::registry::ProjectRegistryModel;
use crate::projects::{ProjectId, WorktreeId};
use crate::root_view::RootView;
use crate::terminal::view::TerminalView;
use crate::workspace::WorkspaceRegistry;
use crate::workspace::view::Workspace;

pub(crate) struct ScreenTarget {
    pub window_id: WindowId,
    pub workspace: ViewHandle<Workspace>,
}

pub(crate) struct TerminalTarget {
    pub window_id: WindowId,
    pub workspace: ViewHandle<Workspace>,
    pub pane_group: ViewHandle<PaneGroup>,
    pub pane_id: PaneId,
    pub terminal: ViewHandle<TerminalView>,
}

pub(crate) fn parse_entity_id(raw: &str, what: &str) -> Result<EntityId, CommandError> {
    raw.parse::<usize>()
        .map(EntityId::from_usize)
        .map_err(|_| CommandError::new(ErrorCode::InvalidRequest, format!("{what} is malformed")))
}

pub(crate) fn parse_project_id(raw: &str) -> Result<ProjectId, CommandError> {
    raw.parse::<ProjectId>()
        .map_err(|_| CommandError::new(ErrorCode::InvalidRequest, "project_id is malformed"))
}

pub(crate) fn parse_worktree_id(raw: &str) -> Result<WorktreeId, CommandError> {
    raw.parse::<WorktreeId>()
        .map_err(|_| CommandError::new(ErrorCode::InvalidRequest, "worktree_id is malformed"))
}

pub(crate) fn screen(screen_id: &str, app: &AppContext) -> Result<ScreenTarget, CommandError> {
    let wanted = parse_entity_id(screen_id, "screen_id")?;
    WorkspaceRegistry::as_ref(app)
        .all_workspaces(app)
        .into_iter()
        .find(|(_, workspace)| workspace.id() == wanted)
        .map(|(window_id, workspace)| ScreenTarget {
            window_id,
            workspace,
        })
        .ok_or_else(|| CommandError::not_found("that screen"))
}

pub(crate) fn screen_for_project(project_id: ProjectId, app: &AppContext) -> Option<ScreenTarget> {
    WorkspaceRegistry::as_ref(app)
        .all_workspaces(app)
        .into_iter()
        .find(|(_, workspace)| workspace.as_ref(app).project_id() == Some(project_id))
        .map(|(window_id, workspace)| ScreenTarget {
            window_id,
            workspace,
        })
}

pub(crate) fn tab(tab_id: &str, app: &AppContext) -> Result<(ScreenTarget, usize), CommandError> {
    let wanted = parse_entity_id(tab_id, "tab_id")?;
    for (window_id, workspace) in WorkspaceRegistry::as_ref(app).all_workspaces(app) {
        let index = workspace
            .as_ref(app)
            .tabs
            .iter()
            .position(|tab| tab.pane_group.id() == wanted);
        if let Some(index) = index {
            return Ok((
                ScreenTarget {
                    window_id,
                    workspace,
                },
                index,
            ));
        }
    }
    Err(CommandError::not_found("that tab"))
}

pub(crate) fn pane(pane_id: &str, app: &AppContext) -> Result<TerminalTargetOrPane, CommandError> {
    for (_, workspace) in WorkspaceRegistry::as_ref(app).all_workspaces(app) {
        for tab in &workspace.as_ref(app).tabs {
            let pane_group = tab.pane_group.clone();
            let found = pane_group
                .as_ref(app)
                .visible_pane_ids()
                .into_iter()
                .find(|candidate| candidate.to_string() == pane_id);
            if let Some(found) = found {
                return Ok(TerminalTargetOrPane {
                    workspace,
                    pane_group,
                    pane_id: found,
                });
            }
        }
    }
    Err(CommandError::not_found("that pane"))
}

pub(crate) struct TerminalTargetOrPane {
    pub workspace: ViewHandle<Workspace>,
    pub pane_group: ViewHandle<PaneGroup>,
    pub pane_id: PaneId,
}

pub(crate) fn terminal(
    terminal_id: &str,
    app: &AppContext,
) -> Result<TerminalTarget, CommandError> {
    let wanted = parse_entity_id(terminal_id, "terminal_id")?;
    for (window_id, workspace) in WorkspaceRegistry::as_ref(app).all_workspaces(app) {
        for tab in &workspace.as_ref(app).tabs {
            let pane_group = tab.pane_group.clone();
            let group = pane_group.as_ref(app);
            for pane_id in group.visible_pane_ids() {
                let Some(view) = group.terminal_view_from_pane_id(pane_id, app) else {
                    continue;
                };
                if view.id() == wanted {
                    return Ok(TerminalTarget {
                        window_id,
                        workspace: workspace.clone(),
                        pane_group: pane_group.clone(),
                        pane_id,
                        terminal: view,
                    });
                }
            }
        }
    }
    Err(CommandError::not_found("that terminal"))
}

pub(crate) fn worktree_project(
    worktree_id: WorktreeId,
    app: &AppContext,
) -> Result<ProjectId, CommandError> {
    ProjectRegistryModel::as_ref(app)
        .worktree(worktree_id)
        .map(|worktree| worktree.project_id)
        .ok_or_else(|| CommandError::not_found("that worktree"))
}

pub(crate) fn project_host(
    window_id: WindowId,
    ctx: &mut AppContext,
) -> Result<ViewHandle<ProjectHost>, CommandError> {
    ctx.root_view::<RootView>(window_id)
        .and_then(|root| root.as_ref(ctx).project_host_view().cloned())
        .ok_or_else(|| CommandError::not_found("that window"))
}

pub(crate) fn default_window(app: &AppContext) -> Result<WindowId, CommandError> {
    if let Some(active) = app.windows().active_window() {
        return Ok(active);
    }
    let mut window_ids: Vec<WindowId> = WorkspaceRegistry::as_ref(app)
        .all_workspaces(app)
        .into_iter()
        .map(|(window_id, _)| window_id)
        .collect();
    window_ids.sort();
    window_ids.dedup();
    window_ids
        .into_iter()
        .next()
        .ok_or_else(|| CommandError::new(ErrorCode::NotFound, "Spirit has no open window"))
}

pub(crate) fn screen_index(
    window_id: WindowId,
    screen_id: EntityId,
    app: &AppContext,
) -> Result<usize, CommandError> {
    WorkspaceRegistry::as_ref(app)
        .screen_ids_for_window(window_id)
        .into_iter()
        .position(|candidate| candidate == screen_id)
        .ok_or_else(|| CommandError::not_found("that screen"))
}
