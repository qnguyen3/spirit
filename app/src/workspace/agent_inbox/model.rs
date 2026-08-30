use warpui::{Entity, EntityId, ModelContext, SingletonEntity};

use super::item::{InboxItem, InboxItemId, InboxItems};

#[derive(Debug, Clone, Copy)]
pub enum AgentInboxModelEvent {
    Changed,
}

#[derive(Default)]
pub struct AgentInboxModel {
    items: InboxItems,
}

impl Entity for AgentInboxModel {
    type Event = AgentInboxModelEvent;
}

impl SingletonEntity for AgentInboxModel {}

impl AgentInboxModel {
    pub fn items(&self) -> &InboxItems {
        &self.items
    }

    pub fn record(&mut self, item: InboxItem, ctx: &mut ModelContext<Self>) {
        self.items.push(item);
        ctx.emit(AgentInboxModelEvent::Changed);
    }

    pub fn mark_read(&mut self, id: InboxItemId, ctx: &mut ModelContext<Self>) {
        if self.items.mark_read(id) {
            ctx.emit(AgentInboxModelEvent::Changed);
        }
    }

    pub fn mark_all_read(&mut self, ctx: &mut ModelContext<Self>) {
        if self.items.mark_all_read() {
            ctx.emit(AgentInboxModelEvent::Changed);
        }
    }

    pub fn mark_terminal_view_read(
        &mut self,
        terminal_view_id: EntityId,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.items.mark_terminal_view_read(terminal_view_id) {
            ctx.emit(AgentInboxModelEvent::Changed);
        }
    }
}
