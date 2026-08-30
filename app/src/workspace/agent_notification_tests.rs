use warpui::notification::UserNotification;

use super::format_agent_notification;
use crate::terminal::cli_agent_sessions::signal::AgentSignal;

#[test]
fn the_workspace_name_is_the_title_and_the_task_is_the_body() {
    let notification = format_agent_notification("spirit", "fix worktree bug", AgentSignal::Done);
    assert_eq!(notification.title, "spirit");
    assert_eq!(notification.body, "Task \"fix worktree bug\" is done.");
}

#[test]
fn each_outcome_gets_its_own_wording() {
    assert_eq!(
        format_agent_notification("spirit", "review", AgentSignal::NeedsInput).body,
        "Task \"review\" needs your input."
    );
    assert_eq!(
        format_agent_notification("spirit", "review", AgentSignal::Failed).body,
        "Task \"review\" failed."
    );
}

#[test]
fn a_long_workspace_name_is_truncated_to_the_title_budget() {
    let name = "w".repeat(UserNotification::MAX_TITLE_LENGTH + 20);
    let notification = format_agent_notification(&name, "task", AgentSignal::Done);
    assert_eq!(
        notification.title.chars().count(),
        UserNotification::MAX_TITLE_LENGTH
    );
    assert!(notification.title.ends_with('\u{2026}'));
}

#[test]
fn a_long_task_title_keeps_the_body_within_budget() {
    let task = "t".repeat(400);
    let notification = format_agent_notification("spirit", &task, AgentSignal::NeedsInput);
    assert!(notification.body.chars().count() <= UserNotification::MAX_BODY_LENGTH);
    assert!(notification.body.ends_with("needs your input."));
}

#[test]
fn truncation_lands_on_character_boundaries() {
    let task = "😊".repeat(200);
    let notification = format_agent_notification("spirit", &task, AgentSignal::Done);
    assert!(notification.body.chars().count() <= UserNotification::MAX_BODY_LENGTH);
    assert!(notification.body.starts_with("Task \"😊"));
}

#[test]
fn surrounding_whitespace_is_trimmed() {
    let notification = format_agent_notification("  spirit  ", "  build  ", AgentSignal::Done);
    assert_eq!(notification.title, "spirit");
    assert_eq!(notification.body, "Task \"build\" is done.");
}
