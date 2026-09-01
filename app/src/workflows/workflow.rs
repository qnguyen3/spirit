use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// The workflow model used throughout the app, including for workflows loaded from local YAML
/// files (`~/.warp/workflows`, `<repo>/.warp/workflows`, the bundled corpus, and the public
/// `warp-workflows` crate).
///
/// Serde field names are a stored contract: local YAML corpora parse against them, and unknown
/// keys are ignored so files written by older versions still load.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum Workflow {
    AgentMode {
        name: String,
        /// The query to be inserted in the terminal input.
        query: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default)]
        arguments: Vec<Argument>,
    },
    #[serde(untagged)]
    Command {
        name: String,
        command: String,
        #[serde(default)]
        tags: Vec<String>,
        description: Option<String>,
        #[serde(default)]
        arguments: Vec<Argument>,
        source_url: Option<String>,
        author: Option<String>,
        author_url: Option<String>,
        #[serde(default)]
        shells: Vec<warp_workflows::Shell>,
    },
}

impl Workflow {
    pub fn name(&self) -> &str {
        match self {
            Self::AgentMode { name, .. } => name.as_str(),
            Self::Command { name, .. } => name.as_str(),
        }
    }

    /// The core "content" of the workflow.
    ///
    /// For Command workflows, this is the shell command. For Agent Mode workflows, this is the
    /// query.
    pub fn content(&self) -> &str {
        match self {
            Self::AgentMode { query, .. } => query,
            Self::Command { command, .. } => command,
        }
    }

    pub fn prompt(&self) -> Option<&str> {
        if let Self::AgentMode { query, .. } = self {
            Some(query.as_str())
        } else {
            None
        }
    }

    pub fn command(&self) -> Option<&str> {
        if let Self::Command { command, .. } = self {
            Some(command.as_str())
        } else {
            None
        }
    }

    pub fn description(&self) -> Option<&String> {
        match self {
            Self::AgentMode { description, .. } => description.as_ref(),
            Self::Command { description, .. } => description.as_ref(),
        }
    }

    pub fn arguments(&self) -> &Vec<Argument> {
        match self {
            Self::AgentMode { arguments, .. } => arguments,
            Self::Command { arguments, .. } => arguments,
        }
    }

    pub fn tags(&self) -> Option<&Vec<String>> {
        match self {
            Self::Command { tags, .. } => Some(tags),
            Self::AgentMode { .. } => None,
        }
    }

    pub fn source_url(&self) -> Option<&String> {
        match self {
            Self::Command { source_url, .. } => source_url.as_ref(),
            Self::AgentMode { .. } => None,
        }
    }

    pub fn author_name(&self) -> Option<&String> {
        match self {
            Self::Command { author, .. } => author.as_ref(),
            Self::AgentMode { .. } => None,
        }
    }

    pub fn shells(&self) -> Option<&Vec<warp_workflows::Shell>> {
        match self {
            Self::Command { shells, .. } => Some(shells),
            Self::AgentMode { .. } => None,
        }
    }

    pub fn is_command_workflow(&self) -> bool {
        matches!(self, Self::Command { .. })
    }

    pub fn is_agent_mode_workflow(&self) -> bool {
        matches!(self, Self::AgentMode { .. })
    }

    /// Returns `true` if the workflow name starts with the given character (case-insensitive).
    ///
    /// Used by prompt search datasources to prefix-match on single-character queries, where
    /// fuzzy matching would be unreliable.
    pub fn name_starts_with_char_ignore_case(&self, c: char) -> bool {
        self.name()
            .chars()
            .next()
            .is_some_and(|first| first.eq_ignore_ascii_case(&c))
    }

    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        Workflow::Command {
            name: name.into(),
            command: command.into(),
            tags: Vec::new(),
            arguments: Vec::new(),
            description: None,
            source_url: None,
            author: None,
            author_url: None,
            shells: Vec::new(),
        }
    }

    pub fn with_arguments(mut self, new_arguments: Vec<Argument>) -> Self {
        match self {
            Workflow::AgentMode {
                ref mut arguments, ..
            }
            | Workflow::Command {
                ref mut arguments, ..
            } => {
                *arguments = new_arguments;
            }
        }
        self
    }

    pub fn with_description(mut self, new_description: String) -> Self {
        match self {
            Workflow::AgentMode {
                ref mut description,
                ..
            }
            | Workflow::Command {
                ref mut description,
                ..
            } => {
                *description = Some(new_description);
            }
        }
        self
    }

    pub fn set_name(&mut self, new_name: &str) {
        match self {
            Workflow::AgentMode { name, .. } | Workflow::Command { name, .. } => {
                new_name.clone_into(name)
            }
        }
    }
}

/// Create a Warp workflow from a public-facing workflow
/// https://github.com/warpdotdev/workflows/blob/main/workflow-types/src/lib.rs
impl From<warp_workflows::Workflow> for Workflow {
    fn from(workflow: warp_workflows::Workflow) -> Self {
        Workflow::Command {
            name: workflow.name,
            command: workflow.command,
            description: workflow.description,
            arguments: workflow.arguments.into_iter().map(Argument::from).collect(),
            tags: workflow.tags,
            source_url: workflow.source_url,
            author: workflow.author,
            author_url: workflow.author_url,
            shells: workflow.shells,
        }
    }
}

/// Temporary bridge from the cloud workflow model, for the code paths where cloud and local
/// workflows still meet. Delete along with the cloud workflow model in Phase 5.
impl From<&cloud_object_models::Workflow> for Workflow {
    fn from(workflow: &cloud_object_models::Workflow) -> Self {
        match workflow {
            cloud_object_models::Workflow::AgentMode {
                name,
                query,
                description,
                arguments,
            } => Workflow::AgentMode {
                name: name.clone(),
                query: query.clone(),
                description: description.clone(),
                arguments: arguments.iter().map(Argument::from).collect(),
            },
            cloud_object_models::Workflow::Command {
                name,
                command,
                tags,
                description,
                arguments,
                source_url,
                author,
                author_url,
                shells,
                ..
            } => Workflow::Command {
                name: name.clone(),
                command: command.clone(),
                tags: tags.clone(),
                description: description.clone(),
                arguments: arguments.iter().map(Argument::from).collect(),
                source_url: source_url.clone(),
                author: author.clone(),
                author_url: author_url.clone(),
                shells: shells.clone(),
            },
        }
    }
}

/// Temporary bridge to the cloud workflow model. Delete along with it in Phase 5.
impl From<&Workflow> for cloud_object_models::Workflow {
    fn from(workflow: &Workflow) -> Self {
        match workflow {
            Workflow::AgentMode {
                name,
                query,
                description,
                arguments,
            } => cloud_object_models::Workflow::AgentMode {
                name: name.clone(),
                query: query.clone(),
                description: description.clone(),
                arguments: arguments.iter().map(Into::into).collect(),
            },
            Workflow::Command {
                name,
                command,
                tags,
                description,
                arguments,
                source_url,
                author,
                author_url,
                shells,
            } => cloud_object_models::Workflow::Command {
                name: name.clone(),
                command: command.clone(),
                tags: tags.clone(),
                description: description.clone(),
                arguments: arguments.iter().map(Into::into).collect(),
                source_url: source_url.clone(),
                author: author.clone(),
                author_url: author_url.clone(),
                shells: shells.clone(),
                environment_variables: None,
            },
        }
    }
}

/// A named, user-supplied value substituted into a workflow's command or query.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, Hash, Default)]
pub struct Argument {
    pub name: String,
    /// The type of the argument to the workflow
    #[serde(flatten, deserialize_with = "deserialize_arg_type")]
    pub arg_type: ArgumentType,
    pub description: Option<String>,
    pub default_value: Option<String>,
}

impl From<warp_workflows::Argument> for Argument {
    fn from(arg: warp_workflows::Argument) -> Self {
        Argument {
            name: arg.name,
            arg_type: ArgumentType::Text,
            description: arg.description,
            default_value: arg.default_value,
        }
    }
}

/// Temporary bridge from the cloud argument model. Delete along with it in Phase 5.
impl From<&cloud_object_models::Argument> for Argument {
    fn from(arg: &cloud_object_models::Argument) -> Self {
        Argument {
            name: arg.name.clone(),
            arg_type: ArgumentType::Text,
            description: arg.description.clone(),
            default_value: arg.default_value.clone(),
        }
    }
}

/// Temporary bridge to the cloud argument model. Delete along with it in Phase 5.
impl From<&Argument> for cloud_object_models::Argument {
    fn from(arg: &Argument) -> Self {
        cloud_object_models::Argument {
            name: arg.name.clone(),
            arg_type: cloud_object_models::ArgumentType::Text,
            description: arg.description.clone(),
            default_value: arg.default_value.clone(),
        }
    }
}

impl Argument {
    pub fn new(name: impl Into<String>, arg_type: ArgumentType) -> Self {
        Argument {
            arg_type,
            name: name.into(),
            description: None,
            default_value: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default_value = Some(default.into());
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &Option<String> {
        &self.description
    }

    pub fn arg_type(&self) -> &ArgumentType {
        &self.arg_type
    }

    pub fn default_value(&self) -> &Option<String> {
        &self.default_value
    }
}

/// The type of the workflow argument
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, Hash, Default)]
#[serde(tag = "arg_type")]
pub enum ArgumentType {
    #[default]
    Text,
}

/// Custom deserialization for argument types, used to both `flatten` the argument type
/// and allow for the specification of `default` behavior.
///
/// Necessary because serde currently does not support the use of `flatten` with a `default`,
/// related GitHub issue here: https://github.com/serde-rs/serde/issues/1626
fn deserialize_arg_type<'de, D>(deserializer: D) -> Result<ArgumentType, D::Error>
where
    D: Deserializer<'de>,
{
    let _: Value = Deserialize::deserialize(deserializer)?;
    Ok(ArgumentType::default())
}
