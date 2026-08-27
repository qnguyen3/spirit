// Onboarding library crate

mod agent_onboarding_view;
mod model;
pub mod slides;
pub mod telemetry;

/// User-facing names of the Warp Drive features enabled when Warp Drive is
/// turned on. Shared by the login slide's skip-login confirmation dialog so the
/// list stays in sync with any future surfaces that need it.
pub const WARP_DRIVE_FEATURES: &[&str] = &["Warp Drive", "Session Sharing"];

cfg_if::cfg_if! {
    if #[cfg(feature = "bin")] {
        mod telemetry_provider;
        pub use telemetry_provider::MockTelemetryContextProvider;
    }
}

pub mod components;

pub use agent_onboarding_view::{AgentOnboardingAction, AgentOnboardingEvent, AgentOnboardingView};
pub use model::{OnboardingAuthState, SelectedSettings, UICustomizationSettings};
pub use slides::OfferVariant;
pub use telemetry::OnboardingEvent;

pub fn init(app: &mut warpui_core::AppContext) {
    agent_onboarding_view::init(app);
}
