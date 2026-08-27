use pathfinder_color::ColorU;
use warp_core::ui::appearance::Appearance;
use warp_core::ui::theme::color::internal_colors;
use warp_core::ui::theme::{AnsiColorIdentifier, WarpTheme};

use crate::ui_components::blended_colors;
use crate::ui_components::icons::Icon;

pub fn todo_list_icon(appearance: &Appearance) -> warpui::elements::Icon {
    warpui::elements::Icon::new(
        Icon::BulletedListBlock.into(),
        blended_colors::neutral_7(appearance.theme()),
    )
}

pub fn pending_icon(appearance: &Appearance) -> warpui::elements::Icon {
    warpui::elements::Icon::new(
        Icon::Queued.into(),
        blended_colors::neutral_5(appearance.theme()),
    )
}

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

pub fn addressed_comment_icon(appearance: &Appearance) -> warpui::elements::Icon {
    warpui::elements::Icon::new(
        Icon::AddressedComment.into(),
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

/// Agent is waiting for user to follow-up with next prompt.
pub fn gray_clock_icon(appearance: &Appearance) -> warpui::elements::Icon {
    warpui::elements::Icon::new(
        Icon::ClockSnooze.into(),
        blended_colors::neutral_5(appearance.theme()),
    )
}

/// Loading but not actionable yet.
pub fn gray_circle_icon(appearance: &Appearance) -> warpui::elements::Icon {
    warpui::elements::Icon::new(
        Icon::Circle.into(),
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

/// To be used for actions (like running commands/reading files) that are long-running and executing.
pub fn yellow_running_icon(appearance: &Appearance) -> warpui::elements::Icon {
    warpui::elements::Icon::new(
        Icon::Circle.into(),
        AnsiColorIdentifier::Yellow.to_ansi_color(&appearance.theme().terminal_colors().normal),
    )
}

/// Used for buttons that stop the current task
pub fn red_stop_icon(appearance: &Appearance) -> warpui::elements::Icon {
    warpui::elements::Icon::new(Icon::StopFilled.into(), appearance.theme().ansi_fg_red())
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

    /// The last turn failed transiently and an automatic recovery (retry or resume)
    /// is pending. Non-terminal: returns to `InProgress` when the recovery request
    /// sends, or falls to `Error` if recovery is exhausted.
    TransientError,

    /// The last turn of the agent was cancelled by the user.
    Cancelled,

    /// The last turn of the agent resulted in an action whose execution is blocked by the user.
    Blocked { blocked_action: String },

    /// Agent yielded via wait_for_events and is listening for inbound
    /// input. Quiescent but not terminal.
    WaitingForEvents,
}

impl std::fmt::Display for ConversationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConversationStatus::InProgress => write!(f, "In progress"),
            ConversationStatus::Success => write!(f, "Done"),
            ConversationStatus::Error => write!(f, "Error"),
            ConversationStatus::TransientError => write!(f, "Reconnecting"),
            ConversationStatus::Cancelled => write!(f, "Cancelled"),
            ConversationStatus::Blocked { .. } => write!(f, "Blocked"),
            ConversationStatus::WaitingForEvents => write!(f, "Waiting"),
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
            // Recovery pending: keep the in-progress treatment rather than an error one.
            ConversationStatus::TransientError => in_progress_icon(appearance),
            ConversationStatus::Cancelled => gray_stop_icon(appearance),
            ConversationStatus::WaitingForEvents => in_progress_icon(appearance),
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
            ConversationStatus::TransientError => (
                Icon::ClockLoader,
                match color_style {
                    StatusColorStyle::Standard => theme.ansi_fg_yellow(),
                    StatusColorStyle::Cloud => theme.ansi_bg_yellow(),
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
            ConversationStatus::WaitingForEvents => (
                Icon::ClockLoader,
                match color_style {
                    StatusColorStyle::Standard => theme.ansi_fg_magenta(),
                    StatusColorStyle::Cloud => theme.ansi_bg_magenta(),
                },
            ),
        }
    }

    pub fn is_in_progress(&self) -> bool {
        matches!(self, ConversationStatus::InProgress)
    }

    /// True while a transient failure is being automatically recovered.
    pub fn is_transient_error(&self) -> bool {
        matches!(self, ConversationStatus::TransientError)
    }

    pub fn is_blocked(&self) -> bool {
        matches!(self, ConversationStatus::Blocked { .. })
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self, ConversationStatus::Cancelled)
    }

    /// True iff the run is finished and cannot resume on its own.
    pub fn is_done(&self) -> bool {
        matches!(
            self,
            ConversationStatus::Success | ConversationStatus::Error | ConversationStatus::Cancelled
        )
    }

    /// True iff the agent has yielded via `wait_for_events` and is listening
    /// for inbound input.
    pub fn is_waiting_for_events(&self) -> bool {
        matches!(self, ConversationStatus::WaitingForEvents)
    }

    pub fn is_error(&self) -> bool {
        matches!(self, ConversationStatus::Error)
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
