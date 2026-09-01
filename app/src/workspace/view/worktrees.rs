use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use warpui::{AppContext, SingletonEntity, ViewContext, ViewHandle};

use super::Workspace;
use crate::agent_launcher::pane_manager::AgentPickerPaneManager;
use crate::app_state::{LeafContents, LeafSnapshot, PaneNodeSnapshot, PaneUuid};
use crate::appearance::Appearance;
use crate::editor::{EditorView, Event as EditorEvent, SingleLineEditorOptions, TextOptions};
use crate::features::FeatureFlag;
use crate::modal::{Modal, ModalEvent, ModalViewState};
use crate::pane_group::{AGENT_PICKER_PANE_TITLE, NewTerminalOptions, PanesLayout};
use crate::projects::create_worktree_modal::{CreateWorktreeModal, CreateWorktreeModalEvent};
use crate::projects::delete_worktree_dialog::{DeleteWorktreeDialog, DeleteWorktreeEvent};
use crate::projects::git_ops::{
    self, BranchDeleteOutcome, WorktreeListEntry, generated_worktree_path, next_available,
    sanitize_worktree_name,
};
use crate::projects::registry::ProjectRegistryModel;
use crate::projects::{Project, ProjectId, Worktree, WorktreeId, WorktreeKind};
use crate::tab::TabData;
use crate::view_components::DismissibleToast;
use crate::workspace::ToastStack;

#[derive(Clone)]
pub(crate) struct WorktreeContext {
    pub project_id: ProjectId,
    pub root_path: PathBuf,
    pub primary_branch: String,
}

pub(crate) struct WorktreeCreation {
    pub name: String,
    pub branch: String,
    pub path: PathBuf,
    pub base_branch: String,
    pub agent_catalog_index: Option<usize>,
}

impl Workspace {
    pub(crate) fn build_create_worktree_modal(
        ctx: &mut ViewContext<Self>,
    ) -> ModalViewState<Modal<CreateWorktreeModal>> {
        let body = ctx.add_typed_action_view(CreateWorktreeModal::new);
        ctx.subscribe_to_view(&body, |me, _, event, ctx| match event {
            CreateWorktreeModalEvent::Close => me.close_create_worktree_modal(ctx),
            CreateWorktreeModalEvent::Submit {
                name,
                agent_catalog_index,
            } => me.start_worktree_creation(name.clone(), *agent_catalog_index, ctx),
        });
        let modal = ctx.add_typed_action_view(|ctx| Modal::new(None, body, ctx));
        ctx.subscribe_to_view(&modal, |me, _, event, ctx| {
            if matches!(event, ModalEvent::Close) {
                me.close_create_worktree_modal(ctx);
            }
        });
        ModalViewState::new(modal)
    }

    pub(crate) fn build_delete_worktree_dialog(
        ctx: &mut ViewContext<Self>,
    ) -> ModalViewState<DeleteWorktreeDialog> {
        let dialog = ctx.add_typed_action_view(DeleteWorktreeDialog::new);
        ctx.subscribe_to_view(&dialog, |me, _, event, ctx| match event {
            DeleteWorktreeEvent::Confirm { worktree_id, force } => {
                let (worktree_id, force) = (*worktree_id, *force);
                me.delete_worktree_dialog.close();
                me.execute_worktree_deletion(worktree_id, force, ctx);
            }
            DeleteWorktreeEvent::Cancel => {
                me.delete_worktree_dialog.close();
                ctx.notify();
            }
        });
        ModalViewState::new(dialog)
    }

    pub(crate) fn worktree_sections_enabled(&self) -> bool {
        FeatureFlag::AdeWorkspaces.is_enabled() && self.project_id().is_some()
    }

    pub(crate) fn resolved_worktree_id_of_tab(
        &self,
        tab_index: usize,
        ctx: &AppContext,
    ) -> Option<WorktreeId> {
        if !self.worktree_sections_enabled() {
            return None;
        }
        let worktree_id = self.tabs.get(tab_index)?.worktree_id?;
        ProjectRegistryModel::as_ref(ctx)
            .worktree(worktree_id)
            .map(|worktree| worktree.id)
    }

    pub(crate) fn worktree_run_range(&self, worktree_id: WorktreeId) -> Option<(usize, usize)> {
        let mut members = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, tab)| tab.worktree_id == Some(worktree_id))
            .map(|(index, _)| index);
        let first = members.next()?;
        let last = members.next_back().unwrap_or(first);
        Some((first, last))
    }

    pub(crate) fn index_after_worktree_run(&self, worktree_id: WorktreeId) -> Option<usize> {
        self.worktree_run_range(worktree_id)
            .map(|(_, last)| last + 1)
    }

    fn place_active_tab_in_worktree_run(
        &mut self,
        worktree_id: WorktreeId,
        ctx: &mut ViewContext<Self>,
    ) {
        if !self.worktree_sections_enabled() {
            return;
        }
        let new_index = self.active_tab_index();
        let bound = |tab: &TabData| tab.worktree_id == Some(worktree_id);
        let adjacent = new_index
            .checked_sub(1)
            .and_then(|index| self.tabs.get(index))
            .is_some_and(bound)
            || self.tabs.get(new_index + 1).is_some_and(bound);
        if adjacent {
            return;
        }
        let last_other = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(index, tab)| *index != new_index && bound(tab))
            .map(|(index, _)| index)
            .next_back();
        let Some(last_other) = last_other else {
            return;
        };
        let target = if new_index > last_other {
            last_other + 1
        } else {
            last_other
        };
        self.move_tab_to_index(new_index, target, ctx);
    }

    pub(crate) fn normalize_worktree_runs(&mut self, ctx: &mut ViewContext<Self>) {
        if !self.worktree_sections_enabled() {
            return;
        }
        while let Some((stray, target)) = self.first_worktree_stray() {
            self.hop_tab_to_index(stray, target, ctx);
        }
    }

    fn first_worktree_stray(&self) -> Option<(usize, usize)> {
        let mut run_end: HashMap<WorktreeId, usize> = HashMap::new();
        let mut previous: Option<WorktreeId> = None;
        for (index, tab) in self.tabs.iter().enumerate() {
            if self.is_tab_effectively_pinned(tab) {
                previous = None;
                continue;
            }
            let current = tab.worktree_id;
            if let Some(worktree_id) = current {
                if previous != current
                    && let Some(end) = run_end.get(&worktree_id)
                {
                    return Some((index, *end));
                }
                run_end.insert(worktree_id, index + 1);
            }
            previous = current;
        }
        None
    }

    pub(crate) fn add_tab_in_worktree(
        &mut self,
        worktree_id: WorktreeId,
        agent_catalog_index: Option<usize>,
        ctx: &mut ViewContext<Self>,
    ) {
        let registry = ProjectRegistryModel::as_ref(ctx);
        if registry.worktree(worktree_id).is_none() {
            return;
        }
        let Some(directory) = registry.worktree_directory(worktree_id) else {
            return;
        };
        self.add_tab_with_pane_layout(
            PanesLayout::SingleTerminal(Box::new(NewTerminalOptions {
                initial_directory: Some(directory),
                hide_homepage: true,
                ..Default::default()
            })),
            Arc::new(HashMap::<
                PaneUuid,
                Vec<crate::terminal::model::block::SerializedBlock>,
            >::new()),
            None,
            ctx,
        );
        self.bind_active_tab_to_worktree(Some(worktree_id), ctx);
        self.place_active_tab_in_worktree_run(worktree_id, ctx);
        self.collapsed_worktree_sections.remove(&worktree_id);
        if let Some(catalog_index) = agent_catalog_index {
            self.launch_agent_in_active_tab(catalog_index, ctx);
        }
        ctx.notify();
    }

    pub(crate) fn add_agent_picker_tab_in_worktree(
        &mut self,
        worktree_id: WorktreeId,
        ctx: &mut ViewContext<Self>,
    ) {
        if ProjectRegistryModel::as_ref(ctx)
            .worktree(worktree_id)
            .is_none()
        {
            return;
        }
        let existing_picker = AgentPickerPaneManager::handle(ctx)
            .as_ref(ctx)
            .find_pane(ctx.view_id());
        match existing_picker {
            Some(locator) => self.focus_pane(locator, ctx),
            None => self.add_tab_with_pane_layout(
                PanesLayout::Snapshot(Box::new(PaneNodeSnapshot::Leaf(LeafSnapshot {
                    is_focused: true,
                    custom_vertical_tabs_title: None,
                    contents: LeafContents::AgentPicker,
                }))),
                Arc::new(HashMap::new()),
                Some(AGENT_PICKER_PANE_TITLE.to_owned()),
                ctx,
            ),
        }
        self.bind_active_tab_to_worktree(Some(worktree_id), ctx);
        self.place_active_tab_in_worktree_run(worktree_id, ctx);
        self.collapsed_worktree_sections.remove(&worktree_id);
        ctx.notify();
    }

    pub(crate) fn toggle_worktree_section_collapsed(
        &mut self,
        worktree_id: WorktreeId,
        ctx: &mut ViewContext<Self>,
    ) {
        if !self.collapsed_worktree_sections.remove(&worktree_id) {
            self.collapsed_worktree_sections.insert(worktree_id);
        }
        ctx.notify();
    }

    pub(crate) fn rename_worktree_inline(
        &mut self,
        worktree_id: WorktreeId,
        ctx: &mut ViewContext<Self>,
    ) {
        let registry = ProjectRegistryModel::as_ref(ctx);
        let Some(worktree) = registry.worktree(worktree_id) else {
            return;
        };
        if worktree.is_primary() {
            return;
        }
        let seed_text = worktree.name.clone();
        self.current_workspace_state
            .set_worktree_being_renamed(worktree_id);
        self.clear_worktree_name_editor(ctx);
        self.worktree_rename_editor.update(ctx, move |editor, ctx| {
            editor.insert_selected_text(&seed_text, ctx);
        });
        ctx.focus(&self.worktree_rename_editor);
        ctx.notify();
    }

    fn clear_worktree_name_editor(&mut self, ctx: &mut ViewContext<Self>) {
        self.worktree_rename_editor.update(ctx, move |editor, ctx| {
            editor.clear_buffer_and_reset_undo_stack(ctx);
        });
    }

    pub(crate) fn build_worktree_rename_editor(
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<EditorView> {
        let editor = ctx.add_typed_action_view(|ctx| {
            let appearance = Appearance::as_ref(ctx);
            let options = SingleLineEditorOptions {
                text: TextOptions::ui_text(Some(12.), appearance),
                ..Default::default()
            };
            EditorView::single_line(options, ctx)
        });
        ctx.subscribe_to_view(&editor, move |me, _, event, ctx| {
            me.handle_worktree_rename_editor_event(event, ctx);
        });
        editor
    }

    pub(crate) fn handle_worktree_rename_editor_event(
        &mut self,
        event: &EditorEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        if self.current_workspace_state.is_any_worktree_being_renamed() {
            match event {
                EditorEvent::Blurred | EditorEvent::Enter => {
                    self.finish_worktree_rename(ctx);
                }
                EditorEvent::Escape => {
                    self.cancel_worktree_rename(ctx);
                }
                _ => {}
            }
        }
    }

    fn finish_worktree_rename(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(worktree_id) = self.current_workspace_state.worktree_being_renamed() else {
            return;
        };
        self.current_workspace_state.clear_worktree_being_renamed();
        let title = self.worktree_rename_editor.as_ref(ctx).buffer_text(ctx);
        let trimmed = title.trim();
        let is_linked = ProjectRegistryModel::as_ref(ctx)
            .worktree(worktree_id)
            .is_some_and(|worktree| !worktree.is_primary());
        if !trimmed.is_empty() && is_linked {
            let name = trimmed.to_owned();
            ProjectRegistryModel::handle(ctx).update(ctx, |registry, ctx| {
                registry.rename_worktree(worktree_id, name, ctx);
            });
        }
        self.clear_worktree_name_editor(ctx);
        self.focus_active_tab(ctx);
        ctx.notify();
    }

    pub(crate) fn cancel_worktree_rename(&mut self, ctx: &mut ViewContext<Self>) {
        if self.current_workspace_state.is_any_worktree_being_renamed() {
            self.current_workspace_state.clear_worktree_being_renamed();
            self.clear_worktree_name_editor(ctx);
            self.focus_active_tab(ctx);
            ctx.notify();
        }
    }

    pub(crate) fn worktree_section_new_tab_menu_items(
        &self,
        worktree_id: WorktreeId,
        ctx: &AppContext,
    ) -> Vec<crate::menu::MenuItem<crate::workspace::WorkspaceAction>> {
        use crate::menu::MenuItemFields;
        use crate::ui_components::icons;
        use crate::workspace::WorkspaceAction;

        if ProjectRegistryModel::as_ref(ctx)
            .worktree(worktree_id)
            .is_none()
        {
            return Vec::new();
        }
        vec![
            MenuItemFields::new("New Agent\u{2026}")
                .with_on_select_action(WorkspaceAction::NewAgentPickerInWorktree { worktree_id })
                .with_icon(icons::Icon::Agent)
                .into_item(),
            MenuItemFields::new("Terminal")
                .with_on_select_action(WorkspaceAction::NewTerminalInWorktree { worktree_id })
                .with_icon(icons::Icon::LayoutAlt01)
                .into_item(),
        ]
    }

    pub(crate) fn worktree_section_menu_items(
        &self,
        worktree_id: WorktreeId,
        ctx: &AppContext,
    ) -> Vec<crate::menu::MenuItem<crate::workspace::WorkspaceAction>> {
        use crate::menu::MenuItemFields;
        use crate::workspace::WorkspaceAction;

        let registry = ProjectRegistryModel::as_ref(ctx);
        let Some(worktree) = registry.worktree(worktree_id) else {
            return Vec::new();
        };
        let is_linked = !worktree.is_primary();

        let mut items = vec![
            MenuItemFields::new("Reveal Worktree Folder")
                .with_on_select_action(WorkspaceAction::RevealWorktreeFolder { worktree_id })
                .into_item(),
        ];
        if is_linked {
            items.push(
                MenuItemFields::new("Rename Worktree")
                    .with_on_select_action(WorkspaceAction::RenameWorktree { worktree_id })
                    .into_item(),
            );
            items.push(
                MenuItemFields::new("Delete Worktree\u{2026}")
                    .with_on_select_action(WorkspaceAction::DeleteWorktree { worktree_id })
                    .into_item(),
            );
        }
        items
    }

    pub(crate) fn worktree_context(&self, ctx: &AppContext) -> Option<WorktreeContext> {
        if !FeatureFlag::AdeWorkspaces.is_enabled() {
            return None;
        }
        let project_id = self.project_id()?;
        let registry = ProjectRegistryModel::as_ref(ctx);
        let project = registry.project(project_id)?;
        if !matches!(project.kind, crate::projects::ProjectKind::Git) {
            return None;
        }
        Some(WorktreeContext {
            project_id,
            root_path: project.root_path.clone(),
            primary_branch: project.primary_branch.clone()?,
        })
    }

    pub(crate) fn can_create_worktree(&self, ctx: &AppContext) -> bool {
        self.worktree_context(ctx).is_some()
    }

    pub(crate) fn closed_worktrees(&self, ctx: &AppContext) -> Vec<(WorktreeId, String)> {
        let Some(project_id) = self.project_id() else {
            return Vec::new();
        };
        let open: HashSet<WorktreeId> = self.worktree_ids_with_tabs().into_iter().collect();
        ProjectRegistryModel::as_ref(ctx)
            .worktrees_for_project(project_id)
            .into_iter()
            .filter(|worktree| !open.contains(&worktree.id))
            .map(|worktree| (worktree.id, worktree.name.clone()))
            .collect()
    }

    pub(crate) fn show_create_worktree_modal(
        &mut self,
        agent_catalog_index: Option<usize>,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(context) = self.worktree_context(ctx) else {
            return;
        };
        let root_path = context.root_path.clone();
        let primary_branch = context.primary_branch.clone();
        let existing = ProjectRegistryModel::as_ref(ctx)
            .worktree_names_for_project(context.project_id)
            .into_iter()
            .chain(crate::util::git::list_local_branches_sync(&root_path))
            .collect::<HashSet<String>>();

        let body = self.create_worktree_modal.view.as_ref(ctx).body().clone();
        body.update(ctx, |body, ctx| {
            body.on_open(primary_branch, existing, agent_catalog_index, ctx);
        });
        self.create_worktree_modal.open();
        ctx.focus(&self.create_worktree_modal.view);
        ctx.notify();
    }

    pub(crate) fn close_create_worktree_modal(&mut self, ctx: &mut ViewContext<Self>) {
        self.create_worktree_modal.close();
        self.focus_active_tab(ctx);
        ctx.notify();
    }

    fn start_worktree_creation(
        &mut self,
        name: String,
        agent_catalog_index: Option<usize>,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(context) = self.worktree_context(ctx) else {
            return;
        };
        let taken_names =
            ProjectRegistryModel::as_ref(ctx).worktree_names_for_project(context.project_id);

        let root_path = context.root_path.clone();
        let base_branch = context.primary_branch.clone();
        let sanitized = sanitize_worktree_name(&name);
        let path_future = crate::projects::interactive_path_env(ctx);
        let _ = ctx.spawn(
            async move {
                let path_env = path_future.await;
                let path_env = path_env.as_deref();
                let branches = git_ops::local_branches(&root_path).await;
                let branch = next_available(&sanitized, &|candidate| {
                    taken_names.contains(candidate)
                        || branches.contains(candidate)
                        || generated_worktree_path(&root_path, candidate).exists()
                });
                let path = generated_worktree_path(&root_path, &branch);

                git_ops::worktree_add_or_rollback(
                    &root_path,
                    &branch,
                    &path,
                    &base_branch,
                    path_env,
                )
                .await?;

                let report = git_ops::copy_worktree_includes(&root_path, &path)
                    .await
                    .unwrap_or_default();
                Ok::<_, anyhow::Error>((
                    WorktreeCreation {
                        name: branch.clone(),
                        branch,
                        path,
                        base_branch,
                        agent_catalog_index,
                    },
                    report,
                ))
            },
            move |me: &mut Self, result, ctx| match result {
                Ok((creation, report)) => me.finish_worktree_creation(creation, report, ctx),
                Err(err) => {
                    log::warn!("Worktree creation failed: {err:#}");
                    let detail = crate::projects::error_summary(&err);
                    let body = me.create_worktree_modal.view.as_ref(ctx).body().clone();
                    body.update(ctx, |body, ctx| body.set_error(detail, ctx));
                }
            },
        );
    }

    fn finish_worktree_creation(
        &mut self,
        creation: WorktreeCreation,
        include_report: git_ops::IncludeCopyReport,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(context) = self.worktree_context(ctx) else {
            return;
        };
        self.create_worktree_modal.close();

        let worktree_id = ProjectRegistryModel::handle(ctx).update(ctx, |registry, ctx| {
            registry.add_linked_worktree(
                context.project_id,
                creation.name.clone(),
                creation.path.clone(),
                creation.branch.clone(),
                creation.base_branch.clone(),
                ctx,
            )
        });

        self.add_tab_in_worktree(worktree_id, creation.agent_catalog_index, ctx);

        if let Some(message) = include_report.summary() {
            self.toast_worktree_message(message, ctx);
        }

        ctx.notify();
    }

    pub(crate) fn open_worktree_tab(
        &mut self,
        worktree_id: WorktreeId,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(index) = self.tab_index_for_worktree(worktree_id) {
            self.activate_tab_internal(index, ctx);
            return;
        }
        self.add_tab_in_worktree(worktree_id, None, ctx);
    }

    pub(crate) fn show_delete_worktree_dialog(
        &mut self,
        worktree_id: WorktreeId,
        ctx: &mut ViewContext<Self>,
    ) {
        let registry = ProjectRegistryModel::as_ref(ctx);
        let Some(worktree) = registry.worktree(worktree_id) else {
            return;
        };
        if worktree.is_primary() {
            return;
        }
        let name = worktree.name.clone();
        let branch = worktree.branch().unwrap_or_default().to_owned();
        let Some(directory) = registry.worktree_directory(worktree_id) else {
            return;
        };

        let dialog = self.delete_worktree_dialog.view.clone();
        dialog.update(ctx, |dialog, _| {
            dialog.set_target(worktree_id, name, branch);
        });
        self.delete_worktree_dialog.open();
        ctx.focus(&dialog);
        ctx.notify();

        let path_future = crate::projects::interactive_path_env(ctx);
        let _ = ctx.spawn(
            async move {
                let path_env = path_future.await;
                git_ops::status_is_dirty(&directory, path_env.as_deref())
                    .await
                    .unwrap_or(false)
            },
            move |me: &mut Self, dirty, ctx| {
                let dialog = me.delete_worktree_dialog.view.clone();
                dialog.update(ctx, |dialog, ctx| dialog.set_dirty(dirty, ctx));
            },
        );
    }

    // Tabs are closed before any git call: git refuses to remove a worktree a
    // live shell sits in, and removing it underneath one races its writes.
    fn execute_worktree_deletion(
        &mut self,
        worktree_id: WorktreeId,
        force: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(context) = self.worktree_context(ctx) else {
            return;
        };
        let registry = ProjectRegistryModel::as_ref(ctx);
        let Some(worktree) = registry.worktree(worktree_id) else {
            return;
        };
        let name = worktree.name.clone();
        let branch = worktree.branch().unwrap_or_default().to_owned();
        let Some(directory) = registry.worktree_directory(worktree_id) else {
            return;
        };

        let indices: Vec<usize> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, tab)| tab.worktree_id == Some(worktree_id))
            .map(|(index, _)| index)
            .collect();
        if !indices.is_empty() {
            self.close_tabs(
                indices.into_iter().rev(),
                true,
                false,
                ctx,
            );
        }

        let root_path = context.root_path.clone();
        let path_future = crate::projects::interactive_path_env(ctx);
        let _ = ctx.spawn(
            async move {
                let path_env = path_future.await;
                git_ops::worktree_remove(&root_path, &directory, force, path_env.as_deref())
                    .await?;
                let outcome = if branch.is_empty() {
                    BranchDeleteOutcome::Deleted
                } else {
                    git_ops::delete_branch_safe(&root_path, &branch).await?
                };
                let _ = git_ops::worktree_prune(&root_path).await;
                Ok::<_, anyhow::Error>((outcome, branch))
            },
            move |me: &mut Self, result, ctx| match result {
                Ok((outcome, branch)) => {
                    let removed = ProjectRegistryModel::handle(ctx).update(ctx, |registry, ctx| {
                        registry.remove_worktree(worktree_id, ctx)
                    });
                    if let Err(err) = removed {
                        log::warn!(
                            "Failed to drop worktree {worktree_id} from the registry: {err}"
                        );
                    }
                    if outcome == BranchDeleteOutcome::KeptUnmerged {
                        me.toast_worktree_message(
                            format!("Branch {branch} kept (unmerged work)"),
                            ctx,
                        );
                    }
                    ctx.notify();
                }
                Err(err) => {
                    log::warn!("Worktree deletion failed: {err:#}");
                    let detail = crate::projects::error_summary(&err);
                    me.toast_worktree_message(format!("Could not delete '{name}': {detail}"), ctx);
                }
            },
        );
    }

    pub(crate) fn reveal_worktree_folder(
        &mut self,
        worktree_id: WorktreeId,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(directory) = ProjectRegistryModel::as_ref(ctx).worktree_directory(worktree_id) {
            ctx.open_file_path_in_explorer(&directory);
        }
    }

    pub(crate) fn reconcile_worktrees(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(context) = self.worktree_context(ctx) else {
            return;
        };
        let root_path = context.root_path.clone();
        let _ = ctx.spawn(
            async move {
                let entries = git_ops::worktree_list(&root_path).await.unwrap_or_default();
                let primary_branch = git_ops::detect_primary_branch(&root_path).await.ok();
                (entries, primary_branch)
            },
            move |me: &mut Self, (entries, primary_branch), ctx| {
                me.apply_worktree_reconciliation(context.project_id, entries, primary_branch, ctx);
            },
        );
    }

    fn apply_worktree_reconciliation(
        &mut self,
        project_id: ProjectId,
        entries: Vec<WorktreeListEntry>,
        primary_branch: Option<String>,
        ctx: &mut ViewContext<Self>,
    ) {
        let registry = ProjectRegistryModel::as_ref(ctx);
        let Some(project) = registry.project(project_id).cloned() else {
            return;
        };
        let worktrees: Vec<Worktree> = registry
            .worktrees_for_project(project_id)
            .into_iter()
            .cloned()
            .collect();

        let outcomes: Vec<WorktreeReconciliation> = worktrees
            .iter()
            .map(|worktree| reconcile_worktree(worktree, &project, &entries))
            .collect();

        if let Some(branch) = primary_branch {
            ProjectRegistryModel::handle(ctx).update(ctx, |registry, ctx| {
                registry.set_primary_branch(project_id, Some(branch), ctx);
            });
        }

        for (worktree, outcome) in worktrees.iter().zip(outcomes) {
            match outcome {
                WorktreeReconciliation::Keep => {}
                WorktreeReconciliation::UpdateBranch(branch) => {
                    ProjectRegistryModel::handle(ctx).update(ctx, |registry, ctx| {
                        registry.set_worktree_branch(worktree.id, branch.clone(), ctx);
                    });
                }
                WorktreeReconciliation::Remove => {
                    let indices: Vec<usize> = self
                        .tabs
                        .iter()
                        .enumerate()
                        .filter(|(_, tab)| tab.worktree_id == Some(worktree.id))
                        .map(|(index, _)| index)
                        .collect();
                    if !indices.is_empty() {
                        self.close_tabs(
                            indices.into_iter().rev(),
                            true,
                            false,
                            ctx,
                        );
                    }
                    let _ = ProjectRegistryModel::handle(ctx).update(ctx, |registry, ctx| {
                        registry.remove_worktree(worktree.id, ctx)
                    });
                    self.toast_worktree_message(
                        format!("'{}' was removed outside Spirit", worktree.name),
                        ctx,
                    );
                }
            }
        }
        self.normalize_worktree_runs(ctx);
        ctx.notify();
    }

    pub(crate) fn rename_bound_worktree(
        &mut self,
        tab_index: usize,
        title: &str,
        ctx: &mut ViewContext<Self>,
    ) {
        let title = title.trim();
        if title.is_empty() {
            return;
        }
        let Some(worktree_id) = self.tabs.get(tab_index).and_then(|tab| tab.worktree_id) else {
            return;
        };
        let is_linked = ProjectRegistryModel::as_ref(ctx)
            .worktree(worktree_id)
            .is_some_and(|worktree| !worktree.is_primary());
        if !is_linked {
            return;
        }
        let title = title.to_owned();
        ProjectRegistryModel::handle(ctx).update(ctx, |registry, ctx| {
            registry.rename_worktree(worktree_id, title, ctx);
        });
    }

    pub(crate) fn worktree_tab_menu_items(
        &self,
        tab_index: usize,
        ctx: &AppContext,
    ) -> Vec<crate::menu::MenuItem<crate::workspace::WorkspaceAction>> {
        use crate::menu::{MenuItem, MenuItemFields};
        use crate::workspace::WorkspaceAction;

        if !self.can_create_worktree(ctx) {
            return Vec::new();
        }
        let mut items = vec![
            MenuItem::Separator,
            MenuItemFields::new("New Worktree\u{2026}")
                .with_on_select_action(WorkspaceAction::ShowCreateWorktreeModal {
                    agent_catalog_index: None,
                })
                .into_item(),
        ];

        let worktree_id = self.tabs.get(tab_index).and_then(|tab| tab.worktree_id);
        if let Some(worktree_id) = worktree_id {
            let registry = ProjectRegistryModel::as_ref(ctx);
            let is_known = registry.worktree(worktree_id).is_some();
            let is_linked = registry
                .worktree(worktree_id)
                .is_some_and(|worktree| !worktree.is_primary());
            if is_known {
                items.push(
                    MenuItemFields::new("New Terminal in Worktree")
                        .with_on_select_action(WorkspaceAction::NewTerminalInWorktree {
                            worktree_id,
                        })
                        .into_item(),
                );
            }
            if is_linked {
                items.push(
                    MenuItemFields::new("Reveal Worktree Folder")
                        .with_on_select_action(WorkspaceAction::RevealWorktreeFolder {
                            worktree_id,
                        })
                        .into_item(),
                );
                items.push(
                    MenuItemFields::new("Delete Worktree\u{2026}")
                        .with_on_select_action(WorkspaceAction::DeleteWorktree { worktree_id })
                        .into_item(),
                );
            }
        }
        items
    }

    pub(crate) fn toast_worktree_message(&self, message: String, ctx: &mut ViewContext<Self>) {
        let window_id = ctx.window_id();
        ToastStack::handle(ctx).update(ctx, |toasts, ctx| {
            toasts.add_ephemeral_toast(DismissibleToast::error(message), window_id, ctx);
        });
    }
}

pub(crate) fn worktree_run_partition(
    bindings: &[Option<WorktreeId>],
) -> Vec<(Option<WorktreeId>, usize)> {
    let mut runs: Vec<(Option<WorktreeId>, usize)> = Vec::new();
    for binding in bindings {
        match runs.last_mut() {
            Some((current, len)) if current == binding => *len += 1,
            Some(_) | None => runs.push((*binding, 1)),
        }
    }
    runs
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorktreeReconciliation {
    Keep,
    UpdateBranch(String),
    Remove,
}

pub(crate) fn reconcile_worktree(
    worktree: &Worktree,
    project: &Project,
    entries: &[WorktreeListEntry],
) -> WorktreeReconciliation {
    let WorktreeKind::Linked { path, branch, .. } = &worktree.kind else {
        return WorktreeReconciliation::Keep;
    };
    let _ = project;

    let entry = entries
        .iter()
        .find(|entry| git_ops::same_path(&entry.path, path));

    match entry {
        None if path.exists() => WorktreeReconciliation::Keep,
        None => WorktreeReconciliation::Remove,
        Some(entry) if entry.prunable => WorktreeReconciliation::Remove,
        Some(entry) => match &entry.branch {
            Some(actual) if actual != branch => {
                WorktreeReconciliation::UpdateBranch(actual.clone())
            }
            _ => WorktreeReconciliation::Keep,
        },
    }
}

#[cfg(test)]
#[path = "worktrees_tests.rs"]
mod tests;
