use warpui::{AppContext, ModelHandle, SingletonEntity, View, ViewContext, ViewHandle};

use super::PaneId;
use crate::agent_launcher::pane_manager::AgentPickerPaneManager;
use crate::app_state::LeafContents;
use crate::pane_group::pane::agent_picker_view::{AgentPickerView, AgentPickerViewEvent};
use crate::pane_group::pane::{ShareableLink, ShareableLinkError};
use crate::pane_group::{BackingView, PaneConfiguration, PaneContent, PaneGroup, PaneView};
use crate::workspace::PaneViewLocator;

pub struct AgentPickerPane {
    view: ViewHandle<PaneView<AgentPickerView>>,
    pane_configuration: ModelHandle<PaneConfiguration>,
}

impl AgentPickerPane {
    pub fn new<V: View>(ctx: &mut ViewContext<V>) -> Self {
        let agent_picker_view = ctx.add_typed_action_view(AgentPickerView::new);
        let pane_configuration = agent_picker_view.as_ref(ctx).pane_configuration();
        let pane_view = ctx.add_typed_action_view(|ctx| {
            let pane_id = PaneId::from_agent_picker_pane_ctx(ctx);
            PaneView::new(
                pane_id,
                agent_picker_view,
                (),
                pane_configuration.clone(),
                ctx,
            )
        });
        Self {
            view: pane_view,
            pane_configuration,
        }
    }
}

impl PaneContent for AgentPickerPane {
    fn id(&self) -> PaneId {
        PaneId::from_agent_picker_pane_view(&self.view)
    }

    fn attach(
        &self,
        _group: &PaneGroup,
        focus_handle: crate::pane_group::focus_state::PaneFocusHandle,
        ctx: &mut ViewContext<PaneGroup>,
    ) {
        self.view
            .update(ctx, |view, ctx| view.set_focus_handle(focus_handle, ctx));
        let child = self.view.as_ref(ctx).child(ctx);

        let pane_id = self.id();
        let pane_group_id = ctx.view_id();
        let screen_id = crate::workspace::owning_screen_id(pane_group_id, ctx.window_id(), ctx);
        ctx.subscribe_to_view(&child, move |pane_group, _, event, ctx| {
            let AgentPickerViewEvent::Pane(pane_event) = event;
            pane_group.handle_pane_event(pane_id, pane_event, ctx);
        });
        ctx.subscribe_to_view(&self.view, move |pane_group, _, event, ctx| {
            pane_group.handle_pane_view_event(pane_id, event, ctx);
        });

        if let Some(screen_id) = screen_id {
            AgentPickerPaneManager::handle(ctx).update(ctx, |manager, _ctx| {
                manager.register_pane(
                    screen_id,
                    PaneViewLocator {
                        pane_group_id,
                        pane_id,
                    },
                );
            });
        }
    }

    fn detach(
        &self,
        _group: &PaneGroup,
        _detach_type: super::DetachType,
        ctx: &mut ViewContext<PaneGroup>,
    ) {
        let child = self.view.as_ref(ctx).child(ctx);
        ctx.unsubscribe_to_view(&child);
        ctx.unsubscribe_to_view(&self.view);

        if let Some(screen_id) =
            crate::workspace::owning_screen_id(ctx.view_id(), ctx.window_id(), ctx)
        {
            AgentPickerPaneManager::handle(ctx).update(ctx, |manager, _ctx| {
                manager.deregister_pane(&screen_id);
            });
        }
    }

    fn snapshot(&self, _ctx: &AppContext) -> LeafContents {
        LeafContents::AgentPicker
    }

    fn has_application_focus(&self, ctx: &mut ViewContext<PaneGroup>) -> bool {
        self.view.is_self_or_child_focused(ctx)
    }

    fn focus(&self, ctx: &mut ViewContext<PaneGroup>) {
        self.view
            .as_ref(ctx)
            .child(ctx)
            .update(ctx, BackingView::focus_contents)
    }

    fn shareable_link(
        &self,
        _ctx: &mut ViewContext<PaneGroup>,
    ) -> Result<ShareableLink, ShareableLinkError> {
        Ok(ShareableLink::Base)
    }

    fn pane_configuration(&self) -> ModelHandle<PaneConfiguration> {
        self.pane_configuration.clone()
    }

    fn is_pane_being_dragged(&self, ctx: &AppContext) -> bool {
        self.view.as_ref(ctx).is_being_dragged()
    }
}
