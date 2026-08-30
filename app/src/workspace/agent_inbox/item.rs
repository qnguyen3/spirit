use instant::Instant;
use uuid::Uuid;
use warpui::{EntityId, WindowId};

use crate::pane_group::PaneId;
use crate::projects::ProjectId;
use crate::terminal::CLIAgent;
use crate::terminal::cli_agent_sessions::signal::AgentSignal;

const MAX_ITEMS: usize = 100;

const MAX_WORKSPACE_FILTERS: usize = 4;

const MAX_FILTER_LABEL_CHARS: usize = 16;

const ELLIPSIS: char = '\u{2026}';

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InboxItemId(Uuid);

impl InboxItemId {
    fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// `Workspace(None)` is the projectless "Home" workspace, not an absent one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InboxFilter {
    AllWorkspaces,
    Workspace(Option<ProjectId>),
}

pub struct InboxItemFields {
    pub terminal_view_id: EntityId,
    pub window_id: WindowId,
    pub project_id: Option<ProjectId>,
    pub workspace_name: String,
    pub task_title: String,
    pub outcome: AgentSignal,
    pub agent: CLIAgent,
    pub pane_group_id: EntityId,
    pub pane_id: PaneId,
    pub is_read: bool,
}

pub struct InboxItem {
    pub id: InboxItemId,
    pub created_at: Instant,
    pub terminal_view_id: EntityId,
    pub window_id: WindowId,
    pub project_id: Option<ProjectId>,
    pub workspace_name: String,
    pub task_title: String,
    pub outcome: AgentSignal,
    pub agent: CLIAgent,
    pub pane_group_id: EntityId,
    pub pane_id: PaneId,
    pub is_read: bool,
}

impl InboxItem {
    pub fn new(fields: InboxItemFields) -> Self {
        Self {
            id: InboxItemId::new(),
            created_at: Instant::now(),
            terminal_view_id: fields.terminal_view_id,
            window_id: fields.window_id,
            project_id: fields.project_id,
            workspace_name: fields.workspace_name,
            task_title: fields.task_title,
            outcome: fields.outcome,
            agent: fields.agent,
            pane_group_id: fields.pane_group_id,
            pane_id: fields.pane_id,
            is_read: fields.is_read,
        }
    }

    pub fn message(&self) -> String {
        let suffix = match self.outcome {
            AgentSignal::Done => "is done",
            AgentSignal::NeedsInput => "needs your input",
            AgentSignal::Failed => "failed",
            AgentSignal::Working => "is running",
        };
        format!("Task \"{}\" {suffix}.", self.task_title)
    }

    fn mark_as_read(&mut self) -> bool {
        if self.is_read {
            return false;
        }
        self.is_read = true;
        true
    }
}

fn truncate_label(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let kept: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}{ELLIPSIS}", kept.trim_end())
}

#[derive(Default)]
pub struct InboxItems {
    items: Vec<InboxItem>,
}

impl InboxItems {
    pub fn push(&mut self, item: InboxItem) {
        self.remove_for_terminal_view(item.terminal_view_id);
        self.items.insert(0, item);
        self.items.truncate(MAX_ITEMS);
    }

    pub fn remove_for_terminal_view(&mut self, terminal_view_id: EntityId) -> bool {
        let before = self.items.len();
        self.items
            .retain(|item| item.terminal_view_id != terminal_view_id);
        self.items.len() != before
    }

    pub fn matching(&self, filter: InboxFilter) -> impl Iterator<Item = &InboxItem> {
        self.items.iter().filter(move |item| match filter {
            InboxFilter::AllWorkspaces => true,
            InboxFilter::Workspace(project_id) => item.project_id == project_id,
        })
    }

    pub fn count(&self, filter: InboxFilter) -> usize {
        self.matching(filter).count()
    }

    pub fn unread_count(&self) -> usize {
        self.items.iter().filter(|item| !item.is_read).count()
    }

    pub fn ids_matching(&self, filter: InboxFilter) -> Vec<InboxItemId> {
        self.matching(filter).map(|item| item.id).collect()
    }

    pub fn visible_filters(&self) -> Vec<InboxFilter> {
        let mut seen: Vec<Option<ProjectId>> = Vec::new();
        for item in &self.items {
            if seen.len() == MAX_WORKSPACE_FILTERS {
                break;
            }
            if !seen.contains(&item.project_id) {
                seen.push(item.project_id);
            }
        }
        std::iter::once(InboxFilter::AllWorkspaces)
            .chain(seen.into_iter().map(InboxFilter::Workspace))
            .collect()
    }

    pub fn filter_label(&self, filter: InboxFilter) -> String {
        match filter {
            InboxFilter::AllWorkspaces => "All workspaces".to_owned(),
            InboxFilter::Workspace(project_id) => self
                .items
                .iter()
                .find(|item| item.project_id == project_id)
                .map(|item| truncate_label(&item.workspace_name, MAX_FILTER_LABEL_CHARS))
                .unwrap_or_else(|| "Workspace".to_owned()),
        }
    }

    pub fn get(&self, id: InboxItemId) -> Option<&InboxItem> {
        self.items.iter().find(|item| item.id == id)
    }

    pub fn mark_read(&mut self, id: InboxItemId) -> bool {
        self.items
            .iter_mut()
            .find(|item| item.id == id)
            .is_some_and(|item| item.mark_as_read())
    }

    pub fn mark_all_read(&mut self) -> bool {
        let mut any_changed = false;
        for item in &mut self.items {
            any_changed |= item.mark_as_read();
        }
        any_changed
    }

    pub fn has_unread_for_terminal_view(&self, terminal_view_id: EntityId) -> bool {
        self.items
            .iter()
            .any(|item| item.terminal_view_id == terminal_view_id && !item.is_read)
    }

    pub fn mark_terminal_view_read(&mut self, terminal_view_id: EntityId) -> bool {
        let mut any_changed = false;
        for item in &mut self.items {
            if item.terminal_view_id == terminal_view_id {
                any_changed |= item.mark_as_read();
            }
        }
        any_changed
    }
}

#[cfg(test)]
#[path = "item_tests.rs"]
mod tests;
