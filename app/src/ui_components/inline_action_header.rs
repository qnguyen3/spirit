use std::borrow::Cow;

use pathfinder_color::ColorU;
use warp_core::ui::appearance::Appearance;
use warp_core::ui::theme::color::internal_colors;
use warpui::elements::{
    Border, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Expanded, Flex,
    FormattedTextElement, MainAxisAlignment, MainAxisSize, ParentElement, Radius, Shrinkable, Text,
};
use warpui::fonts::FamilyId;
use warpui::{AppContext, Element, SingletonEntity};

use crate::ui_components::blended_colors;
use crate::ui_components::inline_action_icons::icon_size;

/// Same padding constants as the original for consistency
pub const INLINE_ACTION_HORIZONTAL_PADDING: f32 = 16.;
pub const INLINE_ACTION_HEADER_VERTICAL_PADDING: f32 = 10.;
pub const ICON_MARGIN: f32 = 8.;

#[derive(Clone)]
pub struct HeaderConfig {
    pub title: Cow<'static, str>,
    pub font_family: FamilyId,
    /// Whether to parse the title as markdown when rendering.
    pub use_markdown: bool,
    pub icon: Option<warpui::elements::Icon>,
    pub badge: Option<String>,
    pub is_text_selectable: bool,
    pub font_color_override: Option<ColorU>,
    pub corner_radius_override: Option<CornerRadius>,
    pub soft_wrap_title: bool,
}

impl HeaderConfig {
    pub fn new(title: impl Into<Cow<'static, str>>, app: &AppContext) -> Self {
        Self {
            title: title.into(),
            font_family: Appearance::as_ref(app).ui_font_family(),
            use_markdown: false,
            icon: None,
            badge: None,
            is_text_selectable: false,
            font_color_override: None,
            corner_radius_override: None,
            soft_wrap_title: false,
        }
    }

    pub fn with_corner_radius_override(mut self, corner_radius: CornerRadius) -> Self {
        self.corner_radius_override = Some(corner_radius);
        self
    }

    pub fn render_header(
        self,
        app: &AppContext,
        interaction_mode_content: Option<Box<dyn Element>>,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let header_background = theme.surface_2();

        let mut header_row = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center);

        let mut left_content_container = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_cross_axis_alignment(CrossAxisAlignment::Center);

        if let Some(icon) = self.icon {
            left_content_container.add_child(
                Container::new(
                    ConstrainedBox::new(icon.finish())
                        .with_width(icon_size(app))
                        .with_height(icon_size(app))
                        .finish(),
                )
                .with_margin_right(ICON_MARGIN)
                .finish(),
            )
        }

        let text_color = self
            .font_color_override
            .unwrap_or_else(|| blended_colors::text_main(appearance.theme(), header_background));

        let mut title_element = Text::new_inline(
            self.title.clone(),
            self.font_family,
            appearance.monospace_font_size(),
        )
        .soft_wrap(self.soft_wrap_title)
        .with_selectable(self.is_text_selectable)
        .with_color(text_color)
        .finish();

        if self.use_markdown
            && let Ok(formatted_text) = markdown_parser::parse_markdown(&self.title)
        {
            let mut element = FormattedTextElement::new(
                formatted_text,
                appearance.monospace_font_size(),
                self.font_family,
                appearance.monospace_font_family(),
                text_color,
                Default::default(),
            )
            .set_selectable(self.is_text_selectable);
            if !self.soft_wrap_title {
                element = element.with_no_text_wrapping();
            }
            title_element = element.finish();
        }

        left_content_container.add_child(
            Expanded::new(
                1.,
                Container::new(title_element).with_margin_right(8.).finish(),
            )
            .finish(),
        );

        if let Some(badge) = self.badge {
            left_content_container.add_child(
                Container::new(
                    Text::new(
                        badge,
                        appearance.ui_font_family(),
                        appearance.ui_font_size(),
                    )
                    .with_color(internal_colors::fg_overlay_5(theme).into())
                    .finish(),
                )
                .with_horizontal_padding(8.)
                .with_vertical_padding(4.)
                .with_margin_right(8.)
                .with_border(Border::all(1.0).with_border_fill(theme.outline()))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(2.)))
                .with_background(theme.surface_1())
                .finish(),
            );
        }

        header_row.add_child(Shrinkable::new(1., left_content_container.finish()).finish());

        if let Some(interaction_mode_content) = interaction_mode_content {
            header_row.add_child(interaction_mode_content);
        }

        Container::new(header_row.finish())
            .with_padding_left(INLINE_ACTION_HORIZONTAL_PADDING)
            .with_padding_right(INLINE_ACTION_HORIZONTAL_PADDING)
            .with_vertical_padding(INLINE_ACTION_HEADER_VERTICAL_PADDING)
            .with_background(header_background)
            .with_corner_radius(
                self.corner_radius_override
                    .unwrap_or_else(|| CornerRadius::with_all(Radius::Pixels(8.))),
            )
            .finish()
    }
}
