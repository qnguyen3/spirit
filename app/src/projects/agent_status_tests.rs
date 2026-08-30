use super::*;

#[test]
fn needs_attention_wins_over_working() {
    assert_eq!(
        WorktreeAgentSummary::Working.merge(WorktreeAgentSummary::NeedsAttention),
        WorktreeAgentSummary::NeedsAttention
    );
    assert_eq!(
        WorktreeAgentSummary::NeedsAttention.merge(WorktreeAgentSummary::Working),
        WorktreeAgentSummary::NeedsAttention
    );
    assert_eq!(
        WorktreeAgentSummary::None.merge(WorktreeAgentSummary::Working),
        WorktreeAgentSummary::Working
    );
    assert_eq!(
        WorktreeAgentSummary::None.merge(WorktreeAgentSummary::None),
        WorktreeAgentSummary::None
    );
}

#[test]
fn statuses_map_to_summaries() {
    assert_eq!(
        summarize_status(&CLIAgentSessionStatus::InProgress),
        WorktreeAgentSummary::Working
    );
    assert_eq!(
        summarize_status(&CLIAgentSessionStatus::Blocked { message: None }),
        WorktreeAgentSummary::NeedsAttention
    );
    assert_eq!(
        summarize_status(&CLIAgentSessionStatus::Failed {
            error_type: None,
            message: None
        }),
        WorktreeAgentSummary::NeedsAttention
    );
    assert_eq!(
        summarize_status(&CLIAgentSessionStatus::Success),
        WorktreeAgentSummary::None
    );
    assert_eq!(
        summarize_status(&CLIAgentSessionStatus::Cancelled),
        WorktreeAgentSummary::None
    );
}

#[test]
fn a_detected_but_unused_agent_does_not_report_as_working() {
    assert_eq!(
        summarize_status(&CLIAgentSessionStatus::Idle),
        WorktreeAgentSummary::None
    );
}
