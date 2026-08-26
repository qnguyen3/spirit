use std::collections::HashMap;

use warpui::{Entity, SingletonEntity, WindowId};

use crate::workspace::PaneViewLocator;

#[derive(Default)]
pub struct AgentPickerPaneManager {
    panes: HashMap<WindowId, PaneViewLocator>,
}

impl AgentPickerPaneManager {
    pub fn find_pane(&self, window_id: WindowId) -> Option<PaneViewLocator> {
        self.panes.get(&window_id).copied()
    }

    pub fn register_pane(&mut self, window_id: WindowId, locator: PaneViewLocator) {
        self.panes.insert(window_id, locator);
    }

    pub fn deregister_pane(&mut self, window_id: &WindowId) {
        self.panes.remove(window_id);
    }
}

impl Entity for AgentPickerPaneManager {
    type Event = ();
}

impl SingletonEntity for AgentPickerPaneManager {}
