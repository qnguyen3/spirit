use warp_core::ui::theme::color::internal_colors;
use warpui::elements::new_scrollable::{ScrollableAppearance, SingleAxisConfig};
use warpui::elements::{
    Border, ChildView, ClippedScrollStateHandle, ConstrainedBox, Container, CornerRadius,
    CrossAxisAlignment, Dismiss, DispatchEventResult, Element, EventHandler, Fill as ElementFill,
    Flex, Hoverable, MainAxisAlignment, MainAxisSize, MouseStateHandle, NewScrollable, Padding,
    ParentElement, Radius, SavePosition, ScrollTarget, ScrollToPositionMode, ScrollbarWidth,
    Shrinkable,
};
use warpui::fonts::Weight;
use warpui::keymap::FixedBinding;
use warpui::keymap::macros::id;
use warpui::platform::Cursor;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::{
    AppContext, Entity, EntityId, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle,
    WindowId,
};

use super::item::{InboxFilter, InboxItem, InboxItemId, InboxItems};
use super::item_rendering::render_item_content;
use super::model::{AgentInboxModel, AgentInboxModelEvent};
use crate::appearance::Appearance;
use crate::pane_group::PaneId;
use crate::projects::ProjectId;
use crate::ui_components::icons::Icon;
use crate::view_components::action_button::{ActionButton, ButtonSize, NakedTheme};

const POPUP_WIDTH: f32 = 420.;
const POPUP_MAX_HEIGHT: f32 = 500.;
const ITEM_POSITION_PREFIX: &str = "agent_inbox_item_";

pub struct AgentInboxView {
    active_filter: InboxFilter,
    scroll_state: ClippedScrollStateHandle,
    filter_mouse_states: Vec<MouseStateHandle>,
    item_mouse_states: Vec<MouseStateHandle>,
    close_button: ViewHandle<ActionButton>,
    mark_all_read_button: ViewHandle<ActionButton>,
    filtered_ids: Vec<InboxItemId>,
    selected_index: Option<usize>,
}

#[derive(Debug, Clone)]
pub enum AgentInboxViewEvent {
    NavigateTo {
        window_id: WindowId,
        project_id: Option<ProjectId>,
        pane_group_id: EntityId,
        pane_id: PaneId,
    },
    Dismissed,
}

#[derive(Debug, Clone)]
pub enum AgentInboxViewAction {
    SetFilter(InboxFilter),
    MarkAllRead,
    ClickItem(InboxItemId),
    Dismiss,
    SelectPrevious,
    SelectNext,
    ActivateSelected,
}

impl Entity for AgentInboxView {
    type Event = AgentInboxViewEvent;
}

impl TypedActionView for AgentInboxView {
    type Action = AgentInboxViewAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            AgentInboxViewAction::SetFilter(filter) => self.set_active_filter(*filter, ctx),
            AgentInboxViewAction::MarkAllRead => {
                AgentInboxModel::handle(ctx).update(ctx, |model, ctx| model.mark_all_read(ctx));
            }
            AgentInboxViewAction::ClickItem(id) => self.activate(*id, ctx),
            AgentInboxViewAction::Dismiss => ctx.emit(AgentInboxViewEvent::Dismissed),
            AgentInboxViewAction::SelectPrevious => {
                match self.selected_index {
                    Some(index) if index > 0 => self.selected_index = Some(index - 1),
                    Some(_) => {}
                    None if !self.filtered_ids.is_empty() => self.selected_index = Some(0),
                    None => {}
                }
                self.scroll_selected_into_view();
                ctx.notify();
            }
            AgentInboxViewAction::SelectNext => {
                let last = self.filtered_ids.len().saturating_sub(1);
                match self.selected_index {
                    Some(index) if index < last => self.selected_index = Some(index + 1),
                    Some(_) => {}
                    None if !self.filtered_ids.is_empty() => self.selected_index = Some(0),
                    None => {}
                }
                self.scroll_selected_into_view();
                ctx.notify();
            }
            AgentInboxViewAction::ActivateSelected => {
                if let Some(index) = self.selected_index
                    && let Some(id) = self.filtered_ids.get(index).copied()
                {
                    self.activate(id, ctx);
                }
            }
        }
    }
}

impl AgentInboxView {
    pub fn init(app: &mut AppContext) {
        app.register_fixed_bindings([
            FixedBinding::new(
                "up",
                AgentInboxViewAction::SelectPrevious,
                id!(AgentInboxView::ui_name()),
            ),
            FixedBinding::new(
                "down",
                AgentInboxViewAction::SelectNext,
                id!(AgentInboxView::ui_name()),
            ),
            FixedBinding::new(
                "enter",
                AgentInboxViewAction::ActivateSelected,
                id!(AgentInboxView::ui_name()),
            ),
            FixedBinding::new(
                "escape",
                AgentInboxViewAction::Dismiss,
                id!(AgentInboxView::ui_name()),
            ),
        ]);
    }

    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let model = AgentInboxModel::handle(ctx);
        ctx.subscribe_to_model(&model, |me, _, event, ctx| match event {
            AgentInboxModelEvent::Changed => {
                me.rebuild_filtered_ids(ctx);
                ctx.notify();
            }
        });

        let close_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("", NakedTheme)
                .with_icon(Icon::X)
                .with_size(ButtonSize::XSmall)
                .with_tooltip("Close")
                .with_tooltip_sublabel("Esc")
                .on_click(|ctx| ctx.dispatch_typed_action(AgentInboxViewAction::Dismiss))
        });

        let mark_all_read_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("Mark all as read", NakedTheme)
                .with_size(ButtonSize::Small)
                .on_click(|ctx| ctx.dispatch_typed_action(AgentInboxViewAction::MarkAllRead))
        });

        Self {
            active_filter: InboxFilter::AllWorkspaces,
            scroll_state: Default::default(),
            filter_mouse_states: Vec::new(),
            item_mouse_states: Vec::new(),
            close_button,
            mark_all_read_button,
            filtered_ids: Vec::new(),
            selected_index: None,
        }
    }

    pub fn reset_for_open(&mut self, ctx: &mut ViewContext<Self>) {
        self.rebuild_filtered_ids(ctx);
        self.selected_index = None;
    }

    fn set_active_filter(&mut self, filter: InboxFilter, ctx: &mut ViewContext<Self>) {
        self.active_filter = filter;
        self.selected_index = None;
        self.rebuild_filtered_ids(ctx);
        ctx.notify();
    }

    /// A workspace drops out of the filter list as its items age out, stranding
    /// the popup on a filter that can never match again.
    fn rebuild_filtered_ids(&mut self, ctx: &mut ViewContext<Self>) {
        let items = AgentInboxModel::as_ref(ctx).items();
        let visible = items.visible_filters();
        if !visible.contains(&self.active_filter) {
            self.active_filter = InboxFilter::AllWorkspaces;
        }
        self.filter_mouse_states
            .resize_with(visible.len(), MouseStateHandle::default);
        self.filtered_ids = items.ids_matching(self.active_filter);
        self.item_mouse_states
            .resize_with(self.filtered_ids.len(), MouseStateHandle::default);
        self.selected_index = self
            .selected_index
            .filter(|index| *index < self.filtered_ids.len());
    }

    fn scroll_selected_into_view(&self) {
        if let Some(index) = self.selected_index {
            self.scroll_state.scroll_to_position(ScrollTarget {
                position_id: format!("{ITEM_POSITION_PREFIX}{index}"),
                mode: ScrollToPositionMode::FullyIntoView,
            });
        }
    }

    fn activate(&mut self, id: InboxItemId, ctx: &mut ViewContext<Self>) {
        let Some(target) = AgentInboxModel::as_ref(ctx).items().get(id).map(|item| {
            AgentInboxViewEvent::NavigateTo {
                window_id: item.window_id,
                project_id: item.project_id,
                pane_group_id: item.pane_group_id,
                pane_id: item.pane_id,
            }
        }) else {
            return;
        };
        AgentInboxModel::handle(ctx).update(ctx, |model, ctx| model.mark_read(id, ctx));
        ctx.emit(target);
    }

    fn render_header(&self, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        let label = appearance
            .ui_builder()
            .wrappable_text("Notifications".to_owned(), false)
            .with_style(UiComponentStyles {
                font_size: Some(14.),
                font_weight: Some(Weight::Semibold),
                font_color: Some(theme.main_text_color(theme.surface_2()).into()),
                font_family_id: Some(appearance.ui_font_family()),
                ..Default::default()
            })
            .build()
            .finish();

        Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_main_axis_size(MainAxisSize::Max)
                .with_child(label)
                .with_child(ChildView::new(&self.close_button).finish())
                .finish(),
        )
        .with_padding(
            Padding::default()
                .with_top(8.)
                .with_bottom(4.)
                .with_horizontal(12.),
        )
        .finish()
    }

    fn render_filter_bar(&self, items: &InboxItems, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        let mut filter_buttons = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_size(MainAxisSize::Min)
            .with_spacing(2.);

        for (index, filter) in items.visible_filters().into_iter().enumerate() {
            let Some(mouse_state) = self.filter_mouse_states.get(index).cloned() else {
                continue;
            };
            let is_active = self.active_filter == filter;
            let count = items.count(filter);
            let label = format!("{} ({count})", items.filter_label(filter));
            let text_color = if is_active {
                theme.main_text_color(theme.surface_2())
            } else {
                theme.sub_text_color(theme.surface_2())
            };

            filter_buttons.add_child(
                EventHandler::new(
                    Hoverable::new(mouse_state, move |state| {
                        let background = if is_active {
                            Some(internal_colors::fg_overlay_3(theme))
                        } else if state.is_hovered() {
                            Some(internal_colors::fg_overlay_2(theme))
                        } else {
                            None
                        };
                        let mut container = Container::new(
                            appearance
                                .ui_builder()
                                .wrappable_text(label, false)
                                .with_style(UiComponentStyles {
                                    font_size: Some(12.),
                                    font_weight: Some(Weight::Semibold),
                                    font_color: Some(text_color.into()),
                                    font_family_id: Some(appearance.ui_font_family()),
                                    ..Default::default()
                                })
                                .build()
                                .finish(),
                        )
                        .with_padding(Padding::default().with_vertical(4.).with_horizontal(8.))
                        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));
                        if let Some(background) = background {
                            container = container.with_background_color(background.into());
                        }
                        container.finish()
                    })
                    .with_cursor(Cursor::PointingHand)
                    .finish(),
                )
                .on_left_mouse_down(move |ctx, _, _| {
                    ctx.dispatch_typed_action(AgentInboxViewAction::SetFilter(filter));
                    DispatchEventResult::StopPropagation
                })
                .finish(),
            );
        }

        let mut row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_main_axis_size(MainAxisSize::Max)
            .with_child(Shrinkable::new(1.0, filter_buttons.finish()).finish());

        if items.unread_count() > 0 {
            row.add_child(ChildView::new(&self.mark_all_read_button).finish());
        }

        Container::new(row.finish())
            .with_padding(
                Padding::default()
                    .with_vertical(8.)
                    .with_left(12.)
                    .with_right(6.),
            )
            .with_border(Border::top(1.).with_border_color(theme.outline().into()))
            .with_border(Border::bottom(1.).with_border_color(theme.outline().into()))
            .finish()
    }

    fn render_empty_state(&self, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        Container::new(
            appearance
                .ui_builder()
                .wrappable_text("No agent notifications yet.".to_owned(), false)
                .with_style(UiComponentStyles {
                    font_size: Some(12.),
                    font_color: Some(theme.sub_text_color(theme.surface_2()).into()),
                    font_family_id: Some(appearance.ui_font_family()),
                    ..Default::default()
                })
                .build()
                .finish(),
        )
        .with_padding(Padding::default().with_vertical(28.).with_horizontal(12.))
        .finish()
    }

    fn render_item(
        &self,
        item: &InboxItem,
        mouse_state: MouseStateHandle,
        is_selected: bool,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let id = item.id;
        let content = render_item_content(item, appearance);

        EventHandler::new(
            Hoverable::new(mouse_state, move |state| {
                let mut container = Container::new(content)
                    .with_padding(Padding::default().with_vertical(10.).with_horizontal(12.));
                if is_selected {
                    container = container
                        .with_background_color(internal_colors::fg_overlay_3(theme).into());
                } else if state.is_hovered() {
                    container = container
                        .with_background_color(internal_colors::fg_overlay_2(theme).into());
                }
                container.finish()
            })
            .with_cursor(Cursor::PointingHand)
            .finish(),
        )
        .on_left_mouse_down(move |ctx, _, _| {
            ctx.dispatch_typed_action(AgentInboxViewAction::ClickItem(id));
            DispatchEventResult::StopPropagation
        })
        .finish()
    }
}

impl View for AgentInboxView {
    fn ui_name() -> &'static str {
        "AgentInboxView"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let items = AgentInboxModel::as_ref(app).items();

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_main_axis_size(MainAxisSize::Min)
            .with_child(self.render_header(appearance))
            .with_child(self.render_filter_bar(items, appearance));

        if self.filtered_ids.is_empty() {
            column.add_child(self.render_empty_state(appearance));
        } else {
            let mut rows = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for (index, id) in self.filtered_ids.iter().enumerate() {
                let (Some(item), Some(mouse_state)) =
                    (items.get(*id), self.item_mouse_states.get(index).cloned())
                else {
                    continue;
                };
                rows.add_child(
                    SavePosition::new(
                        self.render_item(
                            item,
                            mouse_state,
                            self.selected_index == Some(index),
                            appearance,
                        ),
                        &format!("{ITEM_POSITION_PREFIX}{index}"),
                    )
                    .finish(),
                );
            }

            let list = NewScrollable::vertical(
                SingleAxisConfig::Clipped {
                    handle: self.scroll_state.clone(),
                    child: rows.finish(),
                },
                theme.nonactive_ui_detail().into(),
                theme.active_ui_detail().into(),
                ElementFill::None,
            )
            .with_vertical_scrollbar(ScrollableAppearance::new(ScrollbarWidth::Auto, true))
            .finish();
            column.add_child(Shrinkable::new(1.0, list).finish());
        }

        let popup = EventHandler::new(
            Container::new(column.finish())
                .with_padding(Padding::default().with_top(4.))
                .with_background(theme.surface_2())
                .with_border(Border::all(1.).with_border_color(theme.outline().into()))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
                .finish(),
        )
        .on_left_mouse_down(|_, _, _| DispatchEventResult::StopPropagation)
        .finish();

        Dismiss::new(
            ConstrainedBox::new(popup)
                .with_width(POPUP_WIDTH)
                .with_max_height(POPUP_MAX_HEIGHT)
                .finish(),
        )
        .on_dismiss(|ctx, _| ctx.dispatch_typed_action(AgentInboxViewAction::Dismiss))
        .prevent_interaction_with_other_elements()
        .finish()
    }
}
