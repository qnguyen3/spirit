use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use settings::Setting;
use warpui::elements::{ParentElement, Stack};
use warpui::platform::FilePickerConfiguration;
use warpui::presenter::ChildView;
use warpui::ui_components::components::UiComponentStyles;
use warpui::{
    AppContext, Element, Entity, EntityId, FocusContext, SingletonEntity, TypedActionView, View,
    ViewContext, ViewHandle, WindowId, keymap,
};

use super::git_ops::CloneProgress;
use super::new_workspace_modal::{NewWorkspaceModal, NewWorkspaceModalEvent, NewWorkspaceMode};
use super::overview::{OverviewEvent, WorkspaceOverviewView};
use super::registry::{ProjectRegistryModel, now_ts};
use super::remove_workspace_dialog::{RemoveWorkspaceDialog, RemoveWorkspaceEvent};
use super::settings::WorkspaceCreationSettings;
use super::{Project, ProjectId, ProjectKind};
use crate::features::FeatureFlag;
use crate::global_resource_handles::GlobalResourceHandles;
use crate::modal::{Modal, ModalEvent, ModalViewState};
use crate::pane_group::NewTerminalOptions;
use crate::persisted_workspace::PersistedWorkspace;
use crate::root_view::NewWorkspaceSource;
use crate::server::server_api::ServerTime;
use crate::view_components::DismissibleToast;
use crate::workspace::{ToastStack, Workspace, WorkspaceRegistry};

pub const MULTIPLE_SCREENS_FLAG: &str = "ProjectHost_MultipleScreens";

pub fn init(app: &mut AppContext) {
    use warpui::keymap::EditableBinding;
    use warpui::keymap::macros::*;

    use crate::util::bindings::BindingGroup;

    super::create_worktree_modal::init(app);
    super::delete_worktree_dialog::init(app);
    super::new_workspace_modal::init(app);
    super::overview::init(app);
    super::remove_workspace_dialog::init(app);

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
            "workspaces:new",
            "New Workspace\u{2026}",
            ProjectHostAction::ShowNewWorkspaceModal { mode: None },
        )
        .with_context_predicate(id!("ProjectHost"))
        .with_enabled(ade_workspaces_enabled)
        .with_group(BindingGroup::Workspaces.as_str()),
        EditableBinding::new(
            "workspaces:overview",
            "Workspace Overview",
            ProjectHostAction::ShowOverview,
        )
        .with_context_predicate(id!("ProjectHost"))
        .with_enabled(ade_workspaces_enabled)
        .with_group(BindingGroup::Workspaces.as_str())
        .with_key_binding("ctrl-cmd-o"),
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
    new_workspace_modal: ModalViewState<Modal<NewWorkspaceModal>>,
    overview: ViewHandle<WorkspaceOverviewView>,
    overview_active: bool,
    remove_dialog: ModalViewState<RemoveWorkspaceDialog>,
    clone_cancelled: Arc<AtomicBool>,
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
    ShowNewWorkspaceModal { mode: Option<NewWorkspaceMode> },
    ShowOverview,
    HideOverview,
    RemoveProject { project_id: ProjectId },
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
            ProjectHostAction::OpenFolderAsWorkspace => {
                self.show_new_workspace_modal(Some(NewWorkspaceMode::Open), ctx)
            }
            ProjectHostAction::ShowNewWorkspaceModal { mode } => {
                self.show_new_workspace_modal(*mode, ctx)
            }
            ProjectHostAction::ShowOverview => self.set_overview_active(true, ctx),
            ProjectHostAction::HideOverview => self.set_overview_active(false, ctx),
            ProjectHostAction::RemoveProject { project_id } => {
                self.show_remove_dialog(*project_id, ctx)
            }
        }
    }
}

impl View for ProjectHost {
    fn ui_name() -> &'static str {
        "ProjectHost"
    }

    fn render(&self, _app: &AppContext) -> Box<dyn Element> {
        let mut stack = Stack::new();
        if self.overview_active {
            ParentElement::add_child(&mut stack, ChildView::new(&self.overview).finish());
        } else {
            ParentElement::add_child(&mut stack, ChildView::new(self.active_workspace()).finish());
        }
        if self.new_workspace_modal.is_open() {
            ParentElement::add_child(&mut stack, self.new_workspace_modal.render());
        }
        if self.remove_dialog.is_open() {
            ParentElement::add_child(&mut stack, self.remove_dialog.render());
        }
        stack.finish()
    }

    fn child_view_ids(&self, _app: &AppContext) -> Vec<EntityId> {
        let mut ids: Vec<EntityId> = self
            .screens
            .iter()
            .map(|screen| screen.workspace.id())
            .collect();
        ids.push(self.overview.id());
        ids.push(self.new_workspace_modal.view.id());
        ids.push(self.remove_dialog.view.id());
        ids
    }

    fn keymap_context(&self, _app: &AppContext) -> keymap::Context {
        let mut context = Self::default_keymap_context();
        if self.screens.len() > 1 {
            context.set.insert(MULTIPLE_SCREENS_FLAG);
        }
        context
    }

    fn on_focus(&mut self, focus_ctx: &FocusContext, ctx: &mut ViewContext<Self>) {
        if !focus_ctx.is_self_focused() {
            return;
        }
        if self.overview_active {
            let overview = self.overview.clone();
            ctx.focus(&overview);
        } else {
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

        let new_workspace_modal = Self::build_new_workspace_modal(ctx);
        let overview = Self::build_overview(ctx);
        let remove_dialog = Self::build_remove_dialog(ctx);

        let host = Self {
            window_id,
            screens,
            active_screen_index,
            global_resource_handles,
            server_time,
            new_workspace_modal,
            overview,
            overview_active: false,
            remove_dialog,
            clone_cancelled: Arc::new(AtomicBool::new(false)),
        };
        host.publish_active_screen(ctx);
        host.seed_restored_project_screens(ctx);
        host
    }

    fn seed_restored_project_screens(&self, ctx: &mut ViewContext<Self>) {
        for screen in &self.screens {
            let Some(project_id) = screen.project_id else {
                continue;
            };
            let seed = ProjectRegistryModel::handle(ctx).read(ctx, |registry, _| {
                registry.project(project_id).map(|project| {
                    (
                        project.root_path.clone(),
                        registry.primary_worktree_id(project_id),
                    )
                })
            });
            let Some((root_path, primary_worktree_id)) = seed else {
                continue;
            };
            let workspace = screen.workspace.clone();
            workspace.update(ctx, |workspace, ctx| {
                if workspace.tab_count() == 0 {
                    workspace.add_tab_with_pane_layout(
                        crate::pane_group::PanesLayout::SingleTerminal(Box::new(
                            NewTerminalOptions {
                                initial_directory: Some(root_path),
                                hide_homepage: true,
                                ..Default::default()
                            },
                        )),
                        Arc::new(std::collections::HashMap::new()),
                        None,
                        ctx,
                    );
                    workspace.bind_active_tab_to_worktree(primary_worktree_id, ctx);
                }
                workspace.reconcile_worktrees(ctx);
            });
        }
    }

    fn build_new_workspace_modal(
        ctx: &mut ViewContext<Self>,
    ) -> ModalViewState<Modal<NewWorkspaceModal>> {
        let body = ctx.add_typed_action_view(NewWorkspaceModal::new);
        ctx.subscribe_to_view(&body, |me, _, event, ctx| {
            me.handle_new_workspace_modal_event(event, ctx);
        });
        let modal = ctx.add_typed_action_view(|ctx| {
            Modal::new(None, body, ctx).with_modal_style(UiComponentStyles {
                width: Some(480.),
                ..Default::default()
            })
        });
        ctx.subscribe_to_view(&modal, |me, _, event, ctx| {
            if matches!(event, ModalEvent::Close) {
                me.close_new_workspace_modal(ctx);
            }
        });
        ModalViewState::new(modal)
    }

    fn build_overview(ctx: &mut ViewContext<Self>) -> ViewHandle<WorkspaceOverviewView> {
        let overview = ctx.add_typed_action_view(WorkspaceOverviewView::new);
        ctx.subscribe_to_view(&overview, |me, _, event, ctx| {
            me.handle_overview_event(event, ctx);
        });
        overview
    }

    fn build_remove_dialog(ctx: &mut ViewContext<Self>) -> ModalViewState<RemoveWorkspaceDialog> {
        let dialog = ctx.add_typed_action_view(RemoveWorkspaceDialog::new);
        ctx.subscribe_to_view(&dialog, |me, _, event, ctx| match event {
            RemoveWorkspaceEvent::Confirm { project_id } => {
                let project_id = *project_id;
                me.remove_dialog.close();
                me.remove_project(project_id, ctx);
            }
            RemoveWorkspaceEvent::Cancel => {
                me.remove_dialog.close();
                ctx.notify();
            }
        });
        ModalViewState::new(dialog)
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

        let opened = ProjectRegistryModel::handle(ctx).read(ctx, |registry, _| {
            let project = registry.project(project_id)?;
            Some((
                project.root_path.clone(),
                registry.primary_worktree_id(project_id),
            ))
        });
        let Some((root_path, primary_worktree_id)) = opened else {
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
        workspace.update(ctx, |workspace, ctx| {
            workspace.bind_active_tab_to_worktree(primary_worktree_id, ctx);
            workspace.reconcile_worktrees(ctx);
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

impl ProjectHost {
    fn set_overview_active(&mut self, active: bool, ctx: &mut ViewContext<Self>) {
        if !FeatureFlag::AdeWorkspaces.is_enabled() && active {
            return;
        }
        if self.overview_active == active {
            return;
        }
        self.overview_active = active;
        if active {
            let open = self.open_project_ids();
            let overview = self.overview.clone();
            overview.update(ctx, |overview, ctx| {
                overview.refresh(ctx);
                overview.set_open_projects(open, ctx);
            });
            ctx.focus(&overview);
        } else {
            self.focus_active_screen(ctx);
        }
        ctx.notify();
    }

    fn show_new_workspace_modal(
        &mut self,
        mode: Option<NewWorkspaceMode>,
        ctx: &mut ViewContext<Self>,
    ) {
        if !FeatureFlag::AdeWorkspaces.is_enabled() {
            return;
        }
        let body = self.modal_body(ctx);
        body.update(ctx, |body, ctx| body.on_open(mode, ctx));
        self.new_workspace_modal.open();
        ctx.focus(&self.new_workspace_modal.view);
        ctx.notify();
    }

    fn close_new_workspace_modal(&mut self, ctx: &mut ViewContext<Self>) {
        self.clone_cancelled.store(true, Ordering::Relaxed);
        self.new_workspace_modal.close();
        if self.overview_active {
            let overview = self.overview.clone();
            ctx.focus(&overview);
        } else {
            self.focus_active_screen(ctx);
        }
        ctx.notify();
    }

    fn modal_body(&self, ctx: &AppContext) -> ViewHandle<NewWorkspaceModal> {
        self.new_workspace_modal.view.as_ref(ctx).body().clone()
    }

    fn handle_new_workspace_modal_event(
        &mut self,
        event: &NewWorkspaceModalEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            NewWorkspaceModalEvent::Close => self.close_new_workspace_modal(ctx),
            NewWorkspaceModalEvent::CancelClone => {
                self.clone_cancelled.store(true, Ordering::Relaxed);
            }
            NewWorkspaceModalEvent::BrowseFolder(mode) => self.browse_folder_for_modal(*mode, ctx),
            NewWorkspaceModalEvent::OpenFolder { path } => {
                let path = path.clone();
                self.close_new_workspace_modal(ctx);
                self.register_and_open_folder(path, ctx);
            }
            NewWorkspaceModalEvent::CloneRepo {
                url,
                parent,
                directory_name,
            } => self.start_clone(url.clone(), parent.clone(), directory_name.clone(), ctx),
            NewWorkspaceModalEvent::CreateProject { name, parent } => {
                self.start_create(name.clone(), parent.clone(), ctx)
            }
        }
    }

    fn browse_folder_for_modal(&mut self, _mode: NewWorkspaceMode, ctx: &mut ViewContext<Self>) {
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
                        let body = host.modal_body(ctx);
                        body.update(ctx, |body, ctx| {
                            body.set_selected_folder(PathBuf::from(path), ctx);
                        });
                    });
                }
            },
            FilePickerConfiguration::new().folders_only(),
        );
    }

    fn start_clone(
        &mut self,
        url: String,
        parent: PathBuf,
        directory_name: String,
        ctx: &mut ViewContext<Self>,
    ) {
        WorkspaceCreationSettings::handle(ctx).update(ctx, |settings, ctx| {
            let _ = Setting::set_value(
                &mut settings.last_clone_parent,
                parent.to_string_lossy().to_string(),
                ctx,
            );
        });

        self.clone_cancelled = Arc::new(AtomicBool::new(false));
        let cancelled = self.clone_cancelled.clone();
        let progress_cancelled = cancelled.clone();
        let body = self.modal_body(ctx);
        let (progress_sender, progress_receiver) = std::sync::mpsc::channel::<CloneProgress>();

        let _ = ctx.spawn(
            async move {
                super::git_ops::clone(
                    &url,
                    &parent,
                    Some(&directory_name),
                    |update| {
                        let _ = progress_sender.send(update);
                    },
                    move || progress_cancelled.load(Ordering::Relaxed),
                )
                .await
            },
            move |host: &mut Self, result, ctx| {
                for update in progress_receiver.try_iter() {
                    let body = body.clone();
                    body.update(ctx, |body, ctx| body.set_clone_progress(update, ctx));
                }
                match result {
                    Ok(root) => {
                        host.close_new_workspace_modal(ctx);
                        host.register_and_open_folder(root, ctx);
                    }
                    Err(err) => {
                        let body = host.modal_body(ctx);
                        body.update(ctx, |body, ctx| body.set_error(err.to_string(), ctx));
                    }
                }
            },
        );
    }

    fn start_create(&mut self, name: String, parent: PathBuf, ctx: &mut ViewContext<Self>) {
        WorkspaceCreationSettings::handle(ctx).update(ctx, |settings, ctx| {
            let _ = Setting::set_value(
                &mut settings.last_create_parent,
                parent.to_string_lossy().to_string(),
                ctx,
            );
        });

        let root = parent.join(&name);
        let _ = ctx.spawn(
            async move {
                super::git_ops::init_new_project(&root).await?;
                let branch = super::git_ops::current_branch(&root).await.ok();
                Ok::<_, anyhow::Error>((root, branch))
            },
            move |host: &mut Self, result, ctx| match result {
                Ok((root, branch)) => {
                    host.close_new_workspace_modal(ctx);
                    host.finish_registering_folder((root, ProjectKind::Git, branch), ctx);
                }
                Err(err) => {
                    let body = host.modal_body(ctx);
                    body.update(ctx, |body, ctx| body.set_error(err.to_string(), ctx));
                }
            },
        );
    }

    fn handle_overview_event(&mut self, event: &OverviewEvent, ctx: &mut ViewContext<Self>) {
        match event {
            OverviewEvent::Close => self.set_overview_active(false, ctx),
            OverviewEvent::ActivateHome => {
                self.set_overview_active(false, ctx);
                self.activate_screen(0, ctx);
            }
            OverviewEvent::OpenProject(project_id) => {
                let project_id = *project_id;
                self.set_overview_active(false, ctx);
                self.open_project(project_id, ctx);
            }
            OverviewEvent::NewWorkspace => self.show_new_workspace_modal(None, ctx),
            OverviewEvent::RemoveProject(project_id) => self.show_remove_dialog(*project_id, ctx),
            OverviewEvent::RevealProject(project_id) => {
                if let Some(path) = ProjectRegistryModel::as_ref(ctx)
                    .project(*project_id)
                    .map(|project| project.root_path.clone())
                {
                    ctx.open_file_path_in_explorer(&path);
                }
            }
            OverviewEvent::MissingRoot(project_id) => {
                let name = ProjectRegistryModel::as_ref(ctx)
                    .project(*project_id)
                    .map(|project| project.display_name.clone())
                    .unwrap_or_else(|| "Workspace".to_owned());
                self.toast(format!("'{name}' is no longer on disk"), ctx);
            }
        }
    }

    fn show_remove_dialog(&mut self, project_id: ProjectId, ctx: &mut ViewContext<Self>) {
        let registry = ProjectRegistryModel::as_ref(ctx);
        let Some(project) = registry.project(project_id) else {
            return;
        };
        let display_name = project.display_name.clone();
        let worktree_count = registry.linked_worktree_count(project_id);
        let worktree_note = (worktree_count > 0).then(|| {
            format!(
                "{worktree_count} worktrees will remain on disk under the Spirit data directory."
            )
        });

        let dialog = self.remove_dialog.view.clone();
        dialog.update(ctx, |dialog, _| {
            dialog.set_target(project_id, display_name, worktree_note);
        });
        self.remove_dialog.open();
        ctx.focus(&dialog);
        ctx.notify();
    }

    fn remove_project(&mut self, project_id: ProjectId, ctx: &mut ViewContext<Self>) {
        self.close_project_screen(project_id, ctx);
        ProjectRegistryModel::handle(ctx).update(ctx, |registry, ctx| {
            registry.remove_project(project_id, ctx);
        });
        let overview = self.overview.clone();
        overview.update(ctx, |overview, ctx| overview.refresh(ctx));
        ctx.notify();
    }

    fn toast(&self, message: String, ctx: &mut ViewContext<Self>) {
        let window_id = self.window_id;
        ToastStack::handle(ctx).update(ctx, |toasts, ctx| {
            toasts.add_ephemeral_toast(DismissibleToast::error(message), window_id, ctx);
        });
    }
}

fn save_app_state(ctx: &mut ViewContext<ProjectHost>) {
    ctx.dispatch_global_action("workspace:save_app", ());
}
