use crate::env_vars::CloudEnvVarCollection;
use crate::search::mixer::SearchMixer;
use crate::server::ids::SyncId;
use crate::terminal::history::LinkedWorkflowData;
use crate::workflows::WorkflowType;

pub type CommandSearchMixer = SearchMixer<CommandSearchItemAction>;

#[derive(Clone, Debug)]
pub struct AcceptedHistoryItem {
    pub command: String,

    /// The workflow used to construct the command, if any.
    pub linked_workflow_data: Option<LinkedWorkflowData>,
}

/// Payload for `AcceptWorkflow`: identifies which workflow was selected.
#[derive(Clone, Debug)]
pub enum AcceptedWorkflow {
    Local { workflow: Box<WorkflowType> },
}

/// The set of events that may be produced by accepting or executing a search
/// result.
#[derive(Clone, Debug)]
pub enum CommandSearchItemAction {
    /// The user accepted a history search item. The contained string is the
    /// command they accepted.
    AcceptHistory(AcceptedHistoryItem),

    /// The user requested the re-execution of a history search item. The
    /// contained string is the command they accepted.
    ExecuteHistory(String),

    /// The user accepted a workflow search item.
    AcceptWorkflow(AcceptedWorkflow),

    /// The user accepted the notebook search item.
    AcceptNotebook(SyncId),

    /// The user accepted an EVC search item.
    AcceptEnvVarCollection(Box<CloudEnvVarCollection>),
}

#[cfg(test)]
#[path = "searcher_tests.rs"]
mod tests;
