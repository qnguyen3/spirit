// Onboarding library crate

mod agent_onboarding_view;
mod model;
pub mod slides;

/// User-facing names of the Warp Drive features enabled when Warp Drive is
pub mod components;

pub use agent_onboarding_view::{AgentOnboardingAction, AgentOnboardingEvent, AgentOnboardingView};
pub use model::{OnboardingAuthState, SelectedSettings, UICustomizationSettings};
pub use slides::OfferVariant;

pub fn init(app: &mut warpui_core::AppContext) {
    agent_onboarding_view::init(app);
}
