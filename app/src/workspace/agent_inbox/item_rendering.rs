use warpui::elements::{
    ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Element, Empty, Flex,
    MainAxisAlignment, MainAxisSize, ParentElement, Radius, Shrinkable,
};
use warpui::fonts::Weight;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};

use super::item::InboxItem;
use crate::appearance::Appearance;
use crate::terminal::cli_agent_sessions::signal::AgentSignal;
use crate::ui_components::icon_with_status::{IconWithStatusVariant, render_icon_with_status};
use crate::ui_components::status_icons::ConversationStatus;
use crate::util::time_format::format_elapsed_since;

const AGENT_ICON_SIZE: f32 = 24.;
const UNREAD_DOT_SIZE: f32 = 6.;

fn conversation_status(outcome: AgentSignal) -> ConversationStatus {
    match outcome {
        AgentSignal::Working => ConversationStatus::InProgress,
        AgentSignal::Done => ConversationStatus::Success,
        AgentSignal::Failed => ConversationStatus::Error,
        AgentSignal::NeedsInput => ConversationStatus::Blocked {
            blocked_action: String::new(),
        },
    }
}

pub fn render_item_content(item: &InboxItem, appearance: &Appearance) -> Box<dyn Element> {
    let theme = appearance.theme();
    let surface = theme.surface_2();

    let agent_icon = render_icon_with_status(
        IconWithStatusVariant::CLIAgent {
            agent: item.agent,
            status: Some(conversation_status(item.outcome)),
            is_ambient: false,
        },
        AGENT_ICON_SIZE,
        0.,
        theme,
        surface,
    );

    let workspace_label = appearance
        .ui_builder()
        .wrappable_text(item.workspace_name.clone(), false)
        .with_style(UiComponentStyles {
            font_size: Some(11.),
            font_weight: Some(Weight::Semibold),
            font_color: Some(theme.sub_text_color(surface).into()),
            font_family_id: Some(appearance.ui_font_family()),
            ..Default::default()
        })
        .build()
        .finish();

    let elapsed = appearance
        .ui_builder()
        .wrappable_text(format_elapsed_since(item.created_at), false)
        .with_style(UiComponentStyles {
            font_size: Some(11.),
            font_color: Some(theme.sub_text_color(surface).into()),
            font_family_id: Some(appearance.ui_font_family()),
            ..Default::default()
        })
        .build()
        .finish();

    let meta_row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .with_main_axis_size(MainAxisSize::Max)
        .with_child(Shrinkable::new(1.0, workspace_label).finish())
        .with_child(elapsed)
        .finish();

    let message = appearance
        .ui_builder()
        .wrappable_text(item.message(), true)
        .with_style(UiComponentStyles {
            font_size: Some(12.),
            font_color: Some(theme.main_text_color(surface).into()),
            font_family_id: Some(appearance.ui_font_family()),
            ..Default::default()
        })
        .build()
        .finish();

    let text_column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_main_axis_size(MainAxisSize::Min)
        .with_child(meta_row)
        .with_child(Container::new(message).with_margin_top(2.).finish())
        .finish();

    let mut row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_main_axis_size(MainAxisSize::Max)
        .with_child(
            Container::new(agent_icon)
                .with_margin_right(10.)
                .with_margin_top(1.)
                .finish(),
        )
        .with_child(Shrinkable::new(1.0, text_column).finish());

    let unread_dot = Container::new(
        ConstrainedBox::new(Empty::new().finish())
            .with_width(UNREAD_DOT_SIZE)
            .with_height(UNREAD_DOT_SIZE)
            .finish(),
    )
    .with_corner_radius(CornerRadius::with_all(Radius::Percentage(50.)))
    .with_margin_left(8.)
    .with_margin_top(6.);

    row.add_child(if item.is_read {
        unread_dot.finish()
    } else {
        unread_dot.with_background(theme.accent()).finish()
    });

    row.finish()
}
