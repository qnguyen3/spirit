use serde::{Deserialize, Serialize};
use settings::macros::define_settings_group;
use settings::{RespectUserSyncSetting, SupportedPlatforms, SyncToCloud};

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    PartialEq,
    Deserialize,
    Serialize,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(
    description = "How third-party CLI agents handle approval prompts when launched from Warp.",
    rename_all = "snake_case"
)]
pub enum AgentApprovalMode {
    #[default]
    Yolo,
    Normal,
}

impl AgentApprovalMode {
    pub fn dropdown_item_label(&self) -> &'static str {
        match self {
            Self::Yolo => "YOLO",
            Self::Normal => "Normal",
        }
    }

    pub fn toggled(&self) -> AgentApprovalMode {
        match self {
            Self::Yolo => Self::Normal,
            Self::Normal => Self::Yolo,
        }
    }
}

define_settings_group!(CLIAgentSettings, settings: [
    agent_approval_mode: AgentApprovalModeSetting {
        type: AgentApprovalMode,
        default: AgentApprovalMode::default(),
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Globally(RespectUserSyncSetting::Yes),
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.cli_agents.approval_mode",
        description: "How third-party CLI agents handle approval prompts when launched from Warp. YOLO launches them with their approval prompts bypassed, so they act without asking. Normal launches them with their own approval prompts intact.",
    },
]);
