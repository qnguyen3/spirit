use super::CLIAgentSessionStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSignal {
    Working,
    Done,
    NeedsInput,
    Failed,
}

const NEEDS_INPUT_MARKERS: &[&str] = &["permission", "approval", "approve", "confirm"];

pub fn classify_generic_notification(body: &str) -> AgentSignal {
    let lowered = body.to_lowercase();
    if NEEDS_INPUT_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        AgentSignal::NeedsInput
    } else {
        AgentSignal::Done
    }
}

impl AgentSignal {
    pub fn into_status(self, message: Option<String>) -> CLIAgentSessionStatus {
        match self {
            AgentSignal::Working => CLIAgentSessionStatus::InProgress,
            AgentSignal::Done => CLIAgentSessionStatus::Success,
            AgentSignal::NeedsInput => CLIAgentSessionStatus::Blocked { message },
            AgentSignal::Failed => CLIAgentSessionStatus::Failed {
                error_type: None,
                message,
            },
        }
    }

    pub fn from_status(status: &CLIAgentSessionStatus) -> Option<Self> {
        match status {
            CLIAgentSessionStatus::Success => Some(AgentSignal::Done),
            CLIAgentSessionStatus::Blocked { .. } => Some(AgentSignal::NeedsInput),
            CLIAgentSessionStatus::Failed { .. } => Some(AgentSignal::Failed),
            CLIAgentSessionStatus::Idle
            | CLIAgentSessionStatus::InProgress
            | CLIAgentSessionStatus::Cancelled => None,
        }
    }

    pub fn is_success(self) -> bool {
        matches!(self, AgentSignal::Done)
    }
}

#[cfg(test)]
#[path = "signal_tests.rs"]
mod tests;
