// Onboarding library crate

mod agent_onboarding_view;
pub mod components;
mod model;
pub mod slides;

pub use agent_onboarding_view::{AgentOnboardingAction, AgentOnboardingEvent, AgentOnboardingView};
pub use model::{SelectedSettings, UICustomizationSettings};

pub fn init(app: &mut warpui_core::AppContext) {
    agent_onboarding_view::init(app);
}
