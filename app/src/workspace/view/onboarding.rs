use onboarding::SelectedSettings;

use crate::terminal::view::OnboardingIntention;

/// Configuration for starting the agent onboarding tutorial.
#[derive(Debug, Clone)]
pub enum OnboardingTutorial {
    /// Start tutorial without a project context.
    NoProject { intention: OnboardingIntention },
}

impl From<SelectedSettings> for OnboardingTutorial {
    fn from(settings: SelectedSettings) -> Self {
        let intention = match settings {
            SelectedSettings::AgentDrivenDevelopment { .. } => {
                OnboardingIntention::AgentDrivenDevelopment
            }
            SelectedSettings::Terminal { .. } => OnboardingIntention::Terminal,
        };
        OnboardingTutorial::NoProject { intention }
    }
}
