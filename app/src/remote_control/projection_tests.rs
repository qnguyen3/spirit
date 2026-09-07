use remote_control::protocol::{
    AgentStatus, AgentSummary, PaneKind, PaneSnapshot, TabKind, TerminalSummary,
};

use super::{
    brand_color, build_agent_catalog, default_pane_title, status_message, status_rank, tab_kind,
    wire_agent_summary, wire_status,
};
use crate::projects::agent_status::WorktreeAgentSummary;
use crate::terminal::cli_agent::CLIAgent;
use crate::terminal::cli_agent_sessions::CLIAgentSessionStatus;

fn pane(kind: PaneKind) -> PaneSnapshot {
    PaneSnapshot {
        id: format!("{kind:?}"),
        kind,
        title: default_pane_title(kind),
        terminal: None,
    }
}

fn terminal_pane(terminal: TerminalSummary) -> PaneSnapshot {
    PaneSnapshot {
        id: "pane".to_owned(),
        kind: PaneKind::Terminal,
        title: "Terminal".to_owned(),
        terminal: Some(terminal),
    }
}

#[test]
fn a_tab_takes_the_kind_of_its_only_pane_kind() {
    assert_eq!(tab_kind(&[pane(PaneKind::Terminal)]), TabKind::Terminal);
    assert_eq!(
        tab_kind(&[pane(PaneKind::Terminal), pane(PaneKind::Terminal)]),
        TabKind::Terminal
    );
    assert_eq!(tab_kind(&[pane(PaneKind::Code)]), TabKind::Code);
    assert_eq!(tab_kind(&[pane(PaneKind::File)]), TabKind::File);
    assert_eq!(
        tab_kind(&[pane(PaneKind::AgentPicker)]),
        TabKind::AgentPicker
    );
    assert_eq!(tab_kind(&[pane(PaneKind::Settings)]), TabKind::Settings);
    assert_eq!(tab_kind(&[pane(PaneKind::Other)]), TabKind::Other);
}

#[test]
fn a_tab_with_several_pane_kinds_is_mixed() {
    assert_eq!(
        tab_kind(&[pane(PaneKind::Terminal), pane(PaneKind::Code)]),
        TabKind::Mixed
    );
}

#[test]
fn a_tab_with_no_panes_is_other() {
    assert_eq!(tab_kind(&[]), TabKind::Other);
}

#[test]
fn a_terminal_pane_still_reports_the_terminal_kind() {
    let summary = TerminalSummary {
        terminal_id: "1".to_owned(),
        title: None,
        cwd: None,
        mode: remote_control::protocol::TerminalMode::Prompt,
        cols: 80,
        rows: 24,
        read_only: false,
        agent: None,
    };
    assert_eq!(tab_kind(&[terminal_pane(summary)]), TabKind::Terminal);
}

#[test]
fn agent_statuses_map_onto_the_wire_enum() {
    let cases = [
        (CLIAgentSessionStatus::Idle, AgentStatus::Idle),
        (CLIAgentSessionStatus::InProgress, AgentStatus::InProgress),
        (CLIAgentSessionStatus::Success, AgentStatus::Success),
        (CLIAgentSessionStatus::Cancelled, AgentStatus::Cancelled),
        (
            CLIAgentSessionStatus::Failed {
                error_type: None,
                message: None,
            },
            AgentStatus::Failed,
        ),
        (
            CLIAgentSessionStatus::Blocked { message: None },
            AgentStatus::Blocked,
        ),
    ];
    for (status, expected) in cases {
        assert_eq!(wire_status(&status), expected);
    }
}

#[test]
fn only_failed_and_blocked_carry_a_status_message() {
    assert_eq!(
        status_message(&CLIAgentSessionStatus::Blocked {
            message: Some("needs approval".to_owned())
        }),
        Some("needs approval".to_owned())
    );
    assert_eq!(
        status_message(&CLIAgentSessionStatus::Failed {
            error_type: Some("io".to_owned()),
            message: Some("boom".to_owned())
        }),
        Some("boom".to_owned())
    );
    assert_eq!(status_message(&CLIAgentSessionStatus::Idle), None);
    assert_eq!(status_message(&CLIAgentSessionStatus::InProgress), None);
    assert_eq!(status_message(&CLIAgentSessionStatus::Success), None);
    assert_eq!(status_message(&CLIAgentSessionStatus::Cancelled), None);
}

#[test]
fn blocked_and_failed_sessions_rank_before_everything_else() {
    assert_eq!(
        status_rank(&CLIAgentSessionStatus::Blocked { message: None }),
        0
    );
    assert_eq!(
        status_rank(&CLIAgentSessionStatus::Failed {
            error_type: None,
            message: None
        }),
        0
    );
    assert_eq!(status_rank(&CLIAgentSessionStatus::InProgress), 1);
    assert_eq!(status_rank(&CLIAgentSessionStatus::Idle), 2);
    assert_eq!(status_rank(&CLIAgentSessionStatus::Success), 2);
    assert_eq!(status_rank(&CLIAgentSessionStatus::Cancelled), 2);
}

#[test]
fn worktree_summaries_map_onto_the_wire_enum() {
    assert_eq!(
        wire_agent_summary(WorktreeAgentSummary::None),
        AgentSummary::None
    );
    assert_eq!(
        wire_agent_summary(WorktreeAgentSummary::Working),
        AgentSummary::Working
    );
    assert_eq!(
        wire_agent_summary(WorktreeAgentSummary::NeedsAttention),
        AgentSummary::NeedsAttention
    );
}

#[test]
fn brand_colors_are_css_hex() {
    for agent in [CLIAgent::Claude, CLIAgent::Codex, CLIAgent::Unknown] {
        let color = brand_color(agent);
        assert_eq!(color.len(), 7, "{agent:?} produced {color}");
        assert!(color.starts_with('#'));
        assert!(color[1..].chars().all(|c| c.is_ascii_hexdigit()));
    }
}

#[test]
fn the_agent_catalog_is_indexed_in_order_and_names_every_agent() {
    let entries = build_agent_catalog(None);
    assert!(!entries.is_empty());
    for (index, entry) in entries.iter().enumerate() {
        assert_eq!(entry.index, index);
        assert!(!entry.display_name.is_empty());
        assert!(!entry.id.is_empty());
        assert!(entry.brand_color.starts_with('#'));
    }
}

#[test]
fn the_first_catalog_entry_supports_yolo() {
    let entries = build_agent_catalog(None);
    assert!(entries[0].supports_yolo);
}

#[test]
fn pane_titles_are_never_empty() {
    for kind in [
        PaneKind::Terminal,
        PaneKind::Code,
        PaneKind::File,
        PaneKind::AgentPicker,
        PaneKind::Settings,
        PaneKind::Other,
    ] {
        assert!(!default_pane_title(kind).is_empty());
    }
}
