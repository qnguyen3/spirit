use pathfinder_color::ColorU;
use warp_core::ui::theme::Fill;
use warpui::elements::Element;
use warpui::event::KeyState;
use warpui::platform::OperatingSystem;
use warpui::platform::keyboard::KeyCode;
use warpui::presenter::ChildView;
use warpui::{AppContext, Entity, TypedActionView, View, ViewContext, ViewHandle};

use crate::appearance::Appearance;
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

pub struct VoiceInputButton {
    state: VoiceInputState,
    button: ViewHandle<ActionButton>,
}

impl VoiceInputButton {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let state = VoiceInputState::Idle;
        let button = ctx.add_typed_action_view(|_| {
            ActionButton::new("", VoiceInputButtonTheme { listening: false })
                .with_icon(Icon::Microphone)
                .with_tooltip(state.tooltip())
                .with_size(ButtonSize::UDIButton)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .on_click(|ctx| ctx.dispatch_typed_action(VoiceInputButtonAction::Toggle))
        });

        Self { state, button }
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
        self.button.update(ctx, |button, ctx| {
            button.set_theme(VoiceInputButtonTheme { listening }, ctx);
            button.set_active(listening, ctx);
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

struct VoiceInputButtonTheme {
    listening: bool,
}

impl ActionButtonTheme for VoiceInputButtonTheme {
    fn background(&self, hovered: bool, appearance: &Appearance) -> Option<Fill> {
        NakedTheme.background(hovered, appearance)
    }

    fn text_color(
        &self,
        hovered: bool,
        background: Option<Fill>,
        appearance: &Appearance,
    ) -> ColorU {
        if self.listening {
            appearance.theme().ansi_fg_red()
        } else {
            NakedTheme.text_color(hovered, background, appearance)
        }
    }
}

#[cfg(test)]
#[path = "voice_input_tests.rs"]
mod tests;
