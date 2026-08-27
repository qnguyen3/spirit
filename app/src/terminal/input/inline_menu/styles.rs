//! Shared styling utilities for inline menu implementations.
//!
//! This module provides common styling functions used across all inline menu
//! implementations (models, slash commands, conversations) to ensure consistent
//! visual design matching the Figma specifications.
use warp_core::ui::appearance::Appearance;
use warp_core::ui::theme::{Fill, WarpTheme};
use warpui::color::ColorU;
use warpui::{AppContext, Element, SingletonEntity};

use crate::search::result_renderer::ItemHighlightState;

/// Font size used for inline menu items.
pub fn font_size(appearance: &Appearance) -> f32 {
    appearance.monospace_font_size() - 2.
}

pub const ICON_MARGIN: f32 = 8.0;
pub const ITEM_HORIZONTAL_PADDING: f32 = 8.0;
pub const ITEM_CORNER_RADIUS: f32 = 4.0;
pub const CONTENT_VERTICAL_PADDING: f32 = 8.;
pub const CONTENT_BORDER_WIDTH: f32 = 1.;

/// Height of the header row content, ensuring all headers render at the same
/// height regardless of whether tabs or trailing elements are present.
pub const HEADER_ROW_HEIGHT: f32 = 24.;
pub const HEADER_BORDER: f32 = 1.;

pub fn menu_background_color(app: &AppContext) -> ColorU {
    let appearance = Appearance::as_ref(app);
    let theme = appearance.theme();
    theme.background().into_solid()
}

pub fn item_background(
    highlight_state: ItemHighlightState,
    appearance: &Appearance,
) -> Option<Fill> {
    let theme = appearance.theme();
    match highlight_state {
        ItemHighlightState::Selected { .. } => Some(theme.surface_overlay_2()),
        ItemHighlightState::Hovered => Some(theme.surface_overlay_1()),
        ItemHighlightState::Default => None,
    }
}

pub fn primary_text_color(theme: &WarpTheme, background: Fill) -> Fill {
    theme.main_text_color(background)
}

pub fn secondary_text_color(theme: &WarpTheme, background: Fill) -> Fill {
    theme.sub_text_color(background)
}

pub fn disabled_text_color(theme: &WarpTheme, background: Fill) -> Fill {
    theme.disabled_text_color(background)
}

pub fn icon_color(appearance: &Appearance) -> Fill {
    let theme = appearance.theme();
    theme.sub_text_color(theme.background()).with_opacity(80)
}

pub fn keystroke_size(appearance: &Appearance) -> f32 {
    font_size(appearance) + 2.
}

pub fn render_keystroke_with_color_overrides(
    keystroke: &warpui::keymap::Keystroke,
    color: Option<ColorU>,
    background_color: Option<ColorU>,
    app: &AppContext,
) -> Box<dyn Element> {
    use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};

    use crate::ui_components::blended_colors;

    let appearance = Appearance::as_ref(app);
    let theme = appearance.theme();
    let font_size = font_size(appearance);
    appearance
        .ui_builder()
        .keyboard_shortcut(keystroke)
        .lowercase_modifier()
        .with_space_between_keys(2.)
        .with_style(UiComponentStyles {
            margin: Some(Coords::default()),
            padding: Some(Coords::default()),
            border_width: Some(1.),
            background: Some(
                background_color
                    .unwrap_or_else(|| blended_colors::neutral_3(theme))
                    .into(),
            ),
            font_color: Some(color.unwrap_or_else(|| theme.foreground().into_solid())),
            font_family_id: Some(appearance.ui_font_family()),
            font_size: Some(font_size),
            width: Some(keystroke_size(appearance)),
            height: Some(keystroke_size(appearance)),
            ..Default::default()
        })
        .with_line_height_ratio(1.0)
        .build()
        .finish()
}
