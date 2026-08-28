use std::path::PathBuf;
use std::sync::Arc;

use warpui::platform::FilePickerConfiguration;
use warpui::presenter::ChildView;
use warpui::{
    AppContext, Element, Entity, EntityId, FocusContext, SingletonEntity, TypedActionView, View,
    ViewContext, ViewHandle, WindowId, keymap,
};

use super::registry::{ProjectRegistryModel, now_ts};
use super::{Project, ProjectId, ProjectKind};
use crate::features::FeatureFlag;
use crate::global_resource_handles::GlobalResourceHandles;
use crate::pane_group::NewTerminalOptions;
use crate::persisted_workspace::PersistedWorkspace;
use crate::root_view::NewWorkspaceSource;
use crate::server::server_api::ServerTime;
use crate::workspace::{Workspace, WorkspaceRegistry};

pub const MULTIPLE_SCREENS_FLAG: &str = "ProjectHost_MultipleScreens";

pub fn init(app: &mut AppContext) {
    use warpui::keymap::EditableBinding;
    use warpui::keymap::macros::*;

    use crate::util::bindings::BindingGroup;

    fn ade_workspaces_enabled() -> bool {
        FeatureFlag::AdeWorkspaces.is_enabled()
    }

    app.register_editable_bindings([
        EditableBinding::new(
            "workspaces:next",
            "Next Workspace",
            ProjectHostAction::NextScreen,
        )
        .with_context_predicate(id!("ProjectHost"))
        .with_enabled(ade_workspaces_enabled)
        .with_group(BindingGroup::Workspaces.as_str())
        .with_key_binding("ctrl-cmd-]"),
        EditableBinding::new(
            "workspaces:previous",
            "Previous Workspace",
            ProjectHostAction::PreviousScreen,
        )
        .with_context_predicate(id!("ProjectHost"))
        .with_enabled(ade_workspaces_enabled)
        .with_group(BindingGroup::Workspaces.as_str())
        .with_key_binding("ctrl-cmd-["),
        EditableBinding::new(
            "workspaces:show_switcher",
            "Switch Workspace\u{2026}",
            ProjectHostAction::ShowSwitcherMenu,
        )
        .with_context_predicate(id!("ProjectHost"))
        .with_enabled(ade_workspaces_enabled)
        .with_group(BindingGroup::Workspaces.as_str())
        .with_key_binding("ctrl-cmd-p"),
        EditableBinding::new(
            "workspaces:open_folder",
            "Open Folder as Workspace\u{2026}",
            ProjectHostAction::OpenFolderAsWorkspace,
        )
        .with_context_predicate(id!("ProjectHost"))
        .with_enabled(ade_workspaces_enabled)
        .with_group(BindingGroup::Workspaces.as_str()),
        EditableBinding::new(
            "workspaces:activate_home",
            "Go to Home Workspace",
            ProjectHostAction::ActivateHome,
        )
        .with_context_predicate(id!("ProjectHost"))
        .with_enabled(ade_workspaces_enabled)
        .with_group(BindingGroup::Workspaces.as_str()),
    ]);
}

pub struct ProjectScreen {
    pub project_id: Option<ProjectId>,
    pub workspace: ViewHandle<Workspace>,
}

pub struct ProjectHost {
    window_id: WindowId,
    screens: Vec<ProjectScreen>,
    active_screen_index: usize,
    global_resource_handles: GlobalResourceHandles,
    server_time: Option<Arc<ServerTime>>,
}

#[derive(Debug, Clone)]
pub enum ProjectHostAction {
    OpenProject { project_id: ProjectId },
    CloseProjectScreen { project_id: ProjectId },
    ActivateScreenAt { index: usize },
    ActivateHome,
    NextScreen,
    PreviousScreen,
    ShowSwitcherMenu,
    OpenFolderAsWorkspace,
}

impl Entity for ProjectHost {
    type Event = ();
}

impl TypedActionView for ProjectHost {
    type Action = ProjectHostAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            ProjectHostAction::OpenProject { project_id } => self.open_project(*project_id, ctx),
            ProjectHostAction::CloseProjectScreen { project_id } => {
                self.close_project_screen(*project_id, ctx)
            }
            ProjectHostAction::ActivateScreenAt { index } => self.activate_screen(*index, ctx),
            ProjectHostAction::ActivateHome => self.activate_screen(0, ctx),
            ProjectHostAction::NextScreen => self.cycle_screen(1, ctx),
            ProjectHostAction::PreviousScreen => self.cycle_screen(-1, ctx),
            ProjectHostAction::ShowSwitcherMenu => {
                let workspace = self.active_workspace().clone();
                workspace.update(ctx, |workspace, ctx| {
                    workspace.show_workspace_switcher_dropdown(ctx);
                });
            }
            ProjectHostAction::OpenFolderAsWorkspace => self.open_folder_as_workspace(ctx),
        }
    }
}

impl View for ProjectHost {
    fn ui_name() -> &'static str {
        "ProjectHost"
    }

    fn render(&self, _app: &AppContext) -> Box<dyn Element> {
        ChildView::new(self.active_workspace()).finish()
    }

    fn child_view_ids(&self, _app: &AppContext) -> Vec<EntityId> {
        self.screens
            .iter()
            .map(|screen| screen.workspace.id())
            .collect()
    }

    fn keymap_context(&self, _app: &AppContext) -> keymap::Context {
        let mut context = Self::default_keymap_context();
        if self.screens.len() > 1 {
            context.set.insert(MULTIPLE_SCREENS_FLAG);
        }
        context
    }

    fn on_focus(&mut self, focus_ctx: &FocusContext, ctx: &mut ViewContext<Self>) {
        if focus_ctx.is_self_focused() {
            let workspace = self.active_workspace().clone();
            ctx.focus(&workspace);
        }
    }
}

impl ProjectHost {
    pub fn new(
        global_resource_handles: GlobalResourceHandles,
        server_time: Option<Arc<ServerTime>>,
        workspace_setting: NewWorkspaceSource,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        let window_id = ctx.window_id();
        let (screen_settings, active_screen_index) = Self::screen_settings(workspace_setting);

        let screens = screen_settings
            .into_iter()
            .map(|(project_id, setting)| {
                let workspace = ctx.add_typed_action_view(|ctx| {
                    Workspace::new(
                        global_resource_handles.clone(),
                        server_time.clone(),
                        setting,
                        project_id,
                        ctx,
                    )
                });
                ProjectScreen {
                    project_id,
                    workspace,
                }
            })
            .collect();

        let host = Self {
            window_id,
            screens,
            active_screen_index,
            global_resource_handles,
            server_time,
        };
        host.publish_active_screen(ctx);
        host
    }

    fn screen_settings(
        workspace_setting: NewWorkspaceSource,
    ) -> (Vec<(Option<ProjectId>, NewWorkspaceSource)>, usize) {
        let NewWorkspaceSource::Restored {
            window_snapshot,
            block_lists,
            ..
        } = &workspace_setting
        else {
            return (vec![(None, workspace_setting)], 0);
        };

        if window_snapshot.screens.len() <= 1 {
            let project_id = window_snapshot
                .screens
                .first()
                .and_then(|screen| screen.project_id);
            return (vec![(project_id, workspace_setting.clone())], 0);
        }

        let settings = window_snapshot
            .screens
            .iter()
            .enumerate()
            .map(|(index, screen)| {
                (
                    screen.project_id,
                    NewWorkspaceSource::Restored {
                        window_snapshot: window_snapshot.clone(),
                        screen_index: index,
                        block_lists: block_lists.clone(),
                    },
                )
            })
            .collect();
        let active = window_snapshot
            .active_screen_index
            .min(window_snapshot.screens.len() - 1);
        (settings, active)
    }

    pub fn active_workspace(&self) -> &ViewHandle<Workspace> {
        let index = self.active_screen_index.min(self.screens.len() - 1);
        &self.screens[index].workspace
    }

    pub fn workspaces(&self) -> impl Iterator<Item = &ViewHandle<Workspace>> {
        self.screens.iter().map(|screen| &screen.workspace)
    }

    pub fn active_screen_index(&self) -> usize {
        self.active_screen_index
    }

    pub fn open_project_ids(&self) -> Vec<ProjectId> {
        self.screens
            .iter()
            .filter_map(|screen| screen.project_id)
            .collect()
    }

    fn publish_active_screen(&self, ctx: &mut ViewContext<Self>) {
        let window_id = self.window_id;
        let screens: Vec<(Option<ProjectId>, EntityId)> = self
            .screens
            .iter()
            .map(|screen| (screen.project_id, screen.workspace.id()))
            .collect();
        let active_id = self.active_workspace().id();
        WorkspaceRegistry::handle(ctx).update(ctx, |registry, _| {
            registry.set_screens(window_id, screens, active_id);
        });
    }

    pub fn activate_screen(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        if index >= self.screens.len() || index == self.active_screen_index {
            return;
        }
        self.active_screen_index = index;
        self.publish_active_screen(ctx);
        self.focus_active_screen(ctx);
        ctx.notify();
        save_app_state(ctx);
    }

    fn focus_active_screen(&self, ctx: &mut ViewContext<Self>) {
        let workspace = self.active_workspace().clone();
        ctx.focus(&workspace);
        workspace.update(ctx, |workspace, ctx| workspace.focus_active_tab(ctx));
    }

    fn cycle_screen(&mut self, delta: isize, ctx: &mut ViewContext<Self>) {
        let count = self.screens.len() as isize;
        if count <= 1 {
            return;
        }
        let next = (self.active_screen_index as isize + delta).rem_euclid(count) as usize;
        self.activate_screen(next, ctx);
    }

    fn screen_index_for_project(&self, project_id: ProjectId) -> Option<usize> {
        self.screens
            .iter()
            .position(|screen| screen.project_id == Some(project_id))
    }

    pub fn open_project(&mut self, project_id: ProjectId, ctx: &mut ViewContext<Self>) {
        if let Some(index) = self.screen_index_for_project(project_id) {
            self.activate_screen(index, ctx);
            return;
        }

        let owning_window = WorkspaceRegistry::as_ref(ctx).window_owning_project(project_id);
        if let Some(owning_window) = owning_window
            && owning_window != self.window_id
        {
            ctx.windows().show_window_and_focus_app(owning_window);
            return;
        }

        let root_path = ProjectRegistryModel::handle(ctx).read(ctx, |registry, _| {
            registry
                .project(project_id)
                .map(|project| project.root_path.clone())
        });
        let Some(root_path) = root_path else {
            log::warn!("Cannot open unknown project {project_id}");
            return;
        };

        let workspace = ctx.add_typed_action_view(|ctx| {
            Workspace::new(
                self.global_resource_handles.clone(),
                self.server_time.clone(),
                NewWorkspaceSource::Session {
                    options: Box::new(NewTerminalOptions {
                        initial_directory: Some(root_path),
                        hide_homepage: true,
                        ..Default::default()
                    }),
                },
                Some(project_id),
                ctx,
            )
        });

        self.screens.push(ProjectScreen {
            project_id: Some(project_id),
            workspace,
        });
        self.active_screen_index = self.screens.len() - 1;
        self.publish_active_screen(ctx);

        ProjectRegistryModel::handle(ctx).update(ctx, |registry, ctx| {
            registry.touch_opened(project_id, now_ts(), ctx);
        });

        self.focus_active_screen(ctx);
        ctx.notify();
        save_app_state(ctx);
    }

    pub fn close_project_screen(&mut self, project_id: ProjectId, ctx: &mut ViewContext<Self>) {
        let Some(index) = self.screen_index_for_project(project_id) else {
            return;
        };

        if self.active_screen_index >= index {
            self.active_screen_index = self.active_screen_index.saturating_sub(1);
        }

        let screen = self.screens.remove(index);
        let workspace_id = screen.workspace.id();
        screen.workspace.update(ctx, |workspace, ctx| {
            workspace.close_all_tabs_for_screen_teardown(ctx);
        });
        crate::workspace::purge_screen_scoped_state(workspace_id, ctx);

        self.publish_active_screen(ctx);
        self.focus_active_screen(ctx);
        ctx.notify();
        save_app_state(ctx);
    }

    fn open_folder_as_workspace(&mut self, ctx: &mut ViewContext<Self>) {
        if !FeatureFlag::AdeWorkspaces.is_enabled() {
            return;
        }
        ctx.open_file_picker(
            |result, ctx| {
                let Ok(paths) = result else {
                    return;
                };
                let Some(path) = paths.into_iter().next() else {
                    return;
                };
                if let Some(handle) = ctx.handle().upgrade(ctx) {
                    handle.update(ctx, |host, ctx| {
                        host.register_and_open_folder(PathBuf::from(path), ctx);
                    });
                }
            },
            FilePickerConfiguration::new().folders_only(),
        );
    }

    pub fn register_and_open_folder(&mut self, path: PathBuf, ctx: &mut ViewContext<Self>) {
        let _ = ctx.spawn(
            async move {
                match super::git_ops::discover_repo_root(&path).await {
                    Ok(Some(repo)) => {
                        let branch = super::git_ops::detect_primary_branch(&repo.root).await.ok();
                        (repo.root, ProjectKind::Git, branch)
                    }
                    _ => (path, ProjectKind::Folder, None),
                }
            },
            Self::finish_registering_folder,
        );
    }

    fn finish_registering_folder(
        &mut self,
        registration: (PathBuf, ProjectKind, Option<String>),
        ctx: &mut ViewContext<Self>,
    ) {
        let (root_path, kind, primary_branch) = registration;
        let display_name = Project::display_name_for_root(&root_path);
        let project_id = ProjectRegistryModel::handle(ctx).update(ctx, |registry, ctx| {
            registry.register_project(root_path.clone(), display_name, kind, primary_branch, ctx)
        });
        PersistedWorkspace::handle(ctx).update(ctx, |persisted, ctx| {
            persisted.user_added_workspace(root_path, ctx);
        });
        self.open_project(project_id, ctx);
    }
}

fn save_app_state(ctx: &mut ViewContext<ProjectHost>) {
    ctx.dispatch_global_action("workspace:save_app", ());
}
