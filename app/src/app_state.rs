use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use pathfinder_geometry::rect::RectF;
use serde::{Deserialize, Serialize};
use warpui::platform::FullscreenState;
use warpui::{AppContext, SingletonEntity as _};

use crate::code::editor_management::CodeSource;
use crate::projects::{ProjectId, WorktreeId};
use crate::root_view::quake_mode_window_id;
use crate::server::ids::{ServerId, SyncId};
use crate::settings_view::SettingsSection;
use crate::tab::SelectedTabColor;
use crate::terminal::ShellLaunchData;
use crate::terminal::model::block::SerializedBlock;
use crate::themes::theme::AnsiColorIdentifier;
use crate::workspace::WorkspaceRegistry;
use crate::workspace::tab_group::TabGroupId;
use crate::workspace::view::left_panel::ToolPanelView;

#[derive(Debug, Clone, PartialEq)]
pub struct AppState {
    pub windows: Vec<WindowSnapshot>,
    pub active_window_index: Option<usize>,
    pub block_lists: Arc<HashMap<PaneUuid, Vec<SerializedBlock>>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PaneUuid(pub Vec<u8>);

#[derive(Clone, Debug, PartialEq)]
pub struct WindowSnapshot {
    pub screens: Vec<ProjectScreenSnapshot>,
    pub active_screen_index: usize,
    pub team_uid: Option<ServerId>,
    pub bounds: Option<RectF>,
    pub fullscreen_state: FullscreenState,
    pub quake_mode: bool,
    pub universal_search_width: Option<f32>,
    pub voltron_width: Option<f32>,
    pub warp_drive_index_width: Option<f32>,
    pub left_panel_open: bool,
    pub vertical_tabs_panel_open: bool,
    pub left_panel_width: Option<f32>,
    pub right_panel_width: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ProjectScreenSnapshot {
    pub project_id: Option<ProjectId>,
    pub tabs: Vec<TabSnapshot>,
    pub active_tab_index: usize,
    /// Tab groups defined in this screen. Group order is implicit from
    /// member tabs' positions, so no explicit ordering is persisted.
    pub tab_groups: Vec<TabGroupSnapshot>,
}

impl WindowSnapshot {
    pub fn screen(&self, index: usize) -> Option<&ProjectScreenSnapshot> {
        self.screens.get(index)
    }

    pub fn has_tabs(&self) -> bool {
        self.screens.iter().any(|screen| !screen.tabs.is_empty())
    }

    pub fn active_screen(&self) -> Option<&ProjectScreenSnapshot> {
        self.screens.get(self.active_screen_index)
    }

    pub fn tabs(&self) -> &[TabSnapshot] {
        self.active_screen()
            .map(|screen| screen.tabs.as_slice())
            .unwrap_or_default()
    }

    pub fn active_tab_index(&self) -> usize {
        self.active_screen()
            .map(|screen| screen.active_tab_index)
            .unwrap_or(0)
    }

    pub fn tab_groups(&self) -> &[TabGroupSnapshot] {
        self.active_screen()
            .map(|screen| screen.tab_groups.as_slice())
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TabGroupSnapshot {
    pub id: TabGroupId,
    pub name: Option<String>,
    pub color: SelectedTabColor,
    pub collapsed: bool,
    pub pinned: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TabSnapshot {
    pub custom_title: Option<String>,
    pub root: PaneNodeSnapshot,
    pub default_directory_color: Option<AnsiColorIdentifier>,
    pub selected_color: SelectedTabColor,
    pub left_panel: Option<LeftPanelSnapshot>,
    pub right_panel: Option<RightPanelSnapshot>,
    /// Tab group this tab belongs to, if any.
    pub group_id: Option<TabGroupId>,
    /// True when this tab is pinned to the front of the tab list.
    pub pinned: bool,
    pub worktree_id: Option<WorktreeId>,
}

impl TabSnapshot {
    pub(crate) fn color(&self) -> Option<AnsiColorIdentifier> {
        self.selected_color.resolve(self.default_directory_color)
    }
}

#[derive(Clone, Debug, PartialEq)]
#[allow(
    clippy::large_enum_variant,
    reason = "LeafSnapshot is significantly larger than BranchSnapshot due to nested snapshot types."
)]
pub enum PaneNodeSnapshot {
    Branch(BranchSnapshot),
    Leaf(LeafSnapshot),
}

impl PaneNodeSnapshot {
    pub fn has_horizontal_split(&self) -> bool {
        match self {
            PaneNodeSnapshot::Leaf(_) => false,
            PaneNodeSnapshot::Branch(BranchSnapshot {
                direction,
                children,
            }) => {
                let self_has_split = *direction == SplitDirection::Horizontal && children.len() > 1;
                self_has_split
                    || children
                        .iter()
                        .any(|(_, child)| child.has_horizontal_split())
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BranchSnapshot {
    pub direction: SplitDirection,
    pub children: Vec<(PaneFlex, PaneNodeSnapshot)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LeafSnapshot {
    pub is_focused: bool,
    pub custom_vertical_tabs_title: Option<String>,
    pub contents: LeafContents,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LeafContents {
    Terminal(TerminalPaneSnapshot),
    Notebook(NotebookPaneSnapshot),
    Code(CodePaneSnapShot),
    Settings(SettingsPaneSnapshot),
    CodeReview(CodeReviewPaneSnapshot),
    /// The in-app network log pane. Not persisted across restarts because the
    /// backing log is an in-memory ring buffer that starts empty on launch.
    NetworkLog,
    /// A new first-time user experience which prioritizes choosing a coding repository.
    GetStarted,
    AgentPicker,
}

#[cfg(feature = "local_fs")]
impl LeafContents {
    /// Whether this pane content should be written to (and later restored
    /// from) the SQLite app-state database.
    ///
    /// Non-persisted pane types are skipped entirely during the pane tree
    /// traversal in `save_app_state`, so no `pane_nodes` row is inserted for
    /// them. This is important: inserting a `pane_nodes` row with
    /// `is_leaf = true` but no matching `pane_leaves` row leaves an orphan
    /// that `read_node` cannot resolve, which causes the surrounding tab's
    /// restoration to fail and the whole tab to disappear on restart.
    pub(crate) fn is_persisted(&self) -> bool {
        match self {
            // Network log: the backing log is an in-memory ring buffer that
            // starts empty on launch; persisting would also regress back to
            // an on-disk log via the app-state database.
            LeafContents::NetworkLog
            // Environment management panes are opened on-demand via workspace
            // actions and have no persistable state.
            | LeafContents::AgentPicker => false,
            LeafContents::Terminal(_)
            | LeafContents::Notebook(_)
            | LeafContents::Code(_)
            | LeafContents::Settings(_)
            | LeafContents::CodeReview(_)
            | LeafContents::GetStarted => true,
        }
    }
}

/// Snapshot of the contents of a terminal pane.
#[derive(Clone, Debug, PartialEq)]
pub struct TerminalPaneSnapshot {
    pub uuid: Vec<u8>,
    pub cwd: Option<String>,
    pub shell_launch_data: Option<ShellLaunchData>,
    pub is_active: bool,
    pub is_read_only: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NotebookPaneSnapshot {
    LocalFileNotebook {
        /// The path to the local file that was open in this pane. This may be `None` if
        /// the pane contained an unreadable file.
        path: Option<PathBuf>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct CodePaneTabSnapshot {
    pub path: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CodePaneSnapShot {
    Local {
        tabs: Vec<CodePaneTabSnapshot>,
        active_tab_index: usize,
        /// The full `CodeSource` for this pane, serialized as JSON in the DB.
        source: Option<CodeSource>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingsPaneSnapshot {
    Local {
        current_page: SettingsSection,
        search_query: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum CodeReviewPaneSnapshot {
    Local {
        terminal_uuid: Vec<u8>,
        repo_path: PathBuf,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LeftPanelDisplayedTab {
    FileTree,
    GlobalSearch,
    SourceControl,
}

impl From<ToolPanelView> for LeftPanelDisplayedTab {
    fn from(view: ToolPanelView) -> Self {
        match view {
            ToolPanelView::ProjectExplorer => LeftPanelDisplayedTab::FileTree,
            ToolPanelView::GlobalSearch { .. } => LeftPanelDisplayedTab::GlobalSearch,
            ToolPanelView::SourceControl => LeftPanelDisplayedTab::SourceControl,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LeftPanelSnapshot {
    pub left_panel_displayed_tab: LeftPanelDisplayedTab,
    pub pane_group_id: String,
    pub width: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RightPanelSnapshot {
    pub pane_group_id: String,
    pub width: usize,
    pub is_maximized: bool,
}

/// Copied from pane group model, which should be private to pane group.
#[derive(Clone, Debug, PartialEq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaneFlex(pub f32);

pub fn get_app_state(app: &AppContext) -> AppState {
    let active_window_id = app.windows().active_window();
    let quake_mode_id = quake_mode_window_id();

    let mut active_window_index = None;

    let mut windows = vec![];

    for (index, window_id) in app.window_ids().enumerate() {
        // Determine index of active window
        if let Some(active_window_id) = active_window_id
            && active_window_id == window_id
        {
            active_window_index = Some(index);
        }

        let screens = WorkspaceRegistry::as_ref(app).workspaces_for_window(window_id, app);
        let Some(active) = WorkspaceRegistry::as_ref(app).get(window_id, app) else {
            continue;
        };
        let active_workspace = active.as_ref(app);
        // Transient drag-preview windows are not real user-visible
        // workspaces; skip them so they never end up in the persisted
        // session. (Persistence is also short-circuited entirely while a
        // cross-window drag is active; see `save_app` in
        // `workspace/global_actions.rs`.)
        if active_workspace.is_tab_drag_preview() {
            continue;
        }

        let mut snapshot = active_workspace.snapshot(
            window_id,
            quake_mode_id.map(|id| id == window_id).unwrap_or(false),
            app,
        );
        snapshot.screens = screens
            .iter()
            .map(|screen| screen.as_ref(app).screen_snapshot(window_id, app))
            .collect();
        snapshot.active_screen_index = screens
            .iter()
            .position(|screen| screen.id() == active.id())
            .unwrap_or(0);

        if snapshot.has_tabs() {
            windows.push(snapshot);
        }
    }

    AppState {
        windows,
        active_window_index,
        block_lists: Default::default(),
    }
}

#[cfg(test)]
#[path = "app_state_tests.rs"]
mod tests;
