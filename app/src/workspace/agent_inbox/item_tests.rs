use uuid::Uuid;
use warpui::{EntityId, WindowId};

use super::*;
use crate::pane_group::TerminalPaneId;
use crate::terminal::CLIAgent;

fn item(terminal_view_id: EntityId, project: Option<ProjectId>, workspace_name: &str) -> InboxItem {
    InboxItem::new(InboxItemFields {
        terminal_view_id,
        window_id: WindowId::new(),
        project_id: project,
        workspace_name: workspace_name.to_owned(),
        task_title: "fix worktree bug".to_owned(),
        outcome: AgentSignal::Done,
        agent: CLIAgent::Claude,
        pane_group_id: EntityId::new(),
        pane_id: TerminalPaneId::dummy_terminal_pane_id().into(),
        is_read: false,
    })
}

fn project(seed: u128) -> Option<ProjectId> {
    Some(ProjectId(Uuid::from_u128(seed)))
}

#[test]
fn a_second_notification_replaces_the_first_from_the_same_terminal() {
    let mut items = InboxItems::default();
    let terminal = EntityId::new();
    items.push(item(terminal, project(1), "spirit"));
    items.push(item(terminal, project(1), "spirit"));
    assert_eq!(items.count(InboxFilter::AllWorkspaces), 1);
}

#[test]
fn filters_list_all_workspaces_then_one_tab_per_workspace() {
    let mut items = InboxItems::default();
    items.push(item(EntityId::new(), project(1), "spirit"));
    items.push(item(EntityId::new(), project(2), "warp"));
    items.push(item(EntityId::new(), project(1), "spirit"));

    assert_eq!(
        items.visible_filters(),
        vec![
            InboxFilter::AllWorkspaces,
            InboxFilter::Workspace(project(1)),
            InboxFilter::Workspace(project(2)),
        ]
    );
    assert_eq!(items.count(InboxFilter::AllWorkspaces), 3);
    assert_eq!(items.count(InboxFilter::Workspace(project(1))), 2);
    assert_eq!(items.count(InboxFilter::Workspace(project(2))), 1);
}

#[test]
fn the_projectless_home_workspace_gets_its_own_filter() {
    let mut items = InboxItems::default();
    items.push(item(EntityId::new(), None, "Home"));
    items.push(item(EntityId::new(), project(1), "spirit"));

    assert!(
        items
            .visible_filters()
            .contains(&InboxFilter::Workspace(None))
    );
    assert_eq!(items.filter_label(InboxFilter::Workspace(None)), "Home");
    assert_eq!(items.count(InboxFilter::Workspace(None)), 1);
}

#[test]
fn filter_bar_stops_growing_past_the_popup_width() {
    let mut items = InboxItems::default();
    for index in 0..8 {
        items.push(item(EntityId::new(), project(index), "workspace"));
    }
    assert_eq!(items.visible_filters().len(), MAX_WORKSPACE_FILTERS + 1);
}

#[test]
fn long_workspace_names_are_truncated_in_the_filter_label() {
    let mut items = InboxItems::default();
    items.push(item(
        EntityId::new(),
        project(1),
        "a-very-long-workspace-name",
    ));
    let label = items.filter_label(InboxFilter::Workspace(project(1)));
    assert_eq!(label.chars().count(), MAX_FILTER_LABEL_CHARS);
    assert!(label.ends_with(ELLIPSIS));
}

#[test]
fn unread_count_tracks_marking_read() {
    let mut items = InboxItems::default();
    items.push(item(EntityId::new(), project(1), "spirit"));
    items.push(item(EntityId::new(), project(2), "warp"));
    assert_eq!(items.unread_count(), 2);

    let first = items.ids_matching(InboxFilter::AllWorkspaces)[0];
    assert!(items.mark_read(first));
    assert!(!items.mark_read(first));
    assert_eq!(items.unread_count(), 1);

    assert!(items.mark_all_read());
    assert_eq!(items.unread_count(), 0);
}

#[test]
fn message_phrasing_follows_the_outcome() {
    let mut done = item(EntityId::new(), project(1), "spirit");
    done.outcome = AgentSignal::Done;
    assert_eq!(done.message(), "Task \"fix worktree bug\" is done.");

    let mut blocked = item(EntityId::new(), project(1), "spirit");
    blocked.outcome = AgentSignal::NeedsInput;
    assert_eq!(
        blocked.message(),
        "Task \"fix worktree bug\" needs your input."
    );

    let mut failed = item(EntityId::new(), project(1), "spirit");
    failed.outcome = AgentSignal::Failed;
    assert_eq!(failed.message(), "Task \"fix worktree bug\" failed.");
}
