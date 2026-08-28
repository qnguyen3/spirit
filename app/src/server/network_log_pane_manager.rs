//! Tracks open [`NetworkLogPane`]s across Workspace screens so that we show at
//! most one per screen and can focus the existing one when reopened.
use std::collections::HashMap;

use warpui::{Entity, EntityId, SingletonEntity};

use crate::workspace::PaneViewLocator;

/// Singleton that maintains a map of screen id -> `PaneViewLocator` for any
/// open network log panes.
#[derive(Default)]
pub struct NetworkLogPaneManager {
    panes: HashMap<EntityId, PaneViewLocator>,
}

impl NetworkLogPaneManager {
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

impl Entity for NetworkLogPaneManager {
    type Event = ();
}

impl SingletonEntity for NetworkLogPaneManager {}
