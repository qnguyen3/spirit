use std::collections::HashMap;

use warpui::{Entity, EntityId, ModelContext, SingletonEntity, ViewHandle};

use super::SettingsView;
use crate::PaneViewLocator;
use crate::pane_group::{PaneContent, PaneId, SettingsPane};
struct SettingsPaneData {
    locator: Option<PaneViewLocator>,
    settings_view: ViewHandle<SettingsView>,
}

/// Singleton model to manage state of settings panes across Workspace screens
/// (where only one settings pane can exist per screen). Specifically:
/// - Maintains settings view handles to preserve state when panes are hidden
/// - Tracks currently open settings panes and their location
#[derive(Default)]
pub struct SettingsPaneManager {
    panes: HashMap<EntityId, SettingsPaneData>,
}

impl SettingsPaneManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn settings_view(&self, screen_id: EntityId) -> ViewHandle<SettingsView> {
        self.panes
            .get(&screen_id)
            .expect("Screen should have corresponding settings view")
            .settings_view
            .clone()
    }

    pub fn settings_view_for(
        &self,
        screen_id: Option<EntityId>,
    ) -> Option<ViewHandle<SettingsView>> {
        screen_id
            .and_then(|screen_id| self.panes.get(&screen_id))
            .or_else(|| self.panes.values().next())
            .map(|data| data.settings_view.clone())
    }

    pub fn has_settings_view(&self, screen_id: EntityId) -> bool {
        self.panes.contains_key(&screen_id)
    }

    pub fn register_view(&mut self, screen_id: EntityId, view: ViewHandle<SettingsView>) {
        if let Some(data) = self.panes.get_mut(&screen_id) {
            data.settings_view = view;
        } else {
            self.panes.insert(
                screen_id,
                SettingsPaneData {
                    locator: None,
                    settings_view: view,
                },
            );
        }
    }

    pub fn find_pane(&self, screen_id: EntityId) -> Option<PaneViewLocator> {
        self.panes.get(&screen_id).and_then(|data| data.locator)
    }

    pub fn forget_screen(&mut self, screen_id: &EntityId) {
        self.panes.remove(screen_id);
    }

    pub fn register_pane(
        &mut self,
        pane: &SettingsPane,
        pane_group_id: EntityId,
        screen_id: EntityId,
        _ctx: &mut ModelContext<Self>,
    ) {
        if let Some(data) = self.panes.get_mut(&screen_id) {
            data.locator = Some(PaneViewLocator {
                pane_group_id,
                pane_id: pane.id(),
            });
        } else {
            log::warn!("Settings view should already exist for settings pane");
        }
    }

    pub fn deregister_pane(
        &mut self,
        screen_id: &EntityId,
        pane_group_id: EntityId,
        pane_id: PaneId,
        _ctx: &mut ModelContext<Self>,
    ) {
        if let Some(data) = self.panes.get_mut(screen_id) {
            let locator = PaneViewLocator {
                pane_group_id,
                pane_id,
            };
            if data.locator == Some(locator) {
                data.locator = None;
            }
        }
    }
}

impl Entity for SettingsPaneManager {
    type Event = ();
}

/// Mark SettingsPaneManager as global application state.
impl SingletonEntity for SettingsPaneManager {}
