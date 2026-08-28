use std::collections::HashMap;

use warpui::{Entity, EntityId, SingletonEntity};

use crate::workspace::PaneViewLocator;

#[derive(Default)]
pub struct AgentPickerPaneManager {
    panes: HashMap<EntityId, PaneViewLocator>,
}

impl AgentPickerPaneManager {
    pub fn find_pane(&self, screen_id: EntityId) -> Option<PaneViewLocator> {
        self.panes.get(&screen_id).copied()
    }

    pub fn register_pane(&mut self, screen_id: EntityId, locator: PaneViewLocator) {
        self.panes.insert(screen_id, locator);
    }

    pub fn deregister_pane(&mut self, screen_id: &EntityId) {
        self.panes.remove(screen_id);
    }
}

impl Entity for AgentPickerPaneManager {
    type Event = ();
}

impl SingletonEntity for AgentPickerPaneManager {}
