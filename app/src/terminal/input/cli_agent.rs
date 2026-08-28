use warp_core::ui::color::ContrastingColor;
use warp_core::ui::color::contrast::MinimumAllowedContrast;
use warp_core::ui::theme::color::internal_colors;
use warpui::elements::{
    Border, Clipped, ConstrainedBox, Container, CrossAxisAlignment, DispatchEventResult,
    DropTarget, Element, Empty, EventHandler, Flex, Hoverable, ParentElement, SavePosition, Stack,
};
use warpui::event::KeyState;
use warpui::presenter::ChildView;
use warpui::{AppContext, SingletonEntity as _, ViewContext};

use super::common::{
    add_input_suggestions_overlays, wrap_input_with_terminal_padding_and_focus_handler,
};
use super::{
    CLI_AGENT_RICH_INPUT_EDITOR_BOTTOM_PADDING, CLI_AGENT_RICH_INPUT_EDITOR_MAX_HEIGHT,
    CLI_AGENT_RICH_INPUT_EDITOR_TOP_PADDING, Input, InputAction, InputDropTargetData,
    TERMINAL_VIEW_PADDING_LEFT, voice_input,
};
use crate::appearance::Appearance;
use crate::editor::TextColors;
use crate::features::FeatureFlag;
use crate::terminal::cli_agent_sessions::CLIAgentSessionsModel;
use crate::terminal::should_right_click_paste;
use crate::terminal::view::TerminalAction;

impl Input {
    /// Renders the toolbar row directly beneath the editor: the CLI agent footer while an
    /// agent is running in this pane, and the standalone voice input button otherwise.
    pub(super) fn render_input_toolbar(&self, app: &AppContext) -> Box<dyn Element> {
        if self.cli_agent_footer.as_ref(app).cli_agent(app).is_some() {
            return ChildView::new(&self.cli_agent_footer).finish();
        }

        if !FeatureFlag::VoiceInput.is_enabled() {
            return Empty::new().finish();
        }

        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(ChildView::new(&self.voice_input_button).finish())
            .finish()
    }

    /// Renders the CLI agent rich input: a prompt composer plus the CLI agent footer.
    pub(super) fn render_cli_agent_input(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let menu_positioning = self.menu_positioning(app);

        let mut stack = Stack::new().with_constrain_absolute_children();

        let input_box = Container::new(
            ConstrainedBox::new(Clipped::new(ChildView::new(&self.editor).finish()).finish())
                .with_max_height(CLI_AGENT_RICH_INPUT_EDITOR_MAX_HEIGHT)
                .finish(),
        )
        .with_padding_top(CLI_AGENT_RICH_INPUT_EDITOR_TOP_PADDING)
        .with_padding_right(*TERMINAL_VIEW_PADDING_LEFT)
        .with_padding_bottom(CLI_AGENT_RICH_INPUT_EDITOR_BOTTOM_PADDING)
        .finish();

        let input_editor_save_position_id = self.editor_save_position_id();
        let editor_element = SavePosition::new(
            EventHandler::new(input_box)
                .on_right_mouse_down(move |ctx, app, position, modifiers| {
                    if should_right_click_paste(modifiers.shift, app) {
                        ctx.dispatch_typed_action(TerminalAction::Paste);
                        return DispatchEventResult::StopPropagation;
                    }
                    let input_rect = ctx
                        .element_position_by_id(input_editor_save_position_id.clone())
                        .expect("input editor position id should be saved");
                    let offset_position = position - input_rect.origin();
                    ctx.dispatch_typed_action(TerminalAction::OpenInputContextMenu {
                        position: offset_position,
                    });
                    DispatchEventResult::StopPropagation
                })
                .finish(),
            &self.editor_save_position_id(),
        )
        .finish();

        let mut column = Flex::column();
        column.add_child(editor_element);
        column.add_child(
            SavePosition::new(
                Container::new(ChildView::new(&self.cli_agent_footer).finish())
                    .with_padding_right(*TERMINAL_VIEW_PADDING_LEFT)
                    .finish(),
                &self.prompt_save_position_id(),
            )
            .finish(),
        );

        stack.add_child(wrap_input_with_terminal_padding_and_focus_handler(
            self.is_active_session(app),
            column.finish(),
            false,
        ));

        if self.is_pane_focused(app) {
            add_input_suggestions_overlays(self, &mut stack, appearance, menu_positioning, app);
        }

        let mut input_container = Container::new(stack.finish()).with_border(
            Border::top(1.0).with_border_fill(internal_colors::fg_overlay_2(appearance.theme())),
        );

        {
            let terminal_model = self.model.lock();
            if terminal_model.is_alt_screen_active()
                && let Some(bg_color) = terminal_model.alt_screen().inferred_bg_color()
            {
                input_container = input_container.with_background(bg_color);
            }
        }

        let drop_target = DropTarget::new(
            input_container.finish(),
            InputDropTargetData::new(self.weak_view_handle.clone()),
        )
        .finish();

        let input = SavePosition::new(
            Hoverable::new(self.hoverable_handle.clone(), |_| drop_target)
                .on_middle_click(|ctx, _app, _position| {
                    ctx.dispatch_typed_action(TerminalAction::MiddleClickOnInput)
                })
                .finish(),
            &self.status_free_input_save_position_id(),
        )
        .finish();

        let mut outer_column = Flex::column();
        if self.suggestions_mode_model.as_ref(app).is_slash_commands() {
            outer_column.add_child(ChildView::new(&self.inline_slash_commands_view).finish());
        }
        outer_column.add_child(input);

        let composer = SavePosition::new(outer_column.finish(), &self.save_position_id()).finish();
        if !FeatureFlag::VoiceInput.is_enabled() {
            return composer;
        }

        let is_focused = self.is_pane_focused(app);
        let hold_key = voice_input::hold_key();
        EventHandler::new(composer)
            .on_modifier_state_changed(move |ctx, _, key_code, key_state| {
                let released = matches!(key_state, KeyState::Released);
                if *key_code == hold_key && (is_focused || released) {
                    ctx.dispatch_typed_action(InputAction::VoiceHoldKeyChanged(*key_state));
                }
                DispatchEventResult::PropagateToParent
            })
            .finish()
    }

    /// Keeps the rich input editor's text legible when it renders on top of an alt-screen CLI
    /// agent's inferred background, which does not respect the Warp theme.
    pub(super) fn update_cli_agent_editor_text_colors(&mut self, ctx: &mut ViewContext<Self>) {
        let appearance = Appearance::as_ref(ctx);
        let default_colors = TextColors::from_appearance(appearance);

        let rich_input_open =
            CLIAgentSessionsModel::as_ref(ctx).is_input_open(self.terminal_view_id);

        let alt_screen_bg = if rich_input_open {
            let terminal_model = self.model.lock();
            terminal_model
                .is_alt_screen_active()
                .then(|| terminal_model.alt_screen().inferred_bg_color())
                .flatten()
        } else {
            None
        };

        let text_colors = match alt_screen_bg {
            Some(bg) => TextColors {
                default_color: default_colors
                    .default_color
                    .on_background(bg.into(), MinimumAllowedContrast::Text),
                disabled_color: default_colors
                    .disabled_color
                    .on_background(bg.into(), MinimumAllowedContrast::Text),
                hint_color: default_colors
                    .hint_color
                    .on_background(bg.into(), MinimumAllowedContrast::Text),
            },
            None => default_colors,
        };

        self.editor.update(ctx, |editor, ctx| {
            editor.set_text_colors(text_colors, ctx);
        });
    }
}
