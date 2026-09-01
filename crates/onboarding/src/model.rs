use warpui_core::{Entity, ModelContext};

use crate::slides::OfferVariant;

/// UI customization settings chosen during the "Customize your UI" onboarding slide.
#[derive(Clone, Debug)]
pub struct UICustomizationSettings {
    pub use_vertical_tabs: bool,
    pub show_conversation_history: bool,
    pub show_project_explorer: bool,
    pub show_global_search: bool,
    pub show_warp_drive: bool,
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
            show_warp_drive: false,
            show_code_review_button: false,
        }
    }

    /// Returns true if any visible tools-panel sub-setting is enabled. The
    /// conversation-history chip is hidden, so it does not count.
    pub fn tools_panel_enabled(&self) -> bool {
        self.show_project_explorer || self.show_global_search || self.show_warp_drive
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnboardingAuthState {
    LoggedOut,
    FreeUser,
    PayingUser,
}

#[derive(Clone, Debug)]
pub struct SelectedSettings {
    pub ui_customization: Option<UICustomizationSettings>,
    pub cli_agent_toolbar_enabled: bool,
    pub show_agent_notifications: bool,
}

impl SelectedSettings {
    pub fn is_warp_drive_enabled(&self) -> bool {
        self.ui_customization
            .as_ref()
            .map(|ui| ui.show_warp_drive)
            .unwrap_or(false)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OnboardingStep {
    Intro,
    Customize,
    ThemePicker,
    PostAuthOffer,
}

#[derive(Clone, Debug)]
pub(crate) enum OnboardingStateEvent {
    SelectedSlideChanged,
    Completed,
    UpgradeRequested,
    AuthStateChanged,
    /// The user can now use AI, so onboarding may advance past the offer slide.
    AiSellOfferSatisfied,
}

#[derive(Clone, Debug)]
pub(crate) struct OnboardingStateModel {
    step: OnboardingStep,
    ui_customization: UICustomizationSettings,
    cli_agent_toolbar_enabled: bool,
    show_agent_notifications: bool,
    /// Auth / billing state of the user.
    auth_state: OnboardingAuthState,
    /// Which account-first offer is currently presented after authentication.
    offer_variant: Option<OfferVariant>,
    pricing_promotion_message: Option<String>,
}

impl OnboardingStateModel {
    /// Creates a new OnboardingStateModel.
    pub(crate) fn new(auth_state: OnboardingAuthState) -> Self {
        Self {
            step: OnboardingStep::Intro,
            ui_customization: UICustomizationSettings::terminal_defaults(),
            cli_agent_toolbar_enabled: true,
            show_agent_notifications: false,
            auth_state,
            offer_variant: None,
            pricing_promotion_message: None,
        }
    }

    pub(crate) fn auth_state(&self) -> OnboardingAuthState {
        self.auth_state
    }

    pub(crate) fn offer_variant(&self) -> Option<OfferVariant> {
        self.offer_variant
    }

    pub(crate) fn show_post_auth_offer(
        &mut self,
        variant: OfferVariant,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.step == OnboardingStep::PostAuthOffer {
            return;
        }
        self.offer_variant = Some(variant);
        self.set_step(OnboardingStep::PostAuthOffer, ctx);
    }

    pub(crate) fn set_auth_state(
        &mut self,
        auth_state: OnboardingAuthState,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.auth_state == auth_state {
            return;
        }
        self.auth_state = auth_state;
        ctx.emit(OnboardingStateEvent::AuthStateChanged);
    }

    pub(crate) fn settings(&self) -> SelectedSettings {
        SelectedSettings {
            ui_customization: Some(self.ui_customization.clone()),
            cli_agent_toolbar_enabled: self.cli_agent_toolbar_enabled,
            show_agent_notifications: self.show_agent_notifications,
        }
    }

    pub(crate) fn step(&self) -> OnboardingStep {
        self.step
    }

    pub(crate) fn pricing_promotion_message(&self) -> Option<&str> {
        self.pricing_promotion_message.as_deref()
    }

    pub(crate) fn set_pricing_promotion_message(
        &mut self,
        message: Option<String>,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.pricing_promotion_message == message {
            return;
        }
        self.pricing_promotion_message = message;
        ctx.notify();
    }

    /// Reports whether the user can make an AI request. The AI-sell offer
    /// exists to get the user AI usage, so observing that they now have it is
    /// the whole completion condition — a plan or one-time credits, bought
    /// through any call to action.
    pub(crate) fn on_credit_availability_observed(
        &mut self,
        available: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        if !available || !self.is_showing_ai_sell_offer() {
            return;
        }
        self.finish_ai_sell_offer(ctx);
    }

    /// A web checkout reported success through the desktop hand-off. The grant
    /// can lag the redirect, so the hand-off itself is trusted rather than
    /// waiting for an availability read. Returns whether an AI-sell offer
    /// consumed the signal.
    pub(crate) fn on_checkout_succeeded(&mut self, ctx: &mut ModelContext<Self>) -> bool {
        if !self.is_showing_ai_sell_offer() {
            return false;
        }
        self.finish_ai_sell_offer(ctx);
        true
    }

    /// Whether an onboarding screen whose purpose is to sell AI usage is on
    /// screen. The head-start offer is excluded: it ships with AI usage already
    /// on the account, so availability there says nothing about whether the
    /// user has made their choice yet.
    fn is_showing_ai_sell_offer(&self) -> bool {
        self.step == OnboardingStep::PostAuthOffer
            && self.offer_variant.is_some_and(OfferVariant::sells_ai_usage)
    }

    /// Reports that the user can now use AI, so onboarding moves past the offer.
    fn finish_ai_sell_offer(&mut self, ctx: &mut ModelContext<Self>) {
        ctx.emit(OnboardingStateEvent::AiSellOfferSatisfied);
        ctx.notify();
    }

    pub fn ui_customization(&self) -> &UICustomizationSettings {
        &self.ui_customization
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
        self.ui_customization.show_warp_drive = enabled;
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

    pub(crate) fn set_show_warp_drive(&mut self, value: bool, ctx: &mut ModelContext<Self>) {
        if self.ui_customization.show_warp_drive == value {
            return;
        }
        self.ui_customization.show_warp_drive = value;
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

    pub(crate) fn request_upgrade(&mut self, ctx: &mut ModelContext<Self>) {
        ctx.emit(OnboardingStateEvent::UpgradeRequested);
    }

    pub(crate) fn complete(&mut self, ctx: &mut ModelContext<Self>) {
        ctx.emit(OnboardingStateEvent::Completed);
        ctx.notify();
    }

    pub(crate) fn back(&mut self, ctx: &mut ModelContext<Self>) {
        use warp_core::features::FeatureFlag;
        let account_first = FeatureFlag::AccountFirstOnboarding.is_enabled();
        let prev = match self.step {
            OnboardingStep::Intro => None,
            OnboardingStep::Customize => Some(OnboardingStep::Intro),
            OnboardingStep::ThemePicker => Some(OnboardingStep::Customize),
            OnboardingStep::PostAuthOffer => {
                if account_first {
                    Some(OnboardingStep::ThemePicker)
                } else {
                    None
                }
            }
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
            OnboardingStep::PostAuthOffer => {}
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
        if warp_core::features::FeatureFlag::AccountFirstOnboarding.is_enabled() {
            return match self.step {
                OnboardingStep::Intro | OnboardingStep::Customize => (0, 3),
                OnboardingStep::ThemePicker => (1, 3),
                OnboardingStep::PostAuthOffer => (0, 0),
            };
        }

        match self.step {
            OnboardingStep::Intro => (0, 3),
            OnboardingStep::Customize => (1, 3),
            OnboardingStep::ThemePicker => (2, 3),
            OnboardingStep::PostAuthOffer => (0, 0),
        }
    }
}

impl Entity for OnboardingStateModel {
    type Event = OnboardingStateEvent;
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
