use warpui::notification::UserNotification;

use crate::terminal::cli_agent_sessions::signal::AgentSignal;
use crate::terminal::view::BlockNotification;

const ELLIPSIS: char = '\u{2026}';

fn truncate_to_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let text = text.trim();
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let kept: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}{ELLIPSIS}", kept.trim_end())
}

/// The whole sentence does not fit `UserNotification::MAX_TITLE_LENGTH`, and the
/// OS truncates an over-long title silently, so the task line goes in the body.
pub fn format_agent_notification(
    workspace_name: &str,
    task_title: &str,
    outcome: AgentSignal,
) -> BlockNotification {
    let suffix = match outcome {
        AgentSignal::Done => "is done",
        AgentSignal::NeedsInput => "needs your input",
        AgentSignal::Failed => "failed",
        AgentSignal::Working => "is running",
    };

    let body_overhead = "Task \"\" .".chars().count() + suffix.chars().count();
    let task_budget = UserNotification::MAX_BODY_LENGTH.saturating_sub(body_overhead);
    let task = truncate_to_chars(task_title, task_budget);

    BlockNotification {
        title: truncate_to_chars(workspace_name, UserNotification::MAX_TITLE_LENGTH),
        body: format!("Task \"{task}\" {suffix}."),
    }
}

#[cfg(test)]
#[path = "agent_notification_tests.rs"]
mod tests;
