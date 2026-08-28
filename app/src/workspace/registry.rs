use std::collections::HashMap;

use warpui::{AppContext, Entity, EntityId, SingletonEntity, ViewHandle, WeakViewHandle, WindowId};

use super::Workspace;
use crate::projects::ProjectId;

struct WindowScreens {
    screens: Vec<(Option<ProjectId>, WeakViewHandle<Workspace>)>,
    active: Option<EntityId>,
}

impl WindowScreens {
    fn active_handle(&self) -> Option<&WeakViewHandle<Workspace>> {
        let active = self.active?;
        self.screens
            .iter()
            .find(|(_, handle)| handle.id() == active)
            .map(|(_, handle)| handle)
            .or_else(|| self.screens.first().map(|(_, handle)| handle))
    }
}

/// A registry that tracks every workspace screen of every window, and which
/// screen of each window is currently on top.
///
/// This provides O(1) lookup of workspaces instead of the O(n) linear scan
/// that `views_of_type::<Workspace>` performs — which is now also ambiguous,
/// since a window holds one `Workspace` view per open Project plus Home.
pub struct WorkspaceRegistry {
    windows: HashMap<WindowId, WindowScreens>,
}

impl Default for WorkspaceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceRegistry {
    pub fn new() -> Self {
        Self {
            windows: HashMap::new(),
        }
    }

    pub fn register(&mut self, window_id: WindowId, workspace: WeakViewHandle<Workspace>) {
        let id = workspace.id();
        let entry = self
            .windows
            .entry(window_id)
            .or_insert_with(|| WindowScreens {
                screens: Vec::new(),
                active: None,
            });
        if !entry.screens.iter().any(|(_, handle)| handle.id() == id) {
            entry.screens.push((None, workspace));
        }
        if entry.active.is_none() {
            entry.active = Some(id);
        }
    }

    pub fn set_screens(
        &mut self,
        window_id: WindowId,
        screens: Vec<(Option<ProjectId>, EntityId)>,
        active: EntityId,
    ) {
        let Some(entry) = self.windows.get_mut(&window_id) else {
            return;
        };
        let mut ordered = Vec::with_capacity(screens.len());
        for (project_id, view_id) in screens {
            if let Some((_, handle)) = entry
                .screens
                .iter()
                .find(|(_, handle)| handle.id() == view_id)
            {
                ordered.push((project_id, handle.clone()));
            }
        }
        entry.screens = ordered;
        entry.active = Some(active);
    }

    pub fn unregister(&mut self, window_id: WindowId) {
        self.windows.remove(&window_id);
    }

    pub fn unregister_workspace(&mut self, window_id: WindowId, workspace_id: EntityId) {
        let Some(entry) = self.windows.get_mut(&window_id) else {
            return;
        };
        entry
            .screens
            .retain(|(_, handle)| handle.id() != workspace_id);
        if entry.active == Some(workspace_id) {
            entry.active = entry.screens.first().map(|(_, handle)| handle.id());
        }
    }

    pub fn get(&self, window_id: WindowId, app: &AppContext) -> Option<ViewHandle<Workspace>> {
        self.active_workspace(window_id, app)
    }

    pub fn active_workspace(
        &self,
        window_id: WindowId,
        app: &AppContext,
    ) -> Option<ViewHandle<Workspace>> {
        self.windows.get(&window_id)?.active_handle()?.upgrade(app)
    }

    pub fn screen_ids_for_window(&self, window_id: WindowId) -> Vec<EntityId> {
        self.windows
            .get(&window_id)
            .map(|entry| {
                entry
                    .screens
                    .iter()
                    .map(|(_, handle)| handle.id())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn active_workspace_view_id(&self, window_id: WindowId) -> Option<EntityId> {
        self.windows
            .get(&window_id)?
            .active_handle()
            .map(|handle| handle.id())
    }

    pub fn workspaces_for_window(
        &self,
        window_id: WindowId,
        app: &AppContext,
    ) -> Vec<ViewHandle<Workspace>> {
        self.windows
            .get(&window_id)
            .map(|entry| {
                entry
                    .screens
                    .iter()
                    .filter_map(|(_, handle)| handle.upgrade(app))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn window_owning_project(&self, project_id: ProjectId) -> Option<WindowId> {
        self.windows.iter().find_map(|(window_id, entry)| {
            entry
                .screens
                .iter()
                .any(|(screen_project, _)| *screen_project == Some(project_id))
                .then_some(*window_id)
        })
    }

    pub fn all_workspaces(&self, app: &AppContext) -> Vec<(WindowId, ViewHandle<Workspace>)> {
        self.windows
            .iter()
            .flat_map(|(window_id, entry)| {
                entry
                    .screens
                    .iter()
                    .filter_map(|(_, handle)| {
                        handle.upgrade(app).map(|handle| (*window_id, handle))
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

impl Entity for WorkspaceRegistry {
    type Event = ();
}

impl SingletonEntity for WorkspaceRegistry {}
