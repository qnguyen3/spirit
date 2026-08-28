use pathfinder_color::ColorU;
use warp_core::ui::theme::Fill;
use warpui::elements::Element;
use warpui::event::KeyState;
use warpui::platform::OperatingSystem;
use warpui::platform::keyboard::KeyCode;
use warpui::presenter::ChildView;
use warpui::{AppContext, Entity, TypedActionView, View, ViewContext, ViewHandle};

use crate::appearance::Appearance;
use crate::terminal::view::cli_agent_footer::ActiveMicButtonTheme;
use crate::ui_components::icons::Icon;
use crate::view_components::action_button::{
    ActionButton, ActionButtonTheme, ButtonSize, NakedTheme, TooltipAlignment,
};

pub fn hold_key() -> KeyCode {
    match OperatingSystem::get() {
        OperatingSystem::Mac => KeyCode::Fn,
        OperatingSystem::Windows | OperatingSystem::Linux | OperatingSystem::Other(_) => {
            KeyCode::ControlRight
        }
    }
}

fn hold_key_label() -> &'static str {
    match OperatingSystem::get() {
        OperatingSystem::Mac => "fn",
        OperatingSystem::Windows | OperatingSystem::Linux | OperatingSystem::Other(_) => {
            "right ctrl"
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VoiceInputState {
    Idle,
    Listening,
}

impl VoiceInputState {
    fn tooltip(self) -> String {
        match self {
            VoiceInputState::Idle => format!("Voice input (hold {})", hold_key_label()),
            VoiceInputState::Listening => "Stop voice input".to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum VoiceInputButtonAction {
    Toggle,
}

/// Which surface the button is rendered on, which decides its chrome.
#[derive(Clone, Copy, PartialEq, Eq)]
enum VoiceInputButtonStyle {
    /// Borderless icon sitting under the terminal input.
    Naked,
    /// Bordered chip matching the rest of the CLI agent footer.
    AgentFooter,
}

pub struct VoiceInputButton {
    state: VoiceInputState,
    style: VoiceInputButtonStyle,
    button: ViewHandle<ActionButton>,
}

impl VoiceInputButton {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        Self::with_style(VoiceInputButtonStyle::Naked, ButtonSize::UDIButton, ctx)
    }

    pub fn new_for_agent_footer(ctx: &mut ViewContext<Self>) -> Self {
        Self::with_style(
            VoiceInputButtonStyle::AgentFooter,
            ButtonSize::AgentInputButton,
            ctx,
        )
    }

    fn with_style(
        style: VoiceInputButtonStyle,
        size: ButtonSize,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        let state = VoiceInputState::Idle;
        let button = ctx.add_typed_action_view(move |_| {
            ActionButton::new("", theme_for(style, false))
                .with_icon(Icon::Microphone)
                .with_tooltip(state.tooltip())
                .with_size(size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .on_click(|ctx| ctx.dispatch_typed_action(VoiceInputButtonAction::Toggle))
        });

        Self {
            state,
            style,
            button,
        }
    }

    pub fn handle_hold_key(&mut self, key_state: KeyState, ctx: &mut ViewContext<Self>) {
        match key_state {
            KeyState::Pressed => self.start_listening(ctx),
            KeyState::Released => self.finish_listening(ctx),
        }
    }

    fn start_listening(&mut self, ctx: &mut ViewContext<Self>) {
        self.set_state(VoiceInputState::Listening, ctx);
    }

    fn finish_listening(&mut self, ctx: &mut ViewContext<Self>) {
        self.set_state(VoiceInputState::Idle, ctx);
        // TODO: capture microphone audio while listening, transcribe it with a local ASR
        // model, and insert the transcript into the input editor. Nothing is recorded or
        // transcribed yet, so the button and the hold key only drive the listening state.
    }

    fn set_state(&mut self, state: VoiceInputState, ctx: &mut ViewContext<Self>) {
        if self.state == state {
            return;
        }

        self.state = state;
        let listening = matches!(state, VoiceInputState::Listening);
        let style = self.style;
        self.button.update(ctx, |button, ctx| {
            button.set_theme(theme_for(style, listening), ctx);
            button.set_active(listening, ctx);
            button.set_icon(
                Some(if listening {
                    Icon::Stop
                } else {
                    Icon::Microphone
                }),
                ctx,
            );
            button.set_tooltip(Some(state.tooltip()), ctx);
        });
        ctx.notify();
    }
}

impl Entity for VoiceInputButton {
    type Event = ();
}

impl TypedActionView for VoiceInputButton {
    type Action = VoiceInputButtonAction;

    fn handle_action(&mut self, action: &VoiceInputButtonAction, ctx: &mut ViewContext<Self>) {
        match action {
            VoiceInputButtonAction::Toggle => match self.state {
                VoiceInputState::Idle => self.start_listening(ctx),
                VoiceInputState::Listening => self.finish_listening(ctx),
            },
        }
    }
}

impl View for VoiceInputButton {
    fn ui_name() -> &'static str {
        "VoiceInputButton"
    }

    fn render(&self, _app: &AppContext) -> Box<dyn Element> {
        ChildView::new(&self.button).finish()
    }
}

fn theme_for(style: VoiceInputButtonStyle, listening: bool) -> VoiceInputButtonTheme {
    VoiceInputButtonTheme { style, listening }
}

struct VoiceInputButtonTheme {
    style: VoiceInputButtonStyle,
    listening: bool,
}

impl ActionButtonTheme for VoiceInputButtonTheme {
    fn background(&self, hovered: bool, appearance: &Appearance) -> Option<Fill> {
        match self.style {
            VoiceInputButtonStyle::Naked => NakedTheme.background(hovered, appearance),
            VoiceInputButtonStyle::AgentFooter => {
                ActiveMicButtonTheme.background(hovered, appearance)
            }
        }
    }

    fn text_color(
        &self,
        hovered: bool,
        background: Option<Fill>,
        appearance: &Appearance,
    ) -> ColorU {
        if self.listening {
            return appearance.theme().ansi_fg_red();
        }
        match self.style {
            VoiceInputButtonStyle::Naked => NakedTheme.text_color(hovered, background, appearance),
            VoiceInputButtonStyle::AgentFooter => {
                ActiveMicButtonTheme.text_color(hovered, background, appearance)
            }
        }
    }

    fn border(&self, appearance: &Appearance) -> Option<ColorU> {
        match self.style {
            VoiceInputButtonStyle::Naked => NakedTheme.border(appearance),
            VoiceInputButtonStyle::AgentFooter => ActiveMicButtonTheme.border(appearance),
        }
    }

    fn should_opt_out_of_contrast_adjustment(&self) -> bool {
        matches!(self.style, VoiceInputButtonStyle::AgentFooter)
    }
}

#[cfg(test)]
#[path = "voice_input_tests.rs"]
mod tests;
