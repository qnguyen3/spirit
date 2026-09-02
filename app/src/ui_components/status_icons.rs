use pathfinder_color::ColorU;
use warp_core::ui::appearance::Appearance;
use warp_core::ui::theme::color::internal_colors;
use warp_core::ui::theme::{AnsiColorIdentifier, WarpTheme};

use crate::ui_components::blended_colors;
use crate::ui_components::icons::Icon;

pub fn in_progress_icon(appearance: &Appearance) -> warpui::elements::Icon {
    warpui::elements::Icon::new(
        Icon::Circle.into(),
        AnsiColorIdentifier::Magenta.to_ansi_color(&appearance.theme().terminal_colors().normal),
    )
}

pub fn succeeded_icon(appearance: &Appearance) -> warpui::elements::Icon {
    warpui::elements::Icon::new(
        Icon::Check.into(),
        AnsiColorIdentifier::Green.to_ansi_color(&appearance.theme().terminal_colors().normal),
    )
}

pub fn failed_icon(appearance: &Appearance) -> warpui::elements::Icon {
    warpui::elements::Icon::new(
        Icon::Triangle.into(),
        AnsiColorIdentifier::Red.to_ansi_color(&appearance.theme().terminal_colors().normal),
    )
}

/// Not running, does not need user's attention
pub fn gray_stop_icon(appearance: &Appearance) -> warpui::elements::Icon {
    warpui::elements::Icon::new(
        Icon::StopFilled.into(),
        blended_colors::neutral_5(appearance.theme()),
    )
}

/// Not running, requires user's attention
pub fn yellow_stop_icon(appearance: &Appearance) -> warpui::elements::Icon {
    warpui::elements::Icon::new(
        Icon::StopFilled.into(),
        AnsiColorIdentifier::Yellow.to_ansi_color(&appearance.theme().terminal_colors().normal),
    )
}

#[derive(Clone, Copy)]
pub enum StatusColorStyle {
    /// Foreground-blend colors (`ansi_fg`) used by the regular status badge.
    Standard,
    /// Background-blend colors (`ansi_bg`) used by the cloud overlay badge.
    Cloud,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConversationStatus {
    /// Agent is running.
    InProgress,

    /// The last turn of the agent finished with success.
    Success,

    /// The last turn of the agent completed with error.
    Error,

    /// The last turn of the agent was cancelled by the user.
    Cancelled,

    /// The last turn of the agent resulted in an action whose execution is blocked by the user.
    Blocked { blocked_action: String },
}

impl std::fmt::Display for ConversationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConversationStatus::InProgress => write!(f, "In progress"),
            ConversationStatus::Success => write!(f, "Done"),
            ConversationStatus::Error => write!(f, "Error"),
            ConversationStatus::Cancelled => write!(f, "Cancelled"),
            ConversationStatus::Blocked { .. } => write!(f, "Blocked"),
        }
    }
}

impl ConversationStatus {
    pub fn render_icon(&self, appearance: &Appearance) -> warpui::elements::Icon {
        match self {
            ConversationStatus::InProgress => in_progress_icon(appearance),
            ConversationStatus::Success => succeeded_icon(appearance),
            ConversationStatus::Blocked { .. } => yellow_stop_icon(appearance),
            ConversationStatus::Error => failed_icon(appearance),
            ConversationStatus::Cancelled => gray_stop_icon(appearance),
        }
    }

    pub fn status_icon_and_color(
        &self,
        theme: &WarpTheme,
        color_style: StatusColorStyle,
    ) -> (Icon, ColorU) {
        match self {
            ConversationStatus::InProgress => (
                Icon::ClockLoader,
                match color_style {
                    StatusColorStyle::Standard => theme.ansi_fg_magenta(),
                    StatusColorStyle::Cloud => theme.ansi_bg_magenta(),
                },
            ),
            ConversationStatus::Success => (
                Icon::Check,
                match color_style {
                    StatusColorStyle::Standard => theme.ansi_fg_green(),
                    StatusColorStyle::Cloud => theme.ansi_bg_green(),
                },
            ),
            ConversationStatus::Error => (
                Icon::Triangle,
                match color_style {
                    StatusColorStyle::Standard => theme.ansi_fg_red(),
                    StatusColorStyle::Cloud => theme.ansi_bg_red(),
                },
            ),
            ConversationStatus::Cancelled => (Icon::StopFilled, internal_colors::neutral_5(theme)),
            ConversationStatus::Blocked { .. } => (
                Icon::StopFilled,
                match color_style {
                    StatusColorStyle::Standard => theme.ansi_fg_yellow(),
                    StatusColorStyle::Cloud => theme.ansi_bg_yellow(),
                },
            ),
        }
    }
}

/// Padding around the status icon
pub const STATUS_ELEMENT_PADDING: f32 = 2.;

/// Render the status element used by agent and conversation views.
pub fn render_status_element(
    status: &ConversationStatus,
    icon_size: f32,
    appearance: &Appearance,
) -> Box<dyn warpui::Element> {
    use warp_core::ui::color::coloru_with_opacity;
    use warp_core::ui::theme::Fill;
    use warpui::Element;
    use warpui::elements::{ConstrainedBox, Container, CornerRadius, Radius};

    let theme = appearance.theme();
    let (icon, color) = status.status_icon_and_color(theme, StatusColorStyle::Standard);

    Container::new(
        ConstrainedBox::new(icon.to_warpui_icon(Fill::from(color)).finish())
            .with_width(icon_size)
            .with_height(icon_size)
            .finish(),
    )
    .with_uniform_padding(STATUS_ELEMENT_PADDING)
    .with_background(coloru_with_opacity(color, 10))
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
    .finish()
}
