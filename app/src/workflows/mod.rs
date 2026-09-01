
use serde::{Deserialize, Serialize};
use warpui::AppContext;

pub mod categories;
use workflow::Workflow;

pub mod arguments;
pub mod command_parser;
pub mod info_box;
pub mod local_workflows;
pub mod workflow;

pub use categories::{CategoriesView, CategoriesViewEvent, WorkflowsViewAction};

use crate::notebooks::{NotebookId, NotebookLocation};

pub fn init(app: &mut AppContext) {
    categories::init(app);
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize, Eq, PartialEq, Hash)]
pub enum WorkflowSource {
    Global,
    Local,
    Project,
    Notebook {
        notebook_id: Option<NotebookId>,
        location: NotebookLocation,
    },

    /// A hardcoded workflow type that allows Warp to surface features as Workflows (e.g.
    /// a command to see our network log)
    App,
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize, Eq, PartialEq, Hash, PartialOrd)]
pub enum WorkflowSelectionSource {
    WarpDrive,
    CommandPalette,
    UniversalSearch,
    Voltron,
    WarpAI,
    Notebook,
    SlashMenu,
    UpArrowHistory,
    WorkflowView,
    AgentMode,
    Undefined,
    Alias,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AIWorkflowOrigin {
    CommandSearch,
    AgentMode,
    LegacyWarpAI,
}

/// Wrapper type for a workflow that may be saved locally or using cloud sync.
#[derive(Clone, Debug, PartialEq)]
pub enum WorkflowType {
    /// Saved workflows sourced from local, global, project, app collections, saved locally.
    Local(Workflow),
    /// Ephemeral/transient workflows created from Warp AI output
    AIGenerated {
        workflow: Workflow,
        origin: AIWorkflowOrigin,
    },
    /// A workflow that's part of a cloud notebook.
    Notebook(Workflow),
}

impl WorkflowType {
    pub fn as_workflow(&self) -> &Workflow {
        match self {
            WorkflowType::Local(workflow) => workflow,
            WorkflowType::AIGenerated { workflow, .. } => workflow,
            WorkflowType::Notebook(workflow) => workflow,
        }
    }

    /// Returns the contained [`Workflow`], consuming `self`.
    pub fn take_workflow(self) -> Workflow {
        match self {
            WorkflowType::Local(workflow) => workflow,
            WorkflowType::AIGenerated { workflow, .. } => workflow,
            WorkflowType::Notebook(workflow) => workflow,
        }
    }

}


