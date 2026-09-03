use warpui_core::{Entity, ModelContext};

/// UI customization settings chosen during the "Customize your UI" onboarding slide.
#[derive(Clone, Debug)]
pub struct UICustomizationSettings {
    pub use_vertical_tabs: bool,
    pub show_conversation_history: bool,
    pub show_project_explorer: bool,
    pub show_global_search: bool,
    pub show_code_review_button: bool,
}

impl UICustomizationSettings {
    /// Defaults for terminal mode (all features disabled).
    pub fn terminal_defaults() -> Self {
        Self {
            use_vertical_tabs: false,
            show_conversation_history: false,
            show_project_explorer: false,
            show_global_search: false,
            show_code_review_button: false,
        }
    }

    /// Returns true if any visible tools-panel sub-setting is enabled. The
    /// conversation-history chip is hidden, so it does not count.
    pub fn tools_panel_enabled(&self) -> bool {
        self.show_project_explorer || self.show_global_search
    }
}

#[derive(Clone, Debug)]
pub struct SelectedSettings {
    pub ui_customization: Option<UICustomizationSettings>,
    pub cli_agent_toolbar_enabled: bool,
    pub show_agent_notifications: bool,
    pub agent_approval_yolo: bool,
}

impl SelectedSettings {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OnboardingStep {
    Intro,
    Customize,
    ThemePicker,
}

#[derive(Clone, Debug)]
pub(crate) enum OnboardingStateEvent {
    SelectedSlideChanged,
    Completed,
}

#[derive(Clone, Debug)]
pub(crate) struct OnboardingStateModel {
    step: OnboardingStep,
    ui_customization: UICustomizationSettings,
    cli_agent_toolbar_enabled: bool,
    show_agent_notifications: bool,
    agent_approval_yolo: bool,
}

impl OnboardingStateModel {
    /// Creates a new OnboardingStateModel.
    pub(crate) fn new() -> Self {
        Self {
            step: OnboardingStep::Intro,
            ui_customization: UICustomizationSettings::terminal_defaults(),
            cli_agent_toolbar_enabled: true,
            show_agent_notifications: false,
            agent_approval_yolo: true,
        }
    }

    pub(crate) fn settings(&self) -> SelectedSettings {
        SelectedSettings {
            ui_customization: Some(self.ui_customization.clone()),
            cli_agent_toolbar_enabled: self.cli_agent_toolbar_enabled,
            show_agent_notifications: self.show_agent_notifications,
            agent_approval_yolo: self.agent_approval_yolo,
        }
    }

    pub(crate) fn step(&self) -> OnboardingStep {
        self.step
    }

    pub fn ui_customization(&self) -> &UICustomizationSettings {
        &self.ui_customization
    }

    pub fn agent_approval_yolo(&self) -> bool {
        self.agent_approval_yolo
    }

    pub(crate) fn set_use_vertical_tabs(&mut self, value: bool, ctx: &mut ModelContext<Self>) {
        if self.ui_customization.use_vertical_tabs == value {
            return;
        }
        self.ui_customization.use_vertical_tabs = value;
        ctx.notify();
    }

    pub(crate) fn set_tools_panel_enabled(&mut self, enabled: bool, ctx: &mut ModelContext<Self>) {
        self.ui_customization.show_conversation_history = enabled;
        self.ui_customization.show_project_explorer = enabled;
        self.ui_customization.show_global_search = enabled;
        ctx.notify();
    }

    pub(crate) fn set_show_conversation_history(
        &mut self,
        value: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.ui_customization.show_conversation_history == value {
            return;
        }
        self.ui_customization.show_conversation_history = value;
        ctx.notify();
    }

    pub(crate) fn set_show_project_explorer(&mut self, value: bool, ctx: &mut ModelContext<Self>) {
        if self.ui_customization.show_project_explorer == value {
            return;
        }
        self.ui_customization.show_project_explorer = value;
        ctx.notify();
    }

    pub(crate) fn set_show_global_search(&mut self, value: bool, ctx: &mut ModelContext<Self>) {
        if self.ui_customization.show_global_search == value {
            return;
        }
        self.ui_customization.show_global_search = value;
        ctx.notify();
    }

    pub(crate) fn set_show_code_review_button(
        &mut self,
        value: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.ui_customization.show_code_review_button == value {
            return;
        }
        self.ui_customization.show_code_review_button = value;
        ctx.notify();
    }

    pub(crate) fn set_agent_approval_yolo(&mut self, value: bool, ctx: &mut ModelContext<Self>) {
        if self.agent_approval_yolo == value {
            return;
        }
        self.agent_approval_yolo = value;
        ctx.notify();
    }

    pub(crate) fn complete(&mut self, ctx: &mut ModelContext<Self>) {
        ctx.emit(OnboardingStateEvent::Completed);
        ctx.notify();
    }

    pub(crate) fn back(&mut self, ctx: &mut ModelContext<Self>) {
        let prev = match self.step {
            OnboardingStep::Intro => None,
            OnboardingStep::Customize => Some(OnboardingStep::Intro),
            OnboardingStep::ThemePicker => Some(OnboardingStep::Customize),
        };

        if let Some(prev) = prev {
            self.set_step(prev, ctx);
        }
    }

    pub(crate) fn next(&mut self, ctx: &mut ModelContext<Self>) {
        match self.step {
            OnboardingStep::Intro => self.set_step(OnboardingStep::Customize, ctx),
            OnboardingStep::Customize => self.set_step(OnboardingStep::ThemePicker, ctx),
            OnboardingStep::ThemePicker => {}
        }
    }

    pub(crate) fn set_step(&mut self, step: OnboardingStep, ctx: &mut ModelContext<Self>) {
        if self.step == step {
            return;
        }

        self.step = step;

        ctx.emit(OnboardingStateEvent::SelectedSlideChanged);
        ctx.notify();
    }

    /// The `(step_index, step_count)` shown by the bottom-nav progress dots for the
    /// current step.
    pub(crate) fn progress(&self) -> (usize, usize) {
        match self.step {
            OnboardingStep::Intro => (0, 3),
            OnboardingStep::Customize => (1, 3),
            OnboardingStep::ThemePicker => (2, 3),
        }
    }
}

impl Entity for OnboardingStateModel {
    type Event = OnboardingStateEvent;
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
