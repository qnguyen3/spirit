use uuid::Uuid;
use warp_errors::report_error;
use warpui::r#async::SpawnedFutureHandle;
use warpui::{
    AppContext, ClosedWindowData, Entity, EntityId, ModelContext, SingletonEntity, ViewHandle,
    WeakViewHandle, WindowId,
};

use super::UndoCloseSettings;
use super::settings::UndoCloseSettingsChangedEvent;
use crate::pane_group::{PaneGroup, PaneId};
use crate::root_view::RootView;
use crate::tab::TabData;
use crate::window_settings::WindowSettings;
use crate::workspace::Workspace;

/// A unique identifier for an item in the undo close stack.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct ItemId(Uuid);

impl ItemId {
    /// Constructs a new ItemId.
    fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// Data for an item in the undo close stack.
struct UndoData {
    closed_item: ClosedItem,
    expiry_data: ExpiryData,
}

/// Data needed to handle expiration for items in the undo close stack.
struct ExpiryData {
    id: ItemId,
    task_handle: SpawnedFutureHandle,
}

impl std::ops::Drop for ExpiryData {
    fn drop(&mut self) {
        // Make sure we abort the expiry task when we drop the expiry data.
        self.task_handle.abort();
    }
}

/// Data needed to restore a closed pane.
pub(super) struct PaneData {
    /// The pane ID - content is retrieved from the pane group during restoration
    pane_id: PaneId,
    /// Reference to the pane group that contained this pane
    pane_group: WeakViewHandle<PaneGroup>,
}

/// An item in the undo close stack which can be re-opened.
pub enum ClosedItem {
    Window(Box<ClosedWindowData>),
    Tab {
        workspace: WeakViewHandle<Workspace>,
        tab_index: usize,
        data: TabData,
    },
    Pane {
        data: PaneData,
    },
}

impl ClosedItem {
    fn discard(self, ctx: &mut ModelContext<UndoCloseStack>) {
        match self {
            ClosedItem::Window(data) => {
                let ClosedWindowData { window_id, .. } = *data;
                for workspace in window_workspaces(window_id, ctx) {
                    workspace.update(ctx, |workspace, ctx| {
                        for pane_group in workspace.tab_views() {
                            Self::clean_up_pane_group(pane_group, ctx);
                        }
                    });
                }
            }
            ClosedItem::Tab { data, .. } => {
                Self::clean_up_pane_group(&data.pane_group, ctx);
            }
            ClosedItem::Pane { data } => {
                ctx.emit(UndoCloseStackEvent::DiscardPane(data.pane_id));
            }
        }
    }

    fn clean_up_pane_group(pane_group: &ViewHandle<PaneGroup>, ctx: &mut AppContext) {
        let window_id = pane_group.window_id(ctx);

        if !ctx.is_window_open(window_id) {
            return;
        }

        pane_group.update(ctx, |pane_group, ctx| {
            pane_group.clean_up_panes(ctx);
        });
    }
}

pub enum UndoCloseStackEvent {
    DiscardPane(PaneId),
}

/// A stack of closed items which can be re-opened in LIFO order.
pub struct UndoCloseStack {
    stack: Vec<UndoData>,
}

impl UndoCloseStack {
    /// Constructs a new undo close stack.
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        ctx.subscribe_to_model(&UndoCloseSettings::handle(ctx), |me, _, event, ctx| {
            me.handle_settings_event(event, ctx);
        });

        Self {
            stack: Default::default(),
        }
    }

    /// Returns whether or not the stack is empty.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Handles a window being closed, adding the necessary data to the undo
    /// stack.
    pub fn handle_window_closed(&mut self, data: ClosedWindowData, ctx: &mut ModelContext<Self>) {
        self.push_item(ClosedItem::Window(Box::new(data)), ctx);
    }

    /// Handles a tab being closed, adding the necessary data to the undo
    /// stack.
    pub fn handle_tab_closed(
        &mut self,
        workspace: WeakViewHandle<Workspace>,
        tab_index: usize,
        data: TabData,
        ctx: &mut ModelContext<Self>,
    ) {
        self.push_item(
            ClosedItem::Tab {
                workspace,
                tab_index,
                data,
            },
            ctx,
        );
    }

    /// Handles a pane being closed, adding the necessary data to the undo stack.
    pub fn handle_pane_closed_by_id(
        &mut self,
        pane_group: WeakViewHandle<PaneGroup>,
        pane_id: PaneId,
        ctx: &mut ModelContext<Self>,
    ) {
        let pane_data = PaneData {
            pane_id,
            pane_group,
        };

        self.push_item(ClosedItem::Pane { data: pane_data }, ctx);
    }

    pub fn reopen_last_closed_window(&mut self, ctx: &mut AppContext) -> bool {
        let last_closed_window = self
            .stack
            .last()
            .is_some_and(|item| matches!(item.closed_item, ClosedItem::Window(_)));
        if !last_closed_window {
            return false;
        }
        self.undo_close(ctx);
        true
    }

    /// Undoes the last close action in the stack, if possible.
    pub fn undo_close(&mut self, ctx: &mut AppContext) {
        let Some(UndoData { closed_item, .. }) = self.stack.pop() else {
            return;
        };

        match closed_item {
            ClosedItem::Window(data) => {
                let window_id = data.window_id;
                ctx.reopen_closed_window(*data);
                WindowSettings::apply_background_blur_to_all_windows(ctx);
                reopen_window_screens(window_id, ctx);

                // Make sure we update our session restoration state now that the
                // window has been reopened.
                ctx.dispatch_global_action("workspace:save_app", &());
            }
            ClosedItem::Tab {
                workspace,
                tab_index,
                data,
            } => {
                if let Some(workspace) = workspace.upgrade(ctx) {
                    workspace.update(ctx, |workspace, ctx| {
                        workspace.restore_closed_tab(tab_index, data, ctx);
                    });
                    ctx.windows()
                        .show_window_and_focus_app(workspace.window_id(ctx));
                }
                // Make sure we update our session restoration state now that the
                // tab has been reopened.
                ctx.dispatch_global_action("workspace:save_app", &());
            }
            ClosedItem::Pane { data } => {
                if let Some(pane_group) = data.pane_group.upgrade(ctx) {
                    let pane_id = data.pane_id;
                    let window_id = pane_group.window_id(ctx);
                    let pane_group_id = pane_group.id();
                    let restored = pane_group.update(ctx, |pane_group, ctx| {
                        pane_group.restore_closed_pane(pane_id, ctx)
                    });

                    if restored {
                        // Focus the window first
                        ctx.windows().show_window_and_focus_app(window_id);

                        // Now properly focus the restored pane by activating its tab and focusing the pane
                        if let Some(workspace) =
                            workspace_owning_pane_group(window_id, pane_group_id, ctx)
                        {
                            workspace.update(ctx, |workspace, ctx| {
                                let locator = crate::workspace::PaneViewLocator {
                                    pane_group_id,
                                    pane_id,
                                };
                                workspace.focus_pane(locator, ctx);
                            });
                        }

                        ctx.dispatch_global_action("workspace:save_app", &());
                    }
                }
            }
        }
    }

    /// Handles a change to the undo close settings.
    fn handle_settings_event(
        &mut self,
        event: &UndoCloseSettingsChangedEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        match event {
            UndoCloseSettingsChangedEvent::UndoCloseEnabled { .. } => {
                let settings = UndoCloseSettings::as_ref(ctx);
                if !*settings.enabled {
                    for undo_data in self.stack.drain(..) {
                        undo_data.closed_item.discard(ctx);
                    }
                }
            }
            UndoCloseSettingsChangedEvent::UndoCloseGracePeriod { .. } => {}
        }
    }

    /// Pushes a new item onto the stack.
    fn push_item(&mut self, closed_item: ClosedItem, ctx: &mut ModelContext<Self>) {
        let settings = UndoCloseSettings::as_ref(ctx);
        if !*settings.enabled {
            closed_item.discard(ctx);
            return;
        }

        let id = ItemId::new();
        let grace_period = *settings.grace_period;
        let task_handle = ctx.spawn_abortable(
            warpui::r#async::Timer::after(grace_period),
            move |me, _, ctx| {
                let initial_len = me.stack.len();
                if let Some(pos) = me.stack.iter().position(|item| item.expiry_data.id == id) {
                    let removed_item = me.stack.remove(pos);
                    removed_item.closed_item.discard(ctx);
                }
                // Log errors if the expired item was not found or multiple items were found
                if me.stack.len() == initial_len {
                    report_error!("Undo close expiry task did not find item in stack!");
                } else if me.stack.len() < initial_len - 1 {
                    report_error!("Undo close expiry task found multiple matching items in stack!");
                } else {
                    log::debug!("Removed expired item from undo stack");
                }
            },
            |_, _| {},
        );

        self.stack.push(UndoData {
            closed_item,
            expiry_data: ExpiryData { id, task_handle },
        })
    }
}

/// Find the root [`Workspace`] view for a window.
fn window_workspaces(window_id: WindowId, ctx: &mut AppContext) -> Vec<ViewHandle<Workspace>> {
    ctx.views_of_type::<Workspace>(window_id)
        .unwrap_or_default()
}

fn workspace_owning_pane_group(
    window_id: WindowId,
    pane_group_id: EntityId,
    ctx: &mut AppContext,
) -> Option<ViewHandle<Workspace>> {
    window_workspaces(window_id, ctx)
        .into_iter()
        .find(|workspace| {
            workspace
                .as_ref(ctx)
                .tab_views()
                .any(|pane_group| pane_group.id() == pane_group_id)
        })
}

fn reopen_window_screens(window_id: WindowId, ctx: &mut AppContext) {
    let host = ctx
        .root_view::<RootView>(window_id)
        .and_then(|root| root.as_ref(ctx).project_host_view().cloned());
    match host {
        Some(host) => host.update(ctx, |host, ctx| host.handle_reopen(ctx)),
        None => {
            for workspace in window_workspaces(window_id, ctx) {
                workspace.update(ctx, |workspace, ctx| workspace.handle_reopen(ctx));
            }
        }
    }
}

impl Entity for UndoCloseStack {
    type Event = UndoCloseStackEvent;
}

impl SingletonEntity for UndoCloseStack {}
