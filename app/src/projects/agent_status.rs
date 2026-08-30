use warpui::{AppContext, SingletonEntity};

use super::ProjectId;
use crate::tab::TabData;
use crate::terminal::cli_agent_sessions::{CLIAgentSessionStatus, CLIAgentSessionsModel};
use crate::workspace::{Workspace, WorkspaceRegistry};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum WorktreeAgentSummary {
    #[default]
    None,
    Working,
    NeedsAttention,
}

impl WorktreeAgentSummary {
    pub fn merge(self, other: Self) -> Self {
        self.max(other)
    }

    pub fn needs_attention(self) -> bool {
        self == WorktreeAgentSummary::NeedsAttention
    }
}

fn summarize_status(status: &CLIAgentSessionStatus) -> WorktreeAgentSummary {
    match status {
        CLIAgentSessionStatus::InProgress => WorktreeAgentSummary::Working,
        CLIAgentSessionStatus::Blocked { .. } | CLIAgentSessionStatus::Failed { .. } => {
            WorktreeAgentSummary::NeedsAttention
        }
        CLIAgentSessionStatus::Idle
        | CLIAgentSessionStatus::Success
        | CLIAgentSessionStatus::Cancelled => WorktreeAgentSummary::None,
    }
}

pub fn summarize_tab(tab: &TabData, app: &AppContext) -> WorktreeAgentSummary {
    let sessions = CLIAgentSessionsModel::as_ref(app);
    tab.pane_group
        .as_ref(app)
        .terminal_views(app)
        .iter()
        .filter_map(|terminal_view| sessions.session(terminal_view.id()))
        .map(|session| summarize_status(&session.status))
        .fold(WorktreeAgentSummary::None, WorktreeAgentSummary::merge)
}

pub fn summarize_screen(workspace: &Workspace, app: &AppContext) -> WorktreeAgentSummary {
    workspace
        .tabs
        .iter()
        .map(|tab| summarize_tab(tab, app))
        .fold(WorktreeAgentSummary::None, WorktreeAgentSummary::merge)
}

pub fn summarize_project(project_id: ProjectId, app: &AppContext) -> WorktreeAgentSummary {
    WorkspaceRegistry::as_ref(app)
        .all_workspaces(app)
        .into_iter()
        .filter(|(_, workspace)| workspace.as_ref(app).project_id() == Some(project_id))
        .map(|(_, workspace)| summarize_screen(workspace.as_ref(app), app))
        .fold(WorktreeAgentSummary::None, WorktreeAgentSummary::merge)
}

pub fn project_counts(project_id: ProjectId, app: &AppContext) -> (usize, usize) {
    let mut working = 0;
    let mut needs_attention = 0;
    for (_, workspace) in WorkspaceRegistry::as_ref(app).all_workspaces(app) {
        let workspace = workspace.as_ref(app);
        if workspace.project_id() != Some(project_id) {
            continue;
        }
        for tab in &workspace.tabs {
            match summarize_tab(tab, app) {
                WorktreeAgentSummary::Working => working += 1,
                WorktreeAgentSummary::NeedsAttention => needs_attention += 1,
                WorktreeAgentSummary::None => {}
            }
        }
    }
    (working, needs_attention)
}

pub fn inactive_screens_need_attention(window_id: warpui::WindowId, app: &AppContext) -> bool {
    let registry = WorkspaceRegistry::as_ref(app);
    let active = registry.active_workspace_view_id(window_id);
    registry
        .workspaces_for_window(window_id, app)
        .into_iter()
        .filter(|workspace| Some(workspace.id()) != active)
        .any(|workspace| summarize_screen(workspace.as_ref(app), app).needs_attention())
}

#[cfg(test)]
#[path = "agent_status_tests.rs"]
mod tests;
