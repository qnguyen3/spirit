use onboarding::SelectedSettings;

/// Configuration for starting the agent onboarding tutorial.
#[derive(Debug, Clone)]
pub enum OnboardingTutorial {
    /// Start tutorial without a project context.
    NoProject,
}

impl From<SelectedSettings> for OnboardingTutorial {
    fn from(_: SelectedSettings) -> Self {
        OnboardingTutorial::NoProject
    }
}
