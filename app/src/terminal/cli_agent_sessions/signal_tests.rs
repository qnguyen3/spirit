use super::{AgentSignal, classify_generic_notification};
use crate::terminal::cli_agent_sessions::CLIAgentSessionStatus;

#[test]
fn permission_requests_are_classified_as_needing_input() {
    assert_eq!(
        classify_generic_notification("Claude needs your permission"),
        AgentSignal::NeedsInput
    );
    assert_eq!(
        classify_generic_notification("Claude Code needs your approval for the plan"),
        AgentSignal::NeedsInput
    );
}

#[test]
fn a_finished_turn_is_classified_as_done() {
    assert_eq!(
        classify_generic_notification("Claude is waiting for your input"),
        AgentSignal::Done
    );
    assert_eq!(
        classify_generic_notification("Implemented the consolidated Right Sidebar."),
        AgentSignal::Done
    );
}

#[test]
fn classification_ignores_case() {
    assert_eq!(
        classify_generic_notification("APPROVE THIS COMMAND?"),
        AgentSignal::NeedsInput
    );
}

#[test]
fn an_empty_body_falls_back_to_done() {
    assert_eq!(classify_generic_notification(""), AgentSignal::Done);
}

#[test]
fn statuses_without_an_outcome_produce_no_signal() {
    assert_eq!(AgentSignal::from_status(&CLIAgentSessionStatus::Idle), None);
    assert_eq!(
        AgentSignal::from_status(&CLIAgentSessionStatus::InProgress),
        None
    );
    assert_eq!(
        AgentSignal::from_status(&CLIAgentSessionStatus::Cancelled),
        None
    );
}

#[test]
fn terminal_statuses_map_back_to_their_signal() {
    assert_eq!(
        AgentSignal::from_status(&CLIAgentSessionStatus::Success),
        Some(AgentSignal::Done)
    );
    assert_eq!(
        AgentSignal::from_status(&CLIAgentSessionStatus::Blocked { message: None }),
        Some(AgentSignal::NeedsInput)
    );
    assert_eq!(
        AgentSignal::from_status(&CLIAgentSessionStatus::Failed {
            error_type: None,
            message: None
        }),
        Some(AgentSignal::Failed)
    );
}

#[test]
fn signals_round_trip_through_status() {
    for signal in [
        AgentSignal::Done,
        AgentSignal::NeedsInput,
        AgentSignal::Failed,
    ] {
        let status = signal.into_status(None);
        assert_eq!(AgentSignal::from_status(&status), Some(signal));
    }
}
